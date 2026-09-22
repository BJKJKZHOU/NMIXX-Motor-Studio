<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import ScopeEchartsView from "./ScopeEchartsView.svelte";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { subscribeRefresh } from "../../refreshScheduler";
  import { configureScope, readScopeSnapshot, startScope, stopScope } from "./api";
  import type { ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let active = false;
  export let onSummary: (summary: ScopeSummary) => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;
  export let onClearError: () => void = () => undefined;


  const refreshIntervalMs = 100;
  const defaultLatestSpanSeconds = 0.5;
  const maxGraphPoints = 2500;
  const verticalDivisions = 8;
  const traceColors = [
    "#5470c6", "#b6d72c", "#586080", "#ff9845", "#73c0de", "#3ba272",
    "#fc8452", "#9a60b4", "#ea7ccc", "#91cc75", "#fac858", "#ee6666",
  ];

  let refreshUnsubscribe: (() => void) | undefined;
  let reconfigureTimer: ReturnType<typeof setTimeout> | undefined;
  let viewRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  let selectedIds = new Set<number>();
  let rates = new Map<number, ScopeRate>();
  let verticalScale = new Map<number, number>();
  let verticalOffset = new Map<number, number>();
  let activeChannelId: number | undefined;
  let snapshot: ScopeSnapshot | undefined;
  let busy = false;
  let stopping = false;
  let snapshotBusy = false;
  let configured = false;
  let configurationDirty = false;
  let followingLatest = true;
  let viewRange: [number, number] = [-defaultLatestSpanSeconds, 0];
  let viewRevision = 0;
  let activeConnection: ConnectionInfo | undefined;
  let channelNotice = "";

  $: channels = connection?.channels.filter((channel) => channel.supportsFast || channel.supportsNormal) ?? [];
  $: running = snapshot?.state === "LIVE";
  $: if (connection !== activeConnection) {
    activeConnection = connection;
    resetScope();
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
    channelNotice = "";
    snapshot = undefined;
    followingLatest = true;
    viewRange = [-defaultLatestSpanSeconds, 0];
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
  }

  function selectedRateCount(ids: Set<number>, nextRates: Map<number, ScopeRate>, rate: ScopeRate): number {
    return Array.from(ids).filter((id) => nextRates.get(id) === rate).length;
  }

  function rateLimit(rate: ScopeRate): number {
    if (!connection) return 0;
    return rate === "fast" ? connection.fastMaxChannels : connection.normalMaxChannels;
  }

  function validateChannelSelection(ids: Set<number>, nextRates: Map<number, ScopeRate>): string {
    for (const rate of ["fast", "normal"] as const) {
      const count = selectedRateCount(ids, nextRates, rate);
      const limit = rateLimit(rate);
      if (count > limit) {
        return `Scope supports at most ${limit} ${rate.toUpperCase()} channels.`;
      }
    }
    return "";
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

    const validation = validateChannelSelection(next, nextRates);
    if (validation) {
      channelNotice = validation;
      return;
    }

    channelNotice = "";
    onClearError();
    selectedIds = next;
    rates = nextRates;
    configurationDirty = true;
    scheduleHotReconfigure();
  }

  function setRate(id: number, rate: ScopeRate) {
    if (busy) return;
    const channel = channels.find((item) => item.id === id);
    if (!channel) return;
    if (rate === "fast" && !channel.supportsFast) return;
    if (rate === "normal" && !channel.supportsNormal) return;
    const nextRates = new Map(rates).set(id, rate);
    const validation = validateChannelSelection(selectedIds, nextRates);
    if (validation) {
      channelNotice = validation;
      return;
    }

    channelNotice = "";
    onClearError();
    rates = nextRates;
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
    channelNotice = "";
    onClearError();
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
    stopping = running;
    try {
      if (running) {
        await stopScope();
        await refreshSnapshot(true);
      } else {
        if (!(await ensureConfigured())) return;
        await startScope();
        followingLatest = true;
        viewRange = defaultRunRange();
        viewRevision += 1;
        await refreshSnapshot(true);
      }
    } catch (error) {
      onError(error);
    } finally {
      stopping = false;
      busy = false;
    }
  }

  function activeChannels() {
    return channels.filter((channel) => selectedIds.has(channel.id));
  }

  function selectActiveChannel(id: number) {
    if (!selectedIds.has(id) || activeChannelId === id) return;
    activeChannelId = id;
  }

  function updateVerticalScale(id: number, value: number) {
    if (!Number.isFinite(value) || value <= 0) return;
    verticalScale = new Map(verticalScale).set(id, value);
  }

  function updateVerticalOffset(id: number, value: number) {
    if (!Number.isFinite(value)) return;
    verticalOffset = new Map(verticalOffset).set(id, value);
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
  }

  function availableSeconds(recordedSeconds = snapshot?.recordedSeconds ?? defaultLatestSpanSeconds): number {
    return Math.max(recordedSeconds, 0.0005);
  }

  function viewSpan(range: [number, number] = viewRange): number {
    return Math.max(range[1] - range[0], 0.0005);
  }

  function latestRange(
    recordedSeconds = snapshot?.recordedSeconds ?? defaultLatestSpanSeconds,
    span = viewSpan(),
  ): [number, number] {
    return [-Math.min(span, availableSeconds(recordedSeconds)), 0];
  }

  function defaultRunRange(recordedSeconds = snapshot?.recordedSeconds ?? defaultLatestSpanSeconds): [number, number] {
    return [-Math.min(defaultLatestSpanSeconds, availableSeconds(recordedSeconds)), 0];
  }

  function clampViewRange(
    range: [number, number],
    recordedSeconds = snapshot?.recordedSeconds ?? defaultLatestSpanSeconds,
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

  function displayRange(recordedSeconds = snapshot?.recordedSeconds ?? defaultLatestSpanSeconds): [number, number] {
    return followingLatest
      ? latestRange(recordedSeconds, viewSpan(viewRange))
      : clampViewRange(viewRange, recordedSeconds);
  }

  onMount(() => {
    refreshUnsubscribe = subscribeRefresh(refreshIntervalMs, () => {
      if (active && running && followingLatest) void refreshSnapshot();
    });
  });

  onDestroy(() => {
    refreshUnsubscribe?.();
    if (reconfigureTimer) clearTimeout(reconfigureTimer);
    if (viewRefreshTimer) clearTimeout(viewRefreshTimer);
  });
</script>

<section class="page-toolbar">
  <div class="page-title">ANALYSIS / SCOPE · ECHARTS SPIKE</div>
  <div class="toolbar-actions">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <vscode-button
      class:stopping={stopping}
      disabled={!connection || (!running && selectedIds.size === 0) || busy}
      onclick={() => void toggleRun()}
    >
      <i class={`codicon ${stopping ? "codicon-loading codicon-modifier-spin" : running ? "codicon-debug-stop" : "codicon-play"}`}></i>
      {stopping ? "Stopping…" : running ? "Stop" : "Run"}
    </vscode-button>
  </div>
</section>

<div class="scope-spike-shell">
  <aside class="scope-spike-sidebar">
    <div class="section-heading">CHANNELS</div>
    {#if channelNotice}<div class="scope-channel-notice">{channelNotice}</div>{/if}
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
      Horizontal zoom and pan are continuous ECharts dataZoom interactions; double-click returns the current span to Latest.
    </div>
  </aside>

  <section class="scope-spike-workspace">
    <div class="scope-spike-meta">
      <span>{followingLatest ? `latest ${viewSpan(viewRange).toFixed(3)} s` : `history ${viewSpan(viewRange).toFixed(3)} s`}</span>
      {#if !followingLatest}<button class="latest-button" onclick={returnToLatest}>Latest</button>{/if}
      <span>{snapshot?.recordedSeconds?.toFixed(3) ?? "0.000"} s recorded</span>
      <span>loss {snapshot?.lostFrames ?? 0}</span>
    </div>
    <div class="scope-spike-plot">
      <ScopeEchartsView
        channels={activeChannels()}
        {snapshot}
        {verticalScale}
        {verticalOffset}
        {activeChannelId}
        {viewRange}
        onViewRangeChange={navigateToRange}
        onDoubleClick={returnToLatest}
      />
    </div>
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
  .scope-channel-notice {
    margin: 0 8px 7px;
    padding: 6px 8px;
    border-left: 2px solid #c8b77a;
    color: #c8b77a;
    background: #27251f;
    font-size: 10.5px;
    line-height: 1.4;
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
