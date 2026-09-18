<script lang="ts">
  import { onMount } from "svelte";
  import Split from "split.js";
  import uPlot from "uplot";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { clearScope, configureScope, pauseScope, readScopeSnapshot, startScope } from "./api";
  import type { ScopeConfig, ScopeSnapshot, ScopeSummary } from "./types";
  import MotionCompactEditor from "../tuning/MotionCompactEditor.svelte";

  export let connection: ConnectionInfo | undefined;
  export let active = false;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let plotHost: HTMLDivElement;
  let plot: uPlot | undefined;
  let split: Split.Instance | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let snapshotBusy = false;
  let commandBusy = false;
  let configuring = false;
  let snapshotRevision = 0;
  let configurationDirty = false;
  let channelNotice = "";
  let initialVerticalFit = true;
  let scopeConfig: ScopeConfig | undefined;
  let selectedIds = new Set<number>();
  let plotChannels: PlotChannel[] = [];
  let visibleChannels: PlotChannel[] = [];
  let snapshot: ScopeSnapshot | undefined;
  let carriedPlotData: uPlot.AlignedData | undefined;
  let snapshotChannelIds: number[] = [];
  let latestValues: string[] = [];
  let activeConnection: ConnectionInfo | undefined;
  const scopeWindowSeconds = 0.5;
  const traceColors = ["#7aa2c8", "#c8b77a", "#9b8ac8", "#7fa68a", "#c28b73", "#aa829a", "#79a6ad", "#91a77b"];

  $: if (connection !== activeConnection) {
    activeConnection = connection;
    resetScope();
  }
  $: onSummary({ state: snapshot?.state ?? "STOPPED", selectedChannels: selectedIds.size, lostFrames: snapshot?.lostFrames ?? 0 });
  $: visibleChannels = plotChannels.filter((channel) => selectedIds.has(channel.id));
  $: latestValues = visibleChannels.map((channel) => {
    const index = snapshotChannelIds.indexOf(channel.id);
    const values = index >= 0 ? snapshot?.series[index] : undefined;
    if (!values?.length) return "—";
    const unit = channel.unit ?? "";
    return `${values[values.length - 1].toFixed(3)}${unit ? ` ${unit}` : ""}`;
  });

  function channelMode(channel: PlotChannel): string {
    if (channel.supportsFast && channel.supportsNormal) return "FAST+N";
    if (channel.supportsFast) return "FAST";
    return "NORMAL";
  }

  function resetScope() {
    snapshotRevision += 1;
    configurationDirty = false;
    channelNotice = "";
    initialVerticalFit = true;
    scopeConfig = undefined;
    snapshot = undefined;
    carriedPlotData = undefined;
    snapshotChannelIds = [];
    selectedIds = new Set(connection?.channels.filter((c) => c.supportsFast).slice(0, Math.min(2, connection.fastMaxChannels)).map((c) => c.id) ?? []);
    plotChannels = connection?.channels.filter((c) => c.supportsFast) ?? [];
    rebuildPlot();
  }

  function toggleChannel(id: number) {
    if (!connection || commandBusy) return;
    const next = new Set(selectedIds);
    if (next.has(id)) next.delete(id);
    else if (next.size < connection.fastMaxChannels) next.add(id);
    else {
      channelNotice = `Maximum ${connection.fastMaxChannels} FAST channels.`;
      return;
    }
    channelNotice = "";
    const needsLiveChannel = snapshot?.state === "LIVE" && !!scopeConfig &&
      next.has(id) && !scopeConfig.channels.some((channel) => channel.id === id);
    selectedIds = next;
    configurationDirty = !!scopeConfig &&
      (next.size !== scopeConfig.channels.length || scopeConfig.channels.some((channel) => !next.has(channel.id)));
    const seriesIndex = plotChannels.findIndex((channel) => channel.id === id);
    if (seriesIndex >= 0) plot?.setSeries(seriesIndex + 1, { show: next.has(id) });
    if (needsLiveChannel) void run();
  }

  function autoFitPlot() {
    const chart = plot;
    if (!chart) return;
    let min = Infinity;
    let max = -Infinity;
    for (let index = 0; index < plotChannels.length; index += 1) {
      if (!selectedIds.has(plotChannels[index].id)) continue;
      for (const value of chart.data[index + 1] ?? []) {
        if (value == null || !Number.isFinite(value)) continue;
        min = Math.min(min, value);
        max = Math.max(max, value);
      }
    }
    if (!Number.isFinite(min)) { min = -1; max = 1; }
    const padding = Math.max((max - min) * 0.1, Math.abs(max) * 0.02, 0.001);
    chart.batch(() => {
      chart.setScale("x", { min: -scopeWindowSeconds, max: 0 });
      chart.setScale("y", { min: min - padding, max: max + padding });
    });
    initialVerticalFit = false;
  }

  async function ensureConfigured(): Promise<boolean> {
    if (scopeConfig && !configurationDirty) return true;
    if (!connection) { onError("Connect a device before starting Scope."); return false; }
    if (selectedIds.size === 0) { onError("Select at least one FAST channel."); return false; }
    const revision = snapshotRevision;
    try {
      const configured = await configureScope(Array.from(selectedIds));
      if (revision !== snapshotRevision) return false;
      scopeConfig = configured;
      configurationDirty = false;
      return true;
    } catch (error) {
      if (revision === snapshotRevision) {
        scopeConfig = undefined;
        carriedPlotData = undefined;
        if (snapshot) snapshot = { ...snapshot, state: "STOPPED" };
        onError(error);
      }
      return false;
    }
  }

  async function run() {
    if (commandBusy) return;
    commandBusy = true;
    if (!scopeConfig || configurationDirty) {
      snapshotRevision += 1;
      configuring = true;
      carriedPlotData = plot?.data[0].length ? plot.data : undefined;
    }
    try {
      if (!(await ensureConfigured())) return;
      await startScope();
      configuring = false;
      await refreshSnapshot();
    } catch (error) { onError(error); }
    finally { configuring = false; commandBusy = false; }
  }

  async function pause() {
    if (!scopeConfig || commandBusy) return;
    commandBusy = true;
    try { await pauseScope(); await refreshSnapshot(); }
    catch (error) { onError(error); }
    finally { commandBusy = false; }
  }

  async function clear() {
    if (!scopeConfig || commandBusy) return;
    commandBusy = true;
    try {
      await clearScope();
      snapshotRevision += 1;
      carriedPlotData = undefined;
      if (snapshot) snapshot = { ...snapshot, sampleCount: 0, times: [], series: snapshot.series.map(() => []) };
      plot?.setData([[], ...plotChannels.map(() => [])] as uPlot.AlignedData);
      await refreshSnapshot();
    }
    catch (error) { onError(error); }
    finally { commandBusy = false; }
  }

  async function refreshSnapshot() {
    if (!scopeConfig || configuring || snapshotBusy) return;
    snapshotBusy = true;
    const revision = snapshotRevision;
    const channels = scopeConfig.channels.map((channel) => channel.id);
    try {
      const next = await readScopeSnapshot(scopeWindowSeconds);
      if (revision !== snapshotRevision) return;
      snapshot = next;
      snapshotChannelIds = channels;
      if (active && (next.times.length > 0 || plot?.data[0].length === 0)) {
        let times = next.times;
        let series: Array<Array<number | null | undefined>> = plotChannels.map((channel) => {
          const index = channels.indexOf(channel.id);
          return index >= 0 ? next.series[index] : next.times.map(() => null);
        });
        const previous = carriedPlotData;
        if (previous && next.times.length > 0) {
          const elapsed = next.sampleCount / next.sampleRateHz;
          const firstNewTime = next.times[0];
          const keptIndices: number[] = [];
          for (let index = 0; index < previous[0].length; index += 1) {
            const shifted = previous[0][index] - elapsed;
            if (shifted >= -scopeWindowSeconds && shifted < firstNewTime) keptIndices.push(index);
          }
          if (keptIndices.length > 0) {
            times = keptIndices.map((index) => previous[0][index] - elapsed).concat(next.times);
            series = series.map((values, index) =>
              keptIndices.map((oldIndex) => previous[index + 1][oldIndex]).concat(values));
          } else {
            carriedPlotData = undefined;
          }
        }
        plot?.setData([times, ...series] as uPlot.AlignedData);
        if (initialVerticalFit && times.length > 1) autoFitPlot();
      }
    }
    catch (error) { if (revision === snapshotRevision) onError(error); }
    finally { snapshotBusy = false; }
  }

  function traceColor(channel: PlotChannel): string {
    const index = connection?.channels.findIndex((candidate) => candidate.id === channel.id) ?? 0;
    return traceColors[Math.max(0, index) % traceColors.length];
  }

  function rebuildPlot() {
    if (!plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot?.destroy();
    const series: uPlot.Series[] = [{}, ...plotChannels.map((channel) => ({ label: channel.symbol, stroke: traceColor(channel), width: 1.25, show: selectedIds.has(channel.id) }))];
    plot = new uPlot({
      width: Math.max(420, Math.floor(rect.width)),
      height: Math.max(260, Math.floor(rect.height)),
      legend: { show: false },
      cursor: { drag: { x: true, y: false } },
      scales: {
        x: { time: false, auto: false, range: [-scopeWindowSeconds, 0] },
        y: { auto: false, range: [-1, 1] },
      },
      axes: [
        { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
        { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
      ],
      series,
    }, [[], ...plotChannels.map(() => [])] as uPlot.AlignedData, plotHost);
  }

  function resizePlot() {
    if (!plot || !plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot.setSize({ width: Math.max(420, Math.floor(rect.width)), height: Math.max(260, Math.floor(rect.height)) });
  }

  onMount(() => {
    split = Split(["#scope-sidebar", "#scope-workspace"], { sizes: [25, 75], minSize: [260, 420], gutterSize: 4, snapOffset: 0, onDrag: resizePlot });
    rebuildPlot();
    resizeObserver = new ResizeObserver(resizePlot);
    resizeObserver.observe(plotHost);
    let hiddenTicks = 0;
    refreshTimer = setInterval(() => {
      if (active) {
        hiddenTicks = 0;
        void refreshSnapshot();
      } else if (++hiddenTicks >= 10) {
        hiddenTicks = 0;
        void refreshSnapshot();
      }
    }, 50);
    return () => { if (refreshTimer) clearInterval(refreshTimer); resizeObserver?.disconnect(); plot?.destroy(); split?.destroy(); };
  });
</script>

<section class="page-toolbar"><div class="page-title">ANALYSIS / SCOPE</div><div class="toolbar-actions">
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <vscode-button disabled={!connection || selectedIds.size === 0} onclick={run}><i class="codicon codicon-play"></i>&nbsp;Run</vscode-button>
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <vscode-button secondary disabled={!scopeConfig} onclick={pause}><i class="codicon codicon-debug-pause"></i>&nbsp;Pause</vscode-button>
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <vscode-button secondary title="Reset timebase and fit visible traces" onclick={autoFitPlot}>AUTO</vscode-button>
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <vscode-button secondary disabled={!scopeConfig} onclick={clear}>Clear</vscode-button>
</div></section>
<div class="scope-shell">
  <aside id="scope-sidebar" class="scope-sidebar">
    <section class="side-section"><div class="section-heading">CHANNELS</div><div class="channel-list">
      {#if connection}{#each connection.channels as channel}
        <label class:disabled-row={!channel.supportsFast} class="channel-row" for={`scope-channel-${channel.id}`}>
          <input
            id={`scope-channel-${channel.id}`}
            class="scope-channel-checkbox"
            type="checkbox"
            checked={selectedIds.has(channel.id)}
            disabled={!channel.supportsFast}
            onchange={() => toggleChannel(channel.id)}
          />
          <span class="channel-name">{channel.symbol}</span><span class="channel-unit">{channel.unit ?? ""}</span><span class:normal-only={!channel.supportsFast} class="channel-mode">{channelMode(channel)}</span>
        </label>
      {/each}{:else}<div class="empty-hint">No device connected. Open Connection first to discover acquisition channels.</div>{/if}
    </div>{#if channelNotice}<div class="scope-pending">{channelNotice}</div>{/if}</section>
    <section class="side-section acquisition"><div class="section-heading">ACQUISITION</div><div class="property-grid">
      <span>State</span><strong>{snapshot?.state ?? "STOPPED"}</strong><span>FAST Rate</span><strong>{connection ? `${(connection.fastRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>History</span><strong>{scopeConfig ? `${scopeConfig.historySeconds.toFixed(3)} s` : "10.000 s"}</strong><span>Channels</span><strong>{selectedIds.size} / {connection?.fastMaxChannels ?? "—"}</strong><span>Block</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
    </div>{#if configurationDirty && snapshot?.state !== "LIVE"}<div class="scope-pending">New channels apply on Run.</div>{/if}</section>
    <MotionCompactEditor {onError} />
  </aside>
  <section id="scope-workspace" class="scope-workspace">
    <div class="editor-tabs"><div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div></div>
    <div class="plot-header">{#if visibleChannels.length > 0}{#each visibleChannels as channel, index}<div class="trace-key"><span class="trace-mark" style={`background:${traceColor(channel)}`}></span>{channel.symbol}<span class="value">{latestValues[index] ?? "—"}</span></div>{/each}{:else}<div class="plot-placeholder">{connection ? "Select FAST channels and press Run." : "Connect a device before using Scope."}</div>{/if}<div class="plot-meta">{snapshot?.sampleCount ?? 0} samples · loss {snapshot?.lostFrames ?? 0}</div></div>
    <div bind:this={plotHost} class="plot-host"></div>
  </section>
</div>
