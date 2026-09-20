<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Dygraph from "dygraphs";
  import "dygraphs/dist/dygraph.css";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { subscribeRefresh } from "../../refreshScheduler";
  import { configureScope, readScopeSnapshot, startScope, stopScope } from "./api";
  import type { ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let active = false;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let host: HTMLDivElement;
  let graph: Dygraph | undefined;
  let refreshUnsubscribe: (() => void) | undefined;
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
    const defaults = channels.slice(0, 2);
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

  function rows(next: ScopeSnapshot): Array<Array<number | null>> {
    const activeChannels = channels.filter((channel) => selectedIds.has(channel.id));
    if (activeChannels.length === 0) return [];

    const timeSet = new Set<number>();
    for (const channel of activeChannels) {
      const series = next.series.find((item) => item.id === channel.id);
      for (const time of series?.times ?? []) timeSet.add(time);
    }
    const times = Array.from(timeSet).sort((a, b) => a - b);
    const valuesByChannel = new Map<number, Map<number, number>>();

    for (const channel of activeChannels) {
      const series = next.series.find((item) => item.id === channel.id);
      const values = new Map<number, number>();
      if (series) {
        for (let index = 0; index < series.times.length; index += 1) {
          values.set(series.times[index], series.values[index]);
        }
      }
      valuesByChannel.set(channel.id, values);
    }

    return times.map((time) => [
      time,
      ...activeChannels.map((channel) => valuesByChannel.get(channel.id)?.get(time) ?? null),
    ]);
  }

  function labels(): string[] {
    return [
      "Time",
      ...channels.filter((channel) => selectedIds.has(channel.id)).map((channel) => channel.label),
    ];
  }

  function updateGraph(next: ScopeSnapshot) {
    if (!graph) return;
    graph.updateOptions({
      file: rows(next),
      labels: labels(),
    });
  }

  async function refreshSnapshot() {
    if (!configured || busy) return;
    try {
      const next = await readScopeSnapshot(10, 0, 6000);
      snapshot = next;
      updateGraph(next);
    } catch (error) {
      onError(error);
    }
  }

  function dygraphInteractionModel() {
    return {
      ...Dygraph.defaultInteractionModel,
      mousewheel: (event: WheelEvent, g: Dygraph) => {
        const axis = g.xAxisRange();
        const rect = host.getBoundingClientRect();
        const xPct = Math.min(Math.max((event.clientX - rect.left) / Math.max(rect.width, 1), 0), 1);
        const factor = event.deltaY < 0 ? 0.8 : 1.25;
        const nextRange = (axis[1] - axis[0]) * factor;
        const anchor = axis[0] + (axis[1] - axis[0]) * xPct;
        const min = anchor - nextRange * xPct;
        const max = min + nextRange;
        g.updateOptions({ dateWindow: [min, max] });
        event.preventDefault();
      },
    };
  }

  onMount(() => {
    ensureDefaults();
    graph = new Dygraph(host, [[0]], {
      labels: ["Time"],
      legend: "always",
      animatedZooms: false,
      connectSeparatedPoints: false,
      drawPoints: false,
      strokeWidth: 1.2,
      panEdgeFraction: 0,
      interactionModel: dygraphInteractionModel(),
      axes: {
        x: {
          axisLabelFormatter: (value: number) => `${value.toFixed(3)} s`,
          valueFormatter: (value: number) => `${value.toFixed(6)} s`,
        },
      },
    });

    refreshUnsubscribe = subscribeRefresh(50, () => {
      if (active && running) void refreshSnapshot();
    });
  });

  onDestroy(() => {
    refreshUnsubscribe?.();
    graph?.destroy();
  });
</script>

<section class="page-toolbar">
  <div class="page-title">ANALYSIS / SCOPE · DYGRAPHS SPIKE</div>
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
      Dygraphs native interaction: drag to zoom, Shift/Alt + drag to pan, double-click to reset; wheel zoom uses the library's documented interaction-model hook.
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
  :global(.dygraph-axis-label),
  :global(.dygraph-legend) {
    color: #bdbdbd !important;
    background: transparent !important;
  }
</style>
