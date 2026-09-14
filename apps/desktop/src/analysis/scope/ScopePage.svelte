<script lang="ts">
  import { onMount } from "svelte";
  import Split from "split.js";
  import uPlot from "uplot";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { clearScope, configureScope, pauseScope, readScopeSnapshot, startScope } from "./api";
  import type { ScopeConfig, ScopeSnapshot, ScopeSummary } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let plotHost: HTMLDivElement;
  let plot: uPlot | undefined;
  let split: Split.Instance | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let snapshotBusy = false;
  let scopeConfig: ScopeConfig | undefined;
  let selectedIds = new Set<number>();
  let snapshot: ScopeSnapshot | undefined;
  let activeConnection: ConnectionInfo | undefined;
  const traceColors = ["#7aa2c8", "#c8b77a", "#9b8ac8", "#7fa68a", "#c28b73", "#aa829a", "#79a6ad", "#91a77b"];

  $: if (connection !== activeConnection) {
    activeConnection = connection;
    resetScope();
    if (connection) selectedIds = new Set(connection.channels.filter((c) => c.supportsFast).slice(0, 2).map((c) => c.id));
  }
  $: onSummary({ state: snapshot?.state ?? "STOPPED", selectedChannels: selectedIds.size, lostFrames: snapshot?.lostFrames ?? 0 });

  function channelMode(channel: PlotChannel): string {
    if (channel.supportsFast && channel.supportsNormal) return "FAST+N";
    if (channel.supportsFast) return "FAST";
    return "NORMAL";
  }

  function resetScope() {
    scopeConfig = undefined;
    snapshot = undefined;
    selectedIds = new Set<number>();
    rebuildPlot([]);
  }

  function toggleChannel(id: number) {
    if (!connection) return;
    const next = new Set(selectedIds);
    if (next.has(id)) next.delete(id); else if (next.size < connection.fastMaxChannels) next.add(id);
    selectedIds = next;
    scopeConfig = undefined;
    snapshot = undefined;
  }

  async function ensureConfigured(): Promise<boolean> {
    if (scopeConfig) return true;
    if (!connection) { onError("Connect a device before starting Scope."); return false; }
    if (selectedIds.size === 0) { onError("Select at least one FAST channel."); return false; }
    try {
      scopeConfig = await configureScope(Array.from(selectedIds));
      rebuildPlot(scopeConfig.channels);
      return true;
    } catch (error) {
      onError(error);
      return false;
    }
  }

  async function run() { if (!(await ensureConfigured())) return; try { await startScope(); await refreshSnapshot(); } catch (error) { onError(error); } }
  async function pause() { if (!scopeConfig) return; try { await pauseScope(); await refreshSnapshot(); } catch (error) { onError(error); } }
  async function clear() { if (!scopeConfig) return; try { await clearScope(); await refreshSnapshot(); } catch (error) { onError(error); } }

  async function refreshSnapshot() {
    if (!scopeConfig || snapshotBusy) return;
    snapshotBusy = true;
    try { snapshot = await readScopeSnapshot(); plot?.setData([snapshot.times, ...snapshot.series] as uPlot.AlignedData); }
    catch (error) { onError(error); }
    finally { snapshotBusy = false; }
  }

  function rebuildPlot(channels: ScopeConfig["channels"]) {
    if (!plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot?.destroy();
    const series: uPlot.Series[] = [{}, ...channels.map((c, i) => ({ label: c.symbol, stroke: traceColors[i % traceColors.length], width: 1.25 }))];
    plot = new uPlot({
      width: Math.max(420, Math.floor(rect.width)),
      height: Math.max(260, Math.floor(rect.height)),
      legend: { show: false },
      cursor: { drag: { x: true, y: false } },
      scales: { x: { time: false } },
      axes: [
        { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
        { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
      ],
      series,
    }, [[], ...channels.map(() => [])] as uPlot.AlignedData, plotHost);
  }

  function resizePlot() {
    if (!plot || !plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot.setSize({ width: Math.max(420, Math.floor(rect.width)), height: Math.max(260, Math.floor(rect.height)) });
  }

  function latestValue(index: number): string {
    if (!snapshot || !scopeConfig || snapshot.series[index]?.length === 0) return "—";
    const values = snapshot.series[index];
    const value = values[values.length - 1];
    const unit = scopeConfig.channels[index]?.unit ?? "";
    return `${value.toFixed(3)}${unit ? ` ${unit}` : ""}`;
  }

  onMount(() => {
    split = Split(["#scope-sidebar", "#scope-workspace"], { sizes: [25, 75], minSize: [260, 420], gutterSize: 4, snapOffset: 0, onDrag: resizePlot });
    rebuildPlot([]);
    resizeObserver = new ResizeObserver(resizePlot);
    resizeObserver.observe(plotHost);
    refreshTimer = setInterval(refreshSnapshot, 50);
    return () => { if (refreshTimer) clearInterval(refreshTimer); resizeObserver?.disconnect(); plot?.destroy(); split?.destroy(); };
  });
</script>

<section class="page-toolbar"><div class="page-title">ANALYSIS / SCOPE</div><div class="toolbar-actions">
  <vscode-button disabled={!connection || selectedIds.size === 0} onclick={run}><i class="codicon codicon-play"></i>&nbsp;Run</vscode-button>
  <vscode-button secondary disabled={!scopeConfig} onclick={pause}><i class="codicon codicon-debug-pause"></i>&nbsp;Pause</vscode-button>
  <vscode-button secondary disabled={!scopeConfig} onclick={clear}>Clear</vscode-button>
</div></section>
<div class="scope-shell">
  <aside id="scope-sidebar" class="scope-sidebar">
    <section class="side-section"><div class="section-heading">CHANNELS</div><div class="channel-list">
      {#if connection}{#each connection.channels as channel}
        <label class:disabled-row={!channel.supportsFast} class="channel-row" onclick={() => channel.supportsFast && toggleChannel(channel.id)}>
          <vscode-checkbox checked={selectedIds.has(channel.id) || undefined} disabled={!channel.supportsFast}></vscode-checkbox>
          <span class="channel-name">{channel.symbol}</span><span class="channel-unit">{channel.unit ?? ""}</span><span class:normal-only={!channel.supportsFast} class="channel-mode">{channelMode(channel)}</span>
        </label>
      {/each}{:else}<div class="empty-hint">No device connected. Open Connection first to discover acquisition channels.</div>{/if}
    </div></section>
    <section class="side-section acquisition"><div class="section-heading">ACQUISITION</div><div class="property-grid">
      <span>State</span><strong>{snapshot?.state ?? "STOPPED"}</strong><span>FAST Rate</span><strong>{connection ? `${(connection.fastRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>History</span><strong>{scopeConfig ? `${scopeConfig.historySeconds.toFixed(3)} s` : "10.000 s"}</strong><span>Channels</span><strong>{selectedIds.size} / {connection?.fastMaxChannels ?? "—"}</strong><span>Block</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
    </div></section>
  </aside>
  <section id="scope-workspace" class="scope-workspace">
    <div class="editor-tabs"><div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div></div>
    <div class="plot-header">{#if scopeConfig}{#each scopeConfig.channels as channel, index}<div class="trace-key"><span class="trace-mark" style={`background:${traceColors[index % traceColors.length]}`}></span>{channel.symbol}<span class="value">{latestValue(index)}</span></div>{/each}{:else}<div class="plot-placeholder">{connection ? "Select FAST channels and press Run." : "Connect a device before using Scope."}</div>{/if}<div class="plot-meta">{snapshot?.sampleCount ?? 0} samples · loss {snapshot?.lostFrames ?? 0}</div></div>
    <div bind:this={plotHost} class="plot-host"></div>
  </section>
</div>
