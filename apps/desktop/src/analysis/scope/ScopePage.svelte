<script lang="ts">
  import { onMount } from "svelte";
  import Split from "split.js";
  import uPlot from "uplot";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { clearScope, configureScope, pauseScope, readScopeSnapshot, startScope } from "./api";
  import type { ScopeConfig, ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";
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
  let channelRates = new Map<number, ScopeRate>();
  let plotChannels: PlotChannel[] = [];
  let visibleChannels: PlotChannel[] = [];
  let snapshot: ScopeSnapshot | undefined;
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
    const values = snapshot?.series.find((series) => series.id === channel.id)?.values;
    if (!values?.length) return "—";
    const unit = channel.unit ?? "";
    return `${values[values.length - 1].toFixed(3)}${unit ? ` ${unit}` : ""}`;
  });
  $: fastSelected = Array.from(selectedIds).filter((id) => channelRates.get(id) === "fast").length;
  $: normalSelected = Array.from(selectedIds).filter((id) => channelRates.get(id) === "normal").length;

  function defaultRate(channel: PlotChannel): ScopeRate {
    return channel.supportsFast ? "fast" : "normal";
  }

  function selectedRate(id: number): ScopeRate {
    const channel = plotChannels.find((item) => item.id === id);
    return channelRates.get(id) ?? (channel ? defaultRate(channel) : "normal");
  }

  function rateLabel(rate: ScopeRate): string {
    if (!connection) return rate === "fast" ? "20K" : "1K";
    const hz = rate === "fast" ? connection.fastRateHz : connection.normalRateHz;
    return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)}K` : `${hz} Hz`;
  }

  function rateAllowed(channel: PlotChannel, rate: ScopeRate): boolean {
    return rate === "fast" ? channel.supportsFast : channel.supportsNormal;
  }

  function resetScope() {
    snapshotRevision += 1;
    configurationDirty = false;
    channelNotice = "";
    initialVerticalFit = true;
    scopeConfig = undefined;
    snapshot = undefined;
    plotChannels = connection?.channels.filter((channel) => channel.supportsFast || channel.supportsNormal) ?? [];
    const defaults = plotChannels.slice(0, 2);
    selectedIds = new Set(defaults.map((channel) => channel.id));
    channelRates = new Map(defaults.map((channel) => [channel.id, defaultRate(channel)]));
    rebuildPlot();
  }

  function toggleChannel(id: number) {
    if (!connection || commandBusy) return;
    const channel = plotChannels.find((item) => item.id === id);
    if (!channel) return;

    const next = new Set(selectedIds);
    const rates = new Map(channelRates);
    if (next.has(id)) {
      next.delete(id);
      rates.delete(id);
    } else {
      const rate = rates.get(id) ?? defaultRate(channel);
      const currentCount = Array.from(next).filter((item) => rates.get(item) === rate).length;
      const limit = rate === "fast" ? connection.fastMaxChannels : connection.normalMaxChannels;
      if (currentCount >= limit) {
        channelNotice = `Maximum ${limit} ${rateLabel(rate)} channels.`;
        return;
      }
      next.add(id);
      rates.set(id, rate);
    }

    channelNotice = "";
    selectedIds = next;
    channelRates = rates;
    configurationDirty = configurationChanged(next, rates);
    const seriesIndex = plotChannels.findIndex((item) => item.id === id);
    if (seriesIndex >= 0) plot?.setSeries(seriesIndex + 1, { show: next.has(id) });
  }

  function changeRate(id: number, rate: ScopeRate) {
    if (!connection || commandBusy) return;
    const channel = plotChannels.find((item) => item.id === id);
    if (!channel || !rateAllowed(channel, rate)) return;

    const rates = new Map(channelRates);
    if (selectedIds.has(id)) {
      const count = Array.from(selectedIds).filter((item) => item !== id && rates.get(item) === rate).length;
      const limit = rate === "fast" ? connection.fastMaxChannels : connection.normalMaxChannels;
      if (count >= limit) {
        channelNotice = `Maximum ${limit} ${rateLabel(rate)} channels.`;
        return;
      }
    }

    rates.set(id, rate);
    channelRates = rates;
    channelNotice = "";
    configurationDirty = configurationChanged(selectedIds, rates);
  }

  function configurationChanged(ids: Set<number>, rates: Map<number, ScopeRate>): boolean {
    if (!scopeConfig) return false;
    if (ids.size !== scopeConfig.channels.length) return true;
    return scopeConfig.channels.some((channel) =>
      !ids.has(channel.id) || rates.get(channel.id) !== channel.rate);
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
    if (selectedIds.size === 0) { onError("Select at least one Scope channel."); return false; }
    const revision = snapshotRevision;
    try {
      const configured = await configureScope(Array.from(selectedIds).map((id) => ({ id, rate: selectedRate(id) })));
      if (revision !== snapshotRevision) return false;
      scopeConfig = configured;
      configurationDirty = false;
      return true;
    } catch (error) {
      if (revision === snapshotRevision) {
        scopeConfig = undefined;
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
      if (snapshot) snapshot = { ...snapshot, sampleCount: 0, series: snapshot.series.map((series) => ({ ...series, times: [], values: [] })) };
      plot?.setData([[], ...plotChannels.map(() => [])] as uPlot.AlignedData);
      await refreshSnapshot();
    }
    catch (error) { onError(error); }
    finally { commandBusy = false; }
  }

  function alignedPlotData(next: ScopeSnapshot): uPlot.AlignedData {
    const timeKeys = new Map<string, number>();
    for (const series of next.series) {
      for (const time of series.times) timeKeys.set(time.toFixed(7), time);
    }
    const times = Array.from(timeKeys.values()).sort((a, b) => a - b);
    const indexByKey = new Map(times.map((time, index) => [time.toFixed(7), index]));

    const values = plotChannels.map((channel) => {
      const output: Array<number | null> = times.map(() => null);
      const source = next.series.find((series) => series.id === channel.id);
      if (!source) return output;
      for (let index = 0; index < source.times.length; index += 1) {
        const target = indexByKey.get(source.times[index].toFixed(7));
        if (target !== undefined) output[target] = source.values[index];
      }
      return output;
    });

    return [times, ...values] as uPlot.AlignedData;
  }

  async function refreshSnapshot() {
    if (!scopeConfig || configuring || snapshotBusy) return;
    snapshotBusy = true;
    const revision = snapshotRevision;
    try {
      const next = await readScopeSnapshot(scopeWindowSeconds);
      if (revision !== snapshotRevision) return;
      snapshot = next;
      if (active) {
        const data = alignedPlotData(next);
        plot?.setData(data);
        if (initialVerticalFit && data[0].length > 1) autoFitPlot();
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
    const series: uPlot.Series[] = [{}, ...plotChannels.map((channel) => ({ label: channel.symbol, stroke: traceColor(channel), width: 1.25, show: selectedIds.has(channel.id), spanGaps: true }))];
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
      {#if connection}{#each plotChannels as channel}
        <div class="channel-row">
          <input
            id={`scope-channel-${channel.id}`}
            class="scope-channel-checkbox"
            type="checkbox"
            checked={selectedIds.has(channel.id)}
            onchange={() => toggleChannel(channel.id)}
          />
          <label class="channel-name" for={`scope-channel-${channel.id}`}>{channel.symbol}</label>
          <span class="channel-unit">{channel.unit ?? ""}</span>
          <select
            class="channel-rate"
            value={selectedRate(channel.id)}
            disabled={commandBusy}
            onchange={(event) => changeRate(channel.id, (event.currentTarget as HTMLSelectElement).value as ScopeRate)}
          >
            <option value="fast" disabled={!channel.supportsFast}>{rateLabel("fast")}</option>
            <option value="normal" disabled={!channel.supportsNormal}>{rateLabel("normal")}</option>
          </select>
        </div>
      {/each}{:else}<div class="empty-hint">No device connected. Open Connection first to discover acquisition channels.</div>{/if}
    </div>{#if channelNotice}<div class="scope-pending">{channelNotice}</div>{/if}</section>
    <section class="side-section acquisition"><div class="section-heading">ACQUISITION</div><div class="property-grid">
      <span>State</span><strong>{snapshot?.state ?? "STOPPED"}</strong><span>History</span><strong>{scopeConfig ? `${scopeConfig.historySeconds.toFixed(3)} s` : "10.000 s"}</strong>
      <span>{rateLabel("fast")}</span><strong>{fastSelected} / {connection?.fastMaxChannels ?? "—"}</strong><span>{rateLabel("normal")}</span><strong>{normalSelected} / {connection?.normalMaxChannels ?? "—"}</strong><span>FAST Block</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
    </div>{#if configurationDirty && snapshot?.state !== "LIVE"}<div class="scope-pending">New channels apply on Run.</div>{/if}</section>
    <MotionCompactEditor capabilities={connection?.motion} {onError} />
  </aside>
  <section id="scope-workspace" class="scope-workspace">
    <div class="editor-tabs"><div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div></div>
    <div class="plot-header">{#if visibleChannels.length > 0}{#each visibleChannels as channel, index}<div class="trace-key"><span class="trace-mark" style={`background:${traceColor(channel)}`}></span>{channel.symbol}<span class="value">{latestValues[index] ?? "—"}</span></div>{/each}{:else}<div class="plot-placeholder">{connection ? "Select channels and press Run." : "Connect a device before using Scope."}</div>{/if}<div class="plot-meta">{snapshot?.sampleCount ?? 0} samples · loss {snapshot?.lostFrames ?? 0}</div></div>
    <div bind:this={plotHost} class="plot-host"></div>
  </section>
</div>
