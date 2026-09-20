<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { LineChart } from "echarts/charts";
  import {
    DataZoomComponent,
    GridComponent,
    LegendComponent,
    TooltipComponent,
  } from "echarts/components";
  import { init, use, type ECharts, type EChartsOption } from "echarts/core";
  import { CanvasRenderer } from "echarts/renderers";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { subscribeRefresh } from "../../refreshScheduler";
  import { configureScope, readScopeSnapshot, startScope, stopScope } from "./api";
  import type { ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let active = false;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  use([LineChart, DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

  const refreshIntervalMs = 100;
  const snapshotWindowSeconds = 0.5;
  const maxGraphPoints = 2500;
  const verticalDivisions = 8;
  const traceColors = [
    "#5470c6", "#b6d72c", "#586080", "#ff9845", "#73c0de", "#3ba272",
    "#fc8452", "#9a60b4", "#ea7ccc", "#91cc75", "#fac858", "#ee6666",
  ];

  let host: HTMLDivElement;
  let chart: ECharts | undefined;
  let refreshUnsubscribe: (() => void) | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let reconfigureTimer: ReturnType<typeof setTimeout> | undefined;
  let viewRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  let selectedIds = new Set<number>();
  let rates = new Map<number, ScopeRate>();
  let verticalScale = new Map<number, number>();
  let verticalOffset = new Map<number, number>();
  let activeChannelId: number | undefined;
  let snapshot: ScopeSnapshot | undefined;
  let busy = false;
  let snapshotBusy = false;
  let configured = false;
  let configurationDirty = false;
  let followingLatest = true;
  let viewRange: [number, number] = [-snapshotWindowSeconds, 0];
  let viewRevision = 0;
  let applyingChartOption = false;
  let activeConnection: ConnectionInfo | undefined;

  $: channels = connection?.channels.filter((channel) => channel.supportsFast || channel.supportsNormal) ?? [];
  $: running = snapshot?.state === "LIVE";
  $: if (connection !== activeConnection) {
    activeConnection = connection;
    resetScope();
  }
  $: if (active && host) {
    void tick().then(ensureChart);
  }
  $: onSummary({
    state: snapshot?.state ?? "STOPPED",
    selectedChannels: selectedIds.size,
    lostFrames: snapshot?.lostFrames ?? 0,
  });

  function defaultRate(channel: PlotChannel): ScopeRate {
    return channel.supportsFast ? "fast" : "normal";
  }

  function defaultVerticalScale(channel: PlotChannel): number {
    const unit = channel.unit?.trim().toLowerCase() ?? "";
    if (unit === "a") return 0.5;
    if (unit === "v") return 5;
    if (unit.includes("rad/s")) return 10;
    if (unit.includes("turn")) return 0.5;
    return 1;
  }

  function traceColor(channel: PlotChannel): string {
    const index = Math.max(0, channels.findIndex((item) => item.id === channel.id));
    return traceColors[index % traceColors.length];
  }

  function ceil125(value: number): number {
    if (!Number.isFinite(value) || value <= 0) return 1;
    const exponent = Math.floor(Math.log10(value));
    const decade = 10 ** exponent;
    for (const step of [1, 2, 5, 10]) {
      const candidate = step * decade;
      if (candidate >= value * (1 - 1e-12)) return candidate;
    }
    return 10 * decade;
  }

  function resetScope() {
    if (reconfigureTimer) clearTimeout(reconfigureTimer);
    if (viewRefreshTimer) clearTimeout(viewRefreshTimer);
    reconfigureTimer = undefined;
    viewRefreshTimer = undefined;
    configured = false;
    configurationDirty = false;
    snapshot = undefined;
    followingLatest = true;
    viewRange = [-snapshotWindowSeconds, 0];
    viewRevision += 1;
    const availableChannels = connection?.channels.filter(
      (channel) => channel.supportsFast || channel.supportsNormal,
    ) ?? [];
    const defaults = availableChannels.slice(0, 3);
    selectedIds = new Set(defaults.map((channel) => channel.id));
    rates = new Map(defaults.map((channel) => [channel.id, defaultRate(channel)]));
    verticalScale = new Map(availableChannels.map((channel) => [channel.id, defaultVerticalScale(channel)]));
    verticalOffset = new Map(availableChannels.map((channel) => [channel.id, 0]));
    activeChannelId = defaults[0]?.id;
    chart?.clear();
    setEmptyChartOption();
  }

  function setSelected(id: number) {
    if (busy) return;
    const next = new Set(selectedIds);
    const nextRates = new Map(rates);
    const channel = channels.find((item) => item.id === id);
    if (!channel) return;

    if (next.has(id)) {
      if (running && next.size === 1) {
        onError("Keep at least one channel selected while Scope is running.");
        return;
      }
      next.delete(id);
      nextRates.delete(id);
      if (activeChannelId === id) {
        activeChannelId = channels.find((item) => next.has(item.id))?.id;
      }
    } else {
      next.add(id);
      nextRates.set(id, defaultRate(channel));
      activeChannelId = id;
    }

    selectedIds = next;
    rates = nextRates;
    configurationDirty = true;
    if (snapshot) updateChart(snapshot, displayRange(snapshot.recordedSeconds));
    scheduleHotReconfigure();
  }

  function setRate(id: number, rate: ScopeRate) {
    if (busy) return;
    const channel = channels.find((item) => item.id === id);
    if (!channel) return;
    if (rate === "fast" && !channel.supportsFast) return;
    if (rate === "normal" && !channel.supportsNormal) return;
    rates = new Map(rates).set(id, rate);
    configurationDirty = true;
    scheduleHotReconfigure();
  }

  async function ensureConfigured() {
    if (!connection || selectedIds.size === 0) return false;
    if (configured && !configurationDirty) return true;
    await configureScope(
      Array.from(selectedIds).map((id) => ({ id, rate: rates.get(id) ?? "normal" })),
    );
    configured = true;
    configurationDirty = false;
    return true;
  }

  function scheduleHotReconfigure() {
    if (!running || selectedIds.size === 0) return;
    if (reconfigureTimer) clearTimeout(reconfigureTimer);
    reconfigureTimer = setTimeout(() => {
      reconfigureTimer = undefined;
      void hotReconfigure();
    }, 80);
  }

  async function hotReconfigure() {
    if (!running || busy || !configurationDirty || selectedIds.size === 0) return;
    busy = true;
    try {
      await ensureConfigured();
    } catch (error) {
      onError(error);
    } finally {
      busy = false;
      if (configured) scheduleViewRefresh(0);
    }
  }

  async function toggleRun() {
    if (busy) return;
    busy = true;
    try {
      if (running) {
        await stopScope();
      } else {
        if (!(await ensureConfigured())) return;
        await startScope();
      }
      await refreshSnapshot(true);
    } catch (error) {
      onError(error);
    } finally {
      busy = false;
    }
  }

  function activeChannels() {
    return channels.filter((channel) => selectedIds.has(channel.id));
  }

  function selectActiveChannel(id: number) {
    if (!selectedIds.has(id) || activeChannelId === id) return;
    activeChannelId = id;
    if (snapshot) updateChart(snapshot, displayRange(snapshot.recordedSeconds));
  }

  function updateVerticalScale(id: number, value: number) {
    if (!Number.isFinite(value) || value <= 0) return;
    verticalScale = new Map(verticalScale).set(id, value);
    if (snapshot) updateChart(snapshot, displayRange(snapshot.recordedSeconds));
  }

  function updateVerticalOffset(id: number, value: number) {
    if (!Number.isFinite(value)) return;
    verticalOffset = new Map(verticalOffset).set(id, value);
    if (snapshot) updateChart(snapshot, displayRange(snapshot.recordedSeconds));
  }

  function autoVertical(id: number) {
    const channel = channels.find((item) => item.id === id);
    const values = snapshot?.series.find((series) => series.id === id)?.values ?? [];
    if (!channel || values.length === 0) return;

    let min = Infinity;
    let max = -Infinity;
    for (const value of values) {
      if (!Number.isFinite(value)) continue;
      min = Math.min(min, value);
      max = Math.max(max, value);
    }
    if (!Number.isFinite(min) || !Number.isFinite(max)) return;

    const center = (min + max) / 2;
    const span = Math.max(max - min, Math.abs(center) * 0.01, 1e-6);
    verticalOffset = new Map(verticalOffset).set(id, center);
    verticalScale = new Map(verticalScale).set(id, ceil125(span / (verticalDivisions * 0.75)));
    if (snapshot) updateChart(snapshot, displayRange(snapshot.recordedSeconds));
  }

  function availableSeconds(recordedSeconds = snapshot?.recordedSeconds ?? snapshotWindowSeconds): number {
    return Math.max(recordedSeconds, 0.0005);
  }

  function latestRange(recordedSeconds = snapshot?.recordedSeconds ?? snapshotWindowSeconds): [number, number] {
    return [-Math.min(snapshotWindowSeconds, availableSeconds(recordedSeconds)), 0];
  }

  function clampViewRange(
    range: [number, number],
    recordedSeconds = snapshot?.recordedSeconds ?? snapshotWindowSeconds,
  ): [number, number] {
    const available = availableSeconds(recordedSeconds);
    const width = Math.min(Math.max(range[1] - range[0], 0.0005), available);
    let max = Math.min(range[1], 0);
    let min = max - width;
    if (min < -available) {
      min = -available;
      max = min + width;
    }
    return [min, max];
  }

  function displayRange(recordedSeconds = snapshot?.recordedSeconds ?? snapshotWindowSeconds): [number, number] {
    return followingLatest ? latestRange(recordedSeconds) : clampViewRange(viewRange, recordedSeconds);
  }

  function makeOption(next: ScopeSnapshot, visibleRange: [number, number]): EChartsOption {
    const active = activeChannels();
    const visibleAxisChannel = active.find((channel) => channel.id === activeChannelId) ?? active[0];

    const channelAxes = active.map((channel) => {
      const perDiv = verticalScale.get(channel.id) ?? defaultVerticalScale(channel);
      const center = verticalOffset.get(channel.id) ?? 0;
      const half = perDiv * verticalDivisions / 2;
      const visible = channel.id === visibleAxisChannel?.id;
      const color = traceColor(channel);
      return {
        type: "value" as const,
        name: visible ? `${channel.label}${channel.unit ? ` (${channel.unit})` : ""}` : "",
        show: visible,
        position: "left" as const,
        min: center - half,
        max: center + half,
        interval: perDiv,
        axisLine: { show: visible, lineStyle: { color } },
        axisTick: { show: visible },
        axisLabel: { show: visible, color },
        nameTextStyle: { color },
        splitLine: { show: visible, lineStyle: { color: "#303030" } },
      };
    });
    const yAxis = channelAxes.length > 0 ? channelAxes : [{ type: "value" as const }];

    const series = active.map((channel, index) => {
      const source = next.series.find((item) => item.id === channel.id);
      const color = traceColor(channel);
      return {
        name: channel.label,
        type: "line" as const,
        yAxisIndex: index,
        showSymbol: false,
        symbol: "none",
        sampling: "lttb",
        animation: false,
        lineStyle: { color, width: channel.id === visibleAxisChannel?.id ? 1.5 : 1.1 },
        itemStyle: { color },
        data: (source?.times ?? []).map((time, pointIndex) => [time, source?.values[pointIndex] ?? null]),
      };
    });

    return {
      animation: false,
      backgroundColor: "transparent",
      grid: { left: 76, right: 36, top: 42, bottom: 54, containLabel: false },
      legend: {
        top: 4,
        right: 14,
        textStyle: { color: "#bdbdbd" },
      },
      tooltip: {
        trigger: "axis",
        axisPointer: { type: "cross" },
        backgroundColor: "rgba(30,30,30,0.94)",
        borderColor: "#4a4a4a",
        textStyle: { color: "#d0d0d0" },
      },
      xAxis: {
        type: "value",
        min: -availableSeconds(next.recordedSeconds),
        max: 0,
        axisLine: { lineStyle: { color: "#6e6e6e" } },
        axisLabel: {
          color: "#9a9a9a",
          formatter: (value: number) => `${value.toFixed(3)} s`,
        },
        splitLine: { lineStyle: { color: "#303030" } },
      },
      yAxis,
      dataZoom: [
        {
          type: "inside",
          xAxisIndex: 0,
          zoomOnMouseWheel: true,
          moveOnMouseMove: true,
          moveOnMouseWheel: false,
          preventDefaultMouseMove: true,
          filterMode: "none",
          startValue: visibleRange[0],
          endValue: visibleRange[1],
        },
      ],
      series,
    };
  }

  function setEmptyChartOption() {
    if (!chart) return;
    applyingChartOption = true;
    try {
      chart.setOption({
        animation: false,
        xAxis: { type: "value", min: -snapshotWindowSeconds, max: 0 },
        yAxis: [{ type: "value" }],
        dataZoom: [{ type: "inside", xAxisIndex: 0, startValue: -snapshotWindowSeconds, endValue: 0 }],
        series: [],
      }, { notMerge: true, lazyUpdate: false });
    } finally {
      applyingChartOption = false;
    }
  }

  function updateChart(next: ScopeSnapshot, visibleRange = displayRange(next.recordedSeconds)) {
    if (!chart) return;
    applyingChartOption = true;
    try {
      chart.setOption(makeOption(next, visibleRange), {
        notMerge: false,
        replaceMerge: ["series", "yAxis"],
        lazyUpdate: false,
      });
    } finally {
      applyingChartOption = false;
    }
  }

  async function refreshSnapshot(allowWhileBusy = false) {
    if (!configured || snapshotBusy || (busy && !allowWhileBusy)) return;
    snapshotBusy = true;
    const revision = viewRevision;
    const requestedFollowingLatest = followingLatest;
    const requestedRange = requestedFollowingLatest ? latestRange() : clampViewRange(viewRange);
    const windowSeconds = requestedFollowingLatest
      ? snapshotWindowSeconds
      : requestedRange[1] - requestedRange[0];
    const endOffsetSeconds = requestedFollowingLatest ? 0 : Math.max(0, -requestedRange[1]);
    try {
      const next = await readScopeSnapshot(windowSeconds, endOffsetSeconds, maxGraphPoints);
      if (revision !== viewRevision || requestedFollowingLatest !== followingLatest) return;
      snapshot = next;
      const nextRange = requestedFollowingLatest
        ? latestRange(next.recordedSeconds)
        : clampViewRange(requestedRange, next.recordedSeconds);
      viewRange = nextRange;
      updateChart(next, nextRange);
    } catch (error) {
      onError(error);
    } finally {
      snapshotBusy = false;
    }
  }

  function scheduleViewRefresh(delay = 80) {
    if (viewRefreshTimer) clearTimeout(viewRefreshTimer);
    viewRefreshTimer = setTimeout(() => {
      viewRefreshTimer = undefined;
      if (snapshotBusy || busy) {
        scheduleViewRefresh(50);
        return;
      }
      void refreshSnapshot();
    }, delay);
  }

  function navigateToRange(range: [number, number]) {
    followingLatest = false;
    viewRange = clampViewRange(range);
    viewRevision += 1;
    scheduleViewRefresh();
  }

  function returnToLatest() {
    followingLatest = true;
    viewRange = latestRange();
    viewRevision += 1;
    if (snapshot) updateChart(snapshot, viewRange);
    scheduleViewRefresh(0);
  }

  function dataZoomRange(event: any): [number, number] | undefined {
    const payload = event?.batch?.[0] ?? event ?? {};
    const option = chart?.getOption() as any;
    const zoom = option?.dataZoom?.[0] ?? {};
    const min = -availableSeconds();
    const width = -min;

    const payloadStartValue = Number(payload.startValue);
    const payloadEndValue = Number(payload.endValue);
    if (Number.isFinite(payloadStartValue) && Number.isFinite(payloadEndValue)) {
      return [Math.min(payloadStartValue, payloadEndValue), Math.max(payloadStartValue, payloadEndValue)];
    }

    const payloadStart = Number(payload.start);
    const payloadEnd = Number(payload.end);
    if (Number.isFinite(payloadStart) && Number.isFinite(payloadEnd)) {
      return [min + width * payloadStart / 100, min + width * payloadEnd / 100];
    }

    const startValue = Number(zoom.startValue);
    const endValue = Number(zoom.endValue);
    if (Number.isFinite(startValue) && Number.isFinite(endValue)) {
      return [Math.min(startValue, endValue), Math.max(startValue, endValue)];
    }

    const start = Number(zoom.start);
    const end = Number(zoom.end);
    return Number.isFinite(start) && Number.isFinite(end)
      ? [min + width * start / 100, min + width * end / 100]
      : undefined;
  }

  function handleDataZoom(event: any) {
    if (applyingChartOption) return;
    const range = dataZoomRange(event);
    if (range) navigateToRange(range);
  }

  function ensureChart() {
    if (!active || !host) return;
    const rect = host.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return;
    if (!chart) {
      chart = init(host, undefined, { renderer: "canvas" });
      chart.on("datazoom", handleDataZoom);
      chart.getZr().on("dblclick", returnToLatest);
      setEmptyChartOption();
      if (snapshot) updateChart(snapshot, displayRange(snapshot.recordedSeconds));
    } else {
      chart.resize();
    }
  }

  onMount(() => {
    resizeObserver = new ResizeObserver(ensureChart);
    resizeObserver.observe(host);
    ensureChart();

    refreshUnsubscribe = subscribeRefresh(refreshIntervalMs, () => {
      if (active && running && followingLatest) void refreshSnapshot();
    });
  });

  onDestroy(() => {
    refreshUnsubscribe?.();
    if (reconfigureTimer) clearTimeout(reconfigureTimer);
    if (viewRefreshTimer) clearTimeout(viewRefreshTimer);
    resizeObserver?.disconnect();
    chart?.dispose();
  });
</script>

<section class="page-toolbar">
  <div class="page-title">ANALYSIS / SCOPE · ECHARTS SPIKE</div>
  <div class="toolbar-actions">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <vscode-button disabled={!connection || (!running && selectedIds.size === 0) || busy} onclick={() => void toggleRun()}>
      {running ? "Stop" : "Run"}
    </vscode-button>
  </div>
</section>

<div class="scope-spike-shell">
  <aside class="scope-spike-sidebar">
    <div class="section-heading">CHANNELS</div>
    {#each channels as channel}
      <div class:active-channel={activeChannelId === channel.id} class="channel-block">
        <div class="scope-spike-channel">
          <input
            type="checkbox"
            checked={selectedIds.has(channel.id)}
            disabled={busy}
            onchange={() => setSelected(channel.id)}
          />
          <button
            class="channel-name"
            class:inactive={!selectedIds.has(channel.id)}
            onclick={() => selectActiveChannel(channel.id)}
          >
            <span class="channel-color" style={`background:${traceColor(channel)}`}></span>
            <span>{channel.label}</span>
          </button>
          <span class="channel-unit">{channel.unit ?? ""}</span>
          <select
            value={rates.get(channel.id) ?? defaultRate(channel)}
            disabled={busy}
            onchange={(event) => setRate(channel.id, (event.currentTarget as HTMLSelectElement).value as ScopeRate)}
          >
            <option value="fast" disabled={!channel.supportsFast}>FAST</option>
            <option value="normal" disabled={!channel.supportsNormal}>NORMAL</option>
          </select>
        </div>
        {#if selectedIds.has(channel.id) && activeChannelId === channel.id}
          <div class="channel-y-controls">
            <label>
              <span>Scale/div</span>
              <input
                type="number"
                min="0.000001"
                step="any"
                value={verticalScale.get(channel.id) ?? defaultVerticalScale(channel)}
                onchange={(event) => updateVerticalScale(channel.id, Number((event.currentTarget as HTMLInputElement).value))}
              />
              <em>{channel.unit ?? ""}/div</em>
            </label>
            <label>
              <span>Y Pos</span>
              <input
                type="number"
                step="any"
                value={verticalOffset.get(channel.id) ?? 0}
                oninput={(event) => updateVerticalOffset(channel.id, Number((event.currentTarget as HTMLInputElement).value))}
              />
              <em>{channel.unit ?? ""}</em>
            </label>
            <button class="auto-button" onclick={() => autoVertical(channel.id)}>Auto</button>
          </div>
        {/if}
      </div>
    {/each}
    <div class="scope-spike-note">
      Click a channel name to make its Y axis active. Scale/div and Y Pos control each trace independently.
      Wheel or drag inspects history; double-click returns to the latest 0.5 s window.
    </div>
  </aside>

  <section class="scope-spike-workspace">
    <div class="scope-spike-meta">
      <span>{followingLatest ? "latest 0.500 s" : `history ${(viewRange[1] - viewRange[0]).toFixed(3)} s`}</span>
      {#if !followingLatest}<button class="latest-button" onclick={returnToLatest}>Latest</button>{/if}
      <span>{snapshot?.recordedSeconds?.toFixed(3) ?? "0.000"} s recorded</span>
      <span>loss {snapshot?.lostFrames ?? 0}</span>
    </div>
    <div bind:this={host} class="scope-spike-plot"></div>
  </section>
</div>

<style>
  .scope-spike-shell {
    min-height: 0;
    display: grid;
    grid-template-columns: 300px minmax(0, 1fr);
  }
  .scope-spike-sidebar {
    min-height: 0;
    overflow: auto;
    background: var(--vscode-sideBar-background);
    border-right: 1px solid var(--vscode-panel-border);
  }
  .channel-block {
    border-left: 2px solid transparent;
    border-bottom: 1px solid #252525;
  }
  .channel-block.active-channel {
    border-left-color: var(--vscode-focusBorder);
    background: rgba(255, 255, 255, 0.025);
  }
  .scope-spike-channel {
    min-height: 31px;
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) 54px 76px;
    align-items: center;
    gap: 5px;
    padding: 0 8px 0 6px;
    font-size: 11px;
  }
  .channel-name {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0;
    border: 0;
    color: #d0d0d0;
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .channel-name.inactive {
    color: #9a9a9a;
  }
  .channel-name span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .channel-color {
    width: 8px;
    height: 8px;
    flex: 0 0 auto;
    border-radius: 50%;
  }
  .channel-unit {
    color: #858585;
    text-align: right;
  }
  .scope-spike-channel select {
    height: 22px;
    border: 1px solid #3a3a3a;
    color: #c8c8c8;
    background: #252526;
  }
  .channel-y-controls {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 42px;
    gap: 6px;
    padding: 4px 8px 8px 30px;
  }
  .channel-y-controls label {
    min-width: 0;
    display: grid;
    grid-template-columns: 1fr;
    gap: 2px;
    color: #858585;
    font-size: 9.5px;
  }
  .channel-y-controls input {
    min-width: 0;
    width: 100%;
    box-sizing: border-box;
    height: 22px;
    padding: 0 4px;
    border: 1px solid #3a3a3a;
    color: #c8c8c8;
    background: #252526;
    font: inherit;
  }
  .channel-y-controls em {
    overflow: hidden;
    color: #6f6f6f;
    font-size: 9px;
    font-style: normal;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .auto-button {
    align-self: center;
    height: 22px;
    margin-top: 7px;
    padding: 0 5px;
    border: 1px solid #3a3a3a;
    color: #c8c8c8;
    background: #252526;
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }
  .auto-button:hover {
    background: #303030;
  }
  .scope-spike-note {
    padding: 10px;
    color: #858585;
    font-size: 10.5px;
    line-height: 1.5;
  }
  .scope-spike-workspace {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: 32px minmax(0, 1fr);
  }
  .scope-spike-meta {
    display: flex;
    justify-content: flex-end;
    gap: 16px;
    align-items: center;
    padding: 0 10px;
    border-bottom: 1px solid var(--vscode-panel-border);
    color: #858585;
    font-size: 11px;
  }
  .latest-button {
    height: 22px;
    padding: 0 8px;
    border: 1px solid #3a3a3a;
    border-radius: 2px;
    color: #c8c8c8;
    background: #252526;
    font: inherit;
    cursor: pointer;
  }
  .latest-button:hover {
    background: #303030;
  }
  .scope-spike-plot {
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    background: #1e1e1e;
  }
</style>
