<script lang="ts">
  import { onMount } from "svelte";
  import Split from "split.js";
  import uPlot from "uplot";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { clearScope, configureScope, readScopeSnapshot, startScope, stopScope } from "./api";
  import type { ScopeConfig, ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";
  import MotionCompactEditor from "../tuning/MotionCompactEditor.svelte";

  export let connection: ConnectionInfo | undefined;
  export let active = false;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  const horizontalDivisions = 10;
  const verticalDivisions = 8;
  const timeDivOptions = [
    50e-6, 100e-6, 200e-6, 500e-6,
    1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3,
    100e-3, 200e-3, 500e-3, 1,
  ];
  const traceColors = ["#7aa2c8", "#c8b77a", "#9b8ac8", "#7fa68a", "#c28b73", "#aa829a", "#79a6ad", "#91a77b"];

  let plotHost: HTMLDivElement;
  let plot: uPlot | undefined;
  let split: Split.Instance | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let reconfigureTimer: ReturnType<typeof setTimeout> | undefined;

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
  let verticalScale = new Map<number, number>();
  let verticalOffset = new Map<number, number>();
  let plotChannels: PlotChannel[] = [];
  let visibleChannels: PlotChannel[] = [];
  let snapshot: ScopeSnapshot | undefined;
  let latestValues: string[] = [];
  let fastSelected = 0;
  let normalSelected = 0;
  let activeChannelId: number | undefined;
  let activeConnection: ConnectionInfo | undefined;

  let timePerDiv = 10e-3;
  let horizontalOffset = 0;
  let cursorEnabled = false;
  let cursorA: number | undefined;
  let cursorB: number | undefined;
  let nextCursor: "a" | "b" = "a";
  let isRunning = false;

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
  $: isRunning = snapshot?.state === "LIVE";
  $: if (activeChannelId !== undefined && !selectedIds.has(activeChannelId)) {
    activeChannelId = visibleChannels[0]?.id;
  }

  function windowSeconds(): number {
    return timePerDiv * horizontalDivisions;
  }

  function historySeconds(): number {
    return scopeConfig?.historySeconds ?? 10;
  }

  function maxHorizontalOffset(): number {
    return Math.max(0, historySeconds() - windowSeconds());
  }

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

  function formatTime(value: number): string {
    if (value >= 1) return `${value.toFixed(2)} s`;
    if (value >= 1e-3) return `${(value * 1e3).toFixed(value < 10e-3 ? 2 : 1)} ms`;
    return `${Math.round(value * 1e6)} µs`;
  }

  function formatSignedTime(value: number): string {
    if (Math.abs(value) < 1e-12) return "0";
    return `${value < 0 ? "-" : "+"}${formatTime(Math.abs(value))}`;
  }

  function defaultVerticalScale(channel: PlotChannel): number {
    const unit = (channel.unit ?? "").toLowerCase();
    if (unit === "a") return 0.5;
    if (unit.includes("rad/s")) return 10;
    if (unit.includes("turn")) return 0.5;
    if (unit === "v") return 5;
    return 1;
  }

  function resetScope() {
    snapshotRevision += 1;
    configurationDirty = false;
    channelNotice = "";
    initialVerticalFit = true;
    scopeConfig = undefined;
    snapshot = undefined;
    horizontalOffset = 0;
    cursorA = undefined;
    cursorB = undefined;
    plotChannels = connection?.channels.filter((channel) => channel.supportsFast || channel.supportsNormal) ?? [];

    const defaults = plotChannels.slice(0, 2);
    selectedIds = new Set(defaults.map((channel) => channel.id));
    channelRates = new Map(defaults.map((channel) => [channel.id, defaultRate(channel)]));
    verticalScale = new Map(plotChannels.map((channel) => [channel.id, defaultVerticalScale(channel)]));
    verticalOffset = new Map(plotChannels.map((channel) => [channel.id, 0]));
    activeChannelId = defaults[0]?.id;
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
      activeChannelId = id;
    }

    channelNotice = "";
    selectedIds = next;
    channelRates = rates;
    configurationDirty = configurationChanged(next, rates);
    scheduleHotReconfigure();
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
    scheduleHotReconfigure();
  }

  function scheduleHotReconfigure() {
    if (!scopeConfig || !isRunning) return;
    if (reconfigureTimer) clearTimeout(reconfigureTimer);
    reconfigureTimer = setTimeout(() => {
      void hotReconfigure();
    }, 80);
  }

  async function hotReconfigure() {
    if (!scopeConfig || !isRunning || commandBusy || !configurationDirty) return;
    commandBusy = true;
    try {
      const configured = await configureScope(
        Array.from(selectedIds).map((id) => ({ id, rate: selectedRate(id) })),
      );
      scopeConfig = configured;
      configurationDirty = false;
      channelNotice = "";
    } catch (error) {
      onError(error);
    } finally {
      commandBusy = false;
    }
  }

  function configurationChanged(ids: Set<number>, rates: Map<number, ScopeRate>): boolean {
    if (!scopeConfig) return false;
    if (ids.size !== scopeConfig.channels.length) return true;
    return scopeConfig.channels.some((channel) => !ids.has(channel.id) || rates.get(channel.id) !== channel.rate);
  }

  function updateTimePerDiv(value: number) {
    timePerDiv = value;
    horizontalOffset = Math.min(horizontalOffset, maxHorizontalOffset());
    applyHorizontalScale();
    void refreshSnapshot();
  }

  function updateHorizontalOffset(value: number) {
    horizontalOffset = Math.min(Math.max(0, value), maxHorizontalOffset());
    applyHorizontalScale();
    void refreshSnapshot();
  }

  function applyHorizontalScale() {
    const chart = plot;
    if (!chart) return;
    const end = -horizontalOffset;
    chart.setScale("x", { min: end - windowSeconds(), max: end });
  }

  function yScaleKey(id: number): string {
    return `y-${id}`;
  }

  function updateVerticalScale(id: number, value: number) {
    if (!Number.isFinite(value) || value <= 0) return;
    verticalScale = new Map(verticalScale).set(id, value);
    applyVerticalScale(id);
  }

  function updateVerticalOffset(id: number, value: number) {
    if (!Number.isFinite(value)) return;
    verticalOffset = new Map(verticalOffset).set(id, value);
    applyVerticalScale(id);
  }

  function applyVerticalScale(id: number) {
    const chart = plot;
    if (!chart) return;
    const perDiv = verticalScale.get(id) ?? 1;
    const center = verticalOffset.get(id) ?? 0;
    const half = perDiv * verticalDivisions / 2;
    chart.setScale(yScaleKey(id), { min: center - half, max: center + half });
  }

  function autoSet() {
    if (!snapshot) return;
    const scales = new Map(verticalScale);
    const offsets = new Map(verticalOffset);

    for (const channel of visibleChannels) {
      const values = snapshot.series.find((series) => series.id === channel.id)?.values ?? [];
      if (!values.length) continue;
      let min = Infinity;
      let max = -Infinity;
      for (const value of values) {
        if (!Number.isFinite(value)) continue;
        min = Math.min(min, value);
        max = Math.max(max, value);
      }
      if (!Number.isFinite(min) || !Number.isFinite(max)) continue;
      const center = (min + max) / 2;
      const span = Math.max(max - min, Math.abs(center) * 0.1, 1e-6);
      offsets.set(channel.id, center);
      scales.set(channel.id, span / (verticalDivisions * 0.75));
    }

    verticalScale = scales;
    verticalOffset = offsets;
    horizontalOffset = 0;
    applyHorizontalScale();
    for (const id of selectedIds) applyVerticalScale(id);
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
      horizontalOffset = Math.min(horizontalOffset, maxHorizontalOffset());
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
      horizontalOffset = 0;
      configuring = false;
      await refreshSnapshot();
    } catch (error) { onError(error); }
    finally { configuring = false; commandBusy = false; }
  }

  async function stop() {
    if (!scopeConfig || commandBusy) return;
    commandBusy = true;
    try {
      await stopScope();
      await refreshSnapshot();
    }
    catch (error) { onError(error); }
    finally { commandBusy = false; }
  }

  async function toggleRunStop() {
    if (isRunning) await stop();
    else await run();
  }

  async function clear() {
    if (!scopeConfig || commandBusy) return;
    commandBusy = true;
    try {
      await clearScope();
      snapshotRevision += 1;
      cursorA = undefined;
      cursorB = undefined;
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
      const next = await readScopeSnapshot(windowSeconds(), horizontalOffset);
      if (revision !== snapshotRevision) return;
      snapshot = next;
      if (active) {
        const data = alignedPlotData(next);
        plot?.setData(data);
        applyHorizontalScale();
        if (initialVerticalFit && data[0].length > 1) {
          autoSet();
          initialVerticalFit = false;
        }
      }
    }
    catch (error) { if (revision === snapshotRevision) onError(error); }
    finally { snapshotBusy = false; }
  }

  function traceColor(channel: PlotChannel): string {
    const index = connection?.channels.findIndex((candidate) => candidate.id === channel.id) ?? 0;
    return traceColors[Math.max(0, index) % traceColors.length];
  }

  function cursorPlugin(): uPlot.Plugin {
    return {
      hooks: {
        draw: [
          (u) => {
            if (!cursorEnabled) return;
            const ctx = u.ctx;
            ctx.save();
            ctx.strokeStyle = "#a7a7a7";
            ctx.lineWidth = 1;
            ctx.setLineDash([4, 4]);
            for (const value of [cursorA, cursorB]) {
              if (value === undefined) continue;
              const x = u.valToPos(value, "x", true);
              ctx.beginPath();
              ctx.moveTo(x, u.bbox.top);
              ctx.lineTo(x, u.bbox.top + u.bbox.height);
              ctx.stroke();
            }
            ctx.restore();
          },
        ],
      },
    };
  }

  function setCursorFromPlot() {
    if (!cursorEnabled || !plot || plot.cursor.left === undefined) return;
    const value = plot.posToVal(plot.cursor.left, "x");
    if (nextCursor === "a") {
      cursorA = value;
      nextCursor = "b";
    } else {
      cursorB = value;
      nextCursor = "a";
    }
    plot.redraw(false, false);
  }

  function toggleCursor() {
    cursorEnabled = !cursorEnabled;
    if (!cursorEnabled) {
      cursorA = undefined;
      cursorB = undefined;
    }
    plot?.redraw(false, false);
  }

  function rebuildPlot() {
    if (!plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot?.destroy();

    const scales: Record<string, uPlot.Scale> = {
      x: { time: false, auto: false, range: [-windowSeconds(), 0] },
    };
    for (const channel of plotChannels) {
      const perDiv = verticalScale.get(channel.id) ?? defaultVerticalScale(channel);
      const center = verticalOffset.get(channel.id) ?? 0;
      const half = perDiv * verticalDivisions / 2;
      scales[yScaleKey(channel.id)] = { auto: false, range: [center - half, center + half] };
    }

    const activeChannel = plotChannels.find((channel) => channel.id === activeChannelId) ?? plotChannels[0];
    const series: uPlot.Series[] = [
      {},
      ...plotChannels.map((channel) => ({
        label: channel.symbol,
        stroke: traceColor(channel),
        width: 1.25,
        show: selectedIds.has(channel.id),
        spanGaps: true,
        scale: yScaleKey(channel.id),
      })),
    ];

    const axes: uPlot.Axis[] = [
      { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
    ];
    if (activeChannel) {
      axes.push({
        scale: yScaleKey(activeChannel.id),
        stroke: "#8c8c8c",
        grid: { stroke: "#2a2d2e", width: 1 },
        ticks: { stroke: "#3a3d41" },
      });
    }

    plot = new uPlot({
      width: Math.max(420, Math.floor(rect.width)),
      height: Math.max(260, Math.floor(rect.height)),
      legend: { show: false },
      cursor: { drag: { x: false, y: false } },
      scales,
      axes,
      series,
      plugins: [cursorPlugin()],
    }, [[], ...plotChannels.map(() => [])] as uPlot.AlignedData, plotHost);

    applyHorizontalScale();
  }

  function selectActiveChannel(id: number) {
    if (activeChannelId === id) return;
    activeChannelId = id;
    rebuildPlot();
    if (snapshot) plot?.setData(alignedPlotData(snapshot));
  }

  function resizePlot() {
    if (!plot || !plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot.setSize({ width: Math.max(420, Math.floor(rect.width)), height: Math.max(260, Math.floor(rect.height)) });
  }

  onMount(() => {
    split = Split(["#scope-sidebar", "#scope-workspace"], { sizes: [27, 73], minSize: [285, 420], gutterSize: 4, snapOffset: 0, onDrag: resizePlot });
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
    return () => {
      if (refreshTimer) clearInterval(refreshTimer);
      if (reconfigureTimer) clearTimeout(reconfigureTimer);
      resizeObserver?.disconnect();
      plot?.destroy();
      split?.destroy();
    };
  });
</script>

<section class="page-toolbar">
  <div class="page-title">ANALYSIS / SCOPE</div>
  <div class="toolbar-actions">
    <vscode-button disabled={!connection || selectedIds.size === 0 || commandBusy} onclick={toggleRunStop}>
      <i class={`codicon ${isRunning ? "codicon-debug-stop" : "codicon-play"}`}></i>&nbsp;{isRunning ? "Stop" : "Run"}
    </vscode-button>
    <vscode-button secondary disabled={!scopeConfig} onclick={autoSet}>Auto Set</vscode-button>
    <vscode-button secondary class:scope-tool-active={cursorEnabled} disabled={!scopeConfig} onclick={toggleCursor}>Cursor</vscode-button>
    <vscode-button secondary disabled={!scopeConfig || commandBusy} onclick={clear}>Clear</vscode-button>
  </div>
</section>

<div class="scope-shell">
  <aside id="scope-sidebar" class="scope-sidebar">
    <section class="side-section">
      <div class="section-heading">CHANNELS</div>
      <div class="channel-list">
        {#if connection}
          {#each plotChannels as channel}
            <div class:active-channel={activeChannelId === channel.id} class="channel-row">
              <input
                id={`scope-channel-${channel.id}`}
                class="scope-channel-checkbox"
                type="checkbox"
                checked={selectedIds.has(channel.id)}
                onchange={() => toggleChannel(channel.id)}
              />
              <button class="channel-name channel-select" onclick={() => selectActiveChannel(channel.id)}>{channel.symbol}</button>
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
          {/each}
        {:else}
          <div class="empty-hint">No device connected. Open Connection first to discover acquisition channels.</div>
        {/if}
      </div>
      {#if channelNotice}<div class="scope-pending">{channelNotice}</div>{/if}
      {#if configurationDirty}<div class="scope-pending">{isRunning ? "Applying channel/rate change…" : "Channel/rate changes apply on Run."}</div>{/if}
    </section>

    <section class="side-section">
      <div class="section-heading">HORIZONTAL</div>
      <div class="scope-control-grid">
        <label>
          <span>Time/div</span>
          <select value={timePerDiv} onchange={(event) => updateTimePerDiv(Number((event.currentTarget as HTMLSelectElement).value))}>
            {#each timeDivOptions as value}
              <option value={value}>{formatTime(value)}</option>
            {/each}
          </select>
        </label>
        <label>
          <span>Position</span>
          <strong>{horizontalOffset === 0 ? "Latest" : `-${formatTime(horizontalOffset)}`}</strong>
        </label>
      </div>
      <input
        class="scope-position-slider"
        type="range"
        min="0"
        max={maxHorizontalOffset()}
        step={Math.max(timePerDiv / 10, 0.00005)}
        value={horizontalOffset}
        disabled={!scopeConfig}
        oninput={(event) => updateHorizontalOffset(Number((event.currentTarget as HTMLInputElement).value))}
      />
    </section>

    <section class="side-section">
      <div class="section-heading">VERTICAL</div>
      {#if activeChannelId !== undefined}
        {@const activeChannel = plotChannels.find((channel) => channel.id === activeChannelId)}
        {#if activeChannel}
          <div class="scope-active-channel">
            <span class="trace-mark" style={`background:${traceColor(activeChannel)}`}></span>
            <strong>{activeChannel.symbol}</strong>
            <span>{activeChannel.unit ?? ""}</span>
          </div>
          <div class="scope-control-grid">
            <label>
              <span>Scale/div</span>
              <div class="scope-unit-input">
                <input
                  type="number"
                  min="0.000001"
                  step="any"
                  value={verticalScale.get(activeChannelId) ?? 1}
                  oninput={(event) => updateVerticalScale(activeChannelId!, Number((event.currentTarget as HTMLInputElement).value))}
                />
                <em>{activeChannel.unit ?? ""}/div</em>
              </div>
            </label>
            <label>
              <span>Offset</span>
              <div class="scope-unit-input">
                <input
                  type="number"
                  step="any"
                  value={verticalOffset.get(activeChannelId) ?? 0}
                  oninput={(event) => updateVerticalOffset(activeChannelId!, Number((event.currentTarget as HTMLInputElement).value))}
                />
                <em>{activeChannel.unit ?? ""}</em>
              </div>
            </label>
          </div>
        {/if}
      {:else}
        <div class="empty-hint">Select a channel to adjust its vertical scale.</div>
      {/if}
    </section>

    {#if cursorEnabled}
      <section class="side-section">
        <div class="section-heading">CURSOR</div>
        <div class="property-grid">
          <span>X1</span><strong>{cursorA === undefined ? "—" : formatSignedTime(cursorA)}</strong>
          <span>X2</span><strong>{cursorB === undefined ? "—" : formatSignedTime(cursorB)}</strong>
          <span>Δt</span><strong>{cursorA === undefined || cursorB === undefined ? "—" : formatTime(Math.abs(cursorB - cursorA))}</strong>
          <span>1/Δt</span><strong>{cursorA === undefined || cursorB === undefined || cursorA === cursorB ? "—" : `${(1 / Math.abs(cursorB - cursorA)).toFixed(2)} Hz`}</strong>
        </div>
        <div class="scope-pending">Click the waveform to place X1, then X2.</div>
      </section>
    {/if}

    <section class="side-section acquisition">
      <div class="section-heading">ACQUISITION</div>
      <div class="property-grid">
        <span>State</span><strong>{snapshot?.state ?? "STOPPED"}</strong>
        <span>History</span><strong>{scopeConfig ? `${scopeConfig.historySeconds.toFixed(3)} s` : "10.000 s"}</strong>
        <span>{rateLabel("fast")}</span><strong>{fastSelected} / {connection?.fastMaxChannels ?? "—"}</strong>
        <span>{rateLabel("normal")}</span><strong>{normalSelected} / {connection?.normalMaxChannels ?? "—"}</strong>
        <span>FAST Block</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
      </div>
    </section>

    <MotionCompactEditor capabilities={connection?.motion} {onError} />
  </aside>

  <section id="scope-workspace" class="scope-workspace">
    <div class="editor-tabs"><div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div></div>
    <div class="plot-header">
      {#if visibleChannels.length > 0}
        {#each visibleChannels as channel, index}
          <button class="trace-key trace-key-button" onclick={() => selectActiveChannel(channel.id)}>
            <span class="trace-mark" style={`background:${traceColor(channel)}`}></span>
            {channel.symbol}
            <span class="value">{latestValues[index] ?? "—"}</span>
          </button>
        {/each}
      {:else}
        <div class="plot-placeholder">{connection ? "Select channels and press Run." : "Connect a device before using Scope."}</div>
      {/if}
      <div class="plot-meta">{snapshot?.sampleCount ?? 0} samples · loss {snapshot?.lostFrames ?? 0}</div>
    </div>
    <div bind:this={plotHost} class="plot-host scope-plot-interactive" onclick={setCursorFromPlot}></div>
  </section>
</div>
