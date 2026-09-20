<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import * as echarts from "echarts";
  import type { ECharts, EChartsOption, SeriesOption } from "echarts";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { subscribeRefresh } from "../../refreshScheduler";
  import { configureScope, readScopeSnapshot, startScope, stopScope } from "./api";
  import type { ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let active = false;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let host: HTMLDivElement;
  let chart: ECharts | undefined;
  let refreshUnsubscribe: (() => void) | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let selectedIds = new Set<number>();
  let rates = new Map<number, ScopeRate>();
  let snapshot: ScopeSnapshot | undefined;
  let busy = false;
  let configured = false;

  $: channels = connection?.channels.filter((channel) => channel.supportsFast || channel.supportsNormal) ?? [];
  $: running = snapshot?.state === "LIVE";
  $: onSummary({
    state: snapshot?.state ?? "STOPPED",
    selectedChannels: selectedIds.size,
    lostFrames: snapshot?.lostFrames ?? 0,
  });

  function defaultRate(channel: PlotChannel): ScopeRate {
    return channel.supportsFast ? "fast" : "normal";
  }

  function ensureDefaults() {
    if (!connection || selectedIds.size > 0) return;
    const defaults = channels.slice(0, 3);
    selectedIds = new Set(defaults.map((channel) => channel.id));
    rates = new Map(defaults.map((channel) => [channel.id, defaultRate(channel)]));
  }

  function setSelected(id: number) {
    const next = new Set(selectedIds);
    const nextRates = new Map(rates);
    const channel = channels.find((item) => item.id === id);
    if (!channel) return;

    if (next.has(id)) {
      next.delete(id);
      nextRates.delete(id);
    } else {
      next.add(id);
      nextRates.set(id, defaultRate(channel));
    }

    selectedIds = next;
    rates = nextRates;
    configured = false;
    if (snapshot) updateChart(snapshot);
  }

  function setRate(id: number, rate: ScopeRate) {
    const channel = channels.find((item) => item.id === id);
    if (!channel) return;
    if (rate === "fast" && !channel.supportsFast) return;
    if (rate === "normal" && !channel.supportsNormal) return;
    rates = new Map(rates).set(id, rate);
    configured = false;
  }

  async function ensureConfigured() {
    if (!connection || selectedIds.size === 0) return false;
    if (configured) return true;
    await configureScope(
      Array.from(selectedIds).map((id) => ({ id, rate: rates.get(id) ?? "normal" })),
    );
    configured = true;
    return true;
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
      await refreshSnapshot();
    } catch (error) {
      onError(error);
    } finally {
      busy = false;
    }
  }

  function activeChannels() {
    return channels.filter((channel) => selectedIds.has(channel.id));
  }

  function makeOption(next: ScopeSnapshot): EChartsOption {
    const active = activeChannels();

    const yAxis = active.map((channel, index) => ({
      type: "value" as const,
      name: channel.unit ?? "",
      show: index < 2,
      position: index % 2 === 0 ? "left" as const : "right" as const,
      offset: Math.floor(index / 2) * 42,
      scale: true,
      axisLine: { show: index < 2 },
      axisTick: { show: index < 2 },
      axisLabel: { show: index < 2 },
      splitLine: { show: index === 0 },
    }));

    const series: SeriesOption[] = active.map((channel, index) => {
      const source = next.series.find((item) => item.id === channel.id);
      return {
        name: channel.label,
        type: "line",
        yAxisIndex: index,
        showSymbol: false,
        symbol: "none",
        sampling: "lttb",
        animation: false,
        data: (source?.times ?? []).map((time, pointIndex) => [time, source?.values[pointIndex] ?? null]),
      };
    });

    return {
      animation: false,
      backgroundColor: "transparent",
      grid: { left: 68, right: 36, top: 34, bottom: 54, containLabel: false },
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
        min: next.recordedSeconds > 0 ? -next.recordedSeconds : -1,
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
        },
      ],
      series,
    };
  }

  function updateChart(next: ScopeSnapshot) {
    chart?.setOption(makeOption(next), {
      notMerge: true,
      lazyUpdate: false,
    });
  }

  async function refreshSnapshot() {
    if (!configured || busy) return;
    try {
      const next = await readScopeSnapshot(10, 0, 6000);
      snapshot = next;
      updateChart(next);
    } catch (error) {
      onError(error);
    }
  }

  onMount(() => {
    ensureDefaults();
    chart = echarts.init(host, undefined, { renderer: "canvas" });
    chart.setOption({
      animation: false,
      xAxis: { type: "value" },
      yAxis: [{ type: "value" }],
      series: [],
    });
    resizeObserver = new ResizeObserver(() => chart?.resize());
    resizeObserver.observe(host);

    refreshUnsubscribe = subscribeRefresh(50, () => {
      if (active && running) void refreshSnapshot();
    });
  });

  onDestroy(() => {
    refreshUnsubscribe?.();
    resizeObserver?.disconnect();
    chart?.dispose();
  });
</script>

<section class="page-toolbar">
  <div class="page-title">ANALYSIS / SCOPE · ECHARTS SPIKE</div>
  <div class="toolbar-actions">
    <vscode-button disabled={!connection || selectedIds.size === 0 || busy} onclick={() => void toggleRun()}>
      {running ? "Stop" : "Run"}
    </vscode-button>
  </div>
</section>

<div class="scope-spike-shell">
  <aside class="scope-spike-sidebar">
    <div class="section-heading">CHANNELS</div>
    {#each channels as channel}
      <div class="scope-spike-channel">
        <input
          type="checkbox"
          checked={selectedIds.has(channel.id)}
          onchange={() => setSelected(channel.id)}
        />
        <span>{channel.label}</span>
        <span>{channel.unit ?? ""}</span>
        <select
          value={rates.get(channel.id) ?? defaultRate(channel)}
          onchange={(event) => setRate(channel.id, (event.currentTarget as HTMLSelectElement).value as ScopeRate)}
        >
          <option value="fast" disabled={!channel.supportsFast}>FAST</option>
          <option value="normal" disabled={!channel.supportsNormal}>NORMAL</option>
        </select>
      </div>
    {/each}
    <div class="scope-spike-note">
      ECharts native dataZoom + axisPointer. Each selected channel owns an independent yAxis.
    </div>
  </aside>

  <section class="scope-spike-workspace">
    <div class="scope-spike-meta">
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
  .scope-spike-channel {
    min-height: 31px;
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) 54px 76px;
    align-items: center;
    gap: 5px;
    padding: 0 8px;
    border-bottom: 1px solid #252525;
    font-size: 11px;
  }
  .scope-spike-channel span:nth-child(2) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope-spike-channel span:nth-child(3) {
    color: #858585;
    text-align: right;
  }
  .scope-spike-channel select {
    height: 22px;
    border: 1px solid #3a3a3a;
    color: #c8c8c8;
    background: #252526;
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
  .scope-spike-plot {
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    background: #1e1e1e;
  }
</style>
