<script lang="ts">
  import { onMount } from "svelte";
  import Split from "split.js";
  import uPlot from "uplot";
  import type { ConnectionInfo, PlotChannel } from "../../connection/types";
  import { configureScope, readScopeSnapshot, startScope, stopScope } from "./api";
  import { loadScopeViewSettings, saveScopeViewSettings, type ScopeViewSettings } from "./viewSettings";
  import type { ScopeConfig, ScopeRate, ScopeSnapshot, ScopeSummary } from "./types";

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
  const traceColors = [
    "#7aa2c8", "#c8b77a", "#9b8ac8", "#7fa68a",
    "#c28b73", "#aa829a", "#79a6ad", "#91a77b",
    "#b68f6a", "#6f9ca8", "#a47f86", "#8398b5",
  ];
  const cursorColors = ["#c9c9c9", "#c8b77a"];
  const markerHitPixels = 10;

  let plotHost: HTMLDivElement;
  let plot: uPlot | undefined;
  let split: Split.Instance | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let reconfigureTimer: ReturnType<typeof setTimeout> | undefined;
  let interactionRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  let scopeViewSaveTimer: ReturnType<typeof setTimeout> | undefined;

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
  let channelColors = new Map<number, number>();
  let plotChannels: PlotChannel[] = [];
  let visibleChannels: PlotChannel[] = [];
  let snapshot: ScopeSnapshot | undefined;
  let fastSelected = 0;
  let normalSelected = 0;
  let activeChannelId: number | undefined;
  let activeConnection: ConnectionInfo | undefined;

  let timePerDiv = 10e-3;
  let horizontalOffset = 0;
  let cursorEnabled = false;
  let cursorA: number | undefined;
  let cursorB: number | undefined;
  let isRunning = false;
  let viewNavigationActive = false;
  let followLatest = true;
  let hoverTime: number | undefined;
  let cursorGroupHit: { left: number; right: number; top: number; bottom: number } | undefined;

  type DragState =
    | { kind: "pan"; pointerId: number; startClientX: number; startOffset: number }
    | { kind: "vertical"; pointerId: number; id: number; startClientY: number; startOffset: number }
    | { kind: "cursor"; pointerId: number; cursor: "a" | "b" }
    | { kind: "cursor-group"; pointerId: number; startClientX: number; startA: number; startB: number };

  type CursorReadout = {
    id: number;
    label: string;
    unit: string;
    color: string;
    a?: number;
    b?: number;
    delta?: number;
  };

  let dragState: DragState | undefined;
  let cursorReadouts: CursorReadout[] = [];

  $: if (connection !== activeConnection) {
    activeConnection = connection;
    resetScope();
  }
  $: onSummary({ state: snapshot?.state ?? "STOPPED", selectedChannels: selectedIds.size, lostFrames: snapshot?.lostFrames ?? 0 });
  $: visibleChannels = plotChannels.filter((channel) => selectedIds.has(channel.id));
  $: fastSelected = Array.from(selectedIds).filter((id) => channelRates.get(id) === "fast").length;
  $: normalSelected = Array.from(selectedIds).filter((id) => channelRates.get(id) === "normal").length;
  $: isRunning = snapshot?.state === "LIVE";
  $: followLatest = horizontalOffset <= 1e-12;
  $: cursorReadouts = cursorEnabled
    ? visibleChannels.map((channel) => {
        const a = cursorA === undefined ? undefined : sampleAtTime(channel.id, cursorA);
        const b = cursorB === undefined ? undefined : sampleAtTime(channel.id, cursorB);
        return {
          id: channel.id,
          label: channel.label,
          unit: channel.unit ?? "",
          color: traceColor(channel),
          a,
          b,
          delta: a === undefined || b === undefined ? undefined : b - a,
        };
      })
    : [];
  $: if (activeChannelId !== undefined && !selectedIds.has(activeChannelId)) {
    activeChannelId = visibleChannels[0]?.id;
  }

  $: if (connection && plotChannels.length > 0) {
    selectedIds;
    channelRates;
    channelColors;
    verticalScale;
    verticalOffset;
    timePerDiv;
    cursorEnabled;
    activeChannelId;
    scheduleScopeViewSave();
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

  type EngineeringScale = { value: number; unit: string; factor: number };

  function engineeringScale(value: number, unit: string): EngineeringScale {
    const abs = Math.abs(value);
    const prefixes = [
      { factor: 1e-6, prefix: "µ" },
      { factor: 1e-3, prefix: "m" },
      { factor: 1, prefix: "" },
      { factor: 1e3, prefix: "k" },
    ];

    let selected = prefixes[2];
    if (abs > 0 && abs < 1e-3) selected = prefixes[0];
    else if (abs > 0 && abs < 1) selected = prefixes[1];
    else if (abs >= 1e3) selected = prefixes[3];

    return {
      value: value / selected.factor,
      unit: `${selected.prefix}${unit}`,
      factor: selected.factor,
    };
  }

  function scaleCandidates(value: number): number[] {
    const safe = Math.max(Math.abs(value), 1e-15);
    const exponent = Math.floor(Math.log10(safe));
    const values: number[] = [];
    for (let power = exponent - 2; power <= exponent + 2; power += 1) {
      const decade = 10 ** power;
      values.push(decade, 2 * decade, 5 * decade);
    }
    return values.sort((a, b) => a - b);
  }

  function step125(value: number, direction: -1 | 1): number {
    const candidates = scaleCandidates(value);
    const epsilon = Math.max(Math.abs(value) * 1e-9, 1e-15);
    if (direction > 0) {
      return candidates.find((candidate) => candidate > value + epsilon) ?? value * 2;
    }
    return [...candidates].reverse().find((candidate) => candidate < value - epsilon) ?? value / 2;
  }

  function ceil125(value: number): number {
    if (!Number.isFinite(value) || value <= 0) return 1;
    const candidates = scaleCandidates(value);
    return candidates.find((candidate) => candidate >= value * (1 - 1e-12)) ?? value;
  }

  function stepVerticalScale(id: number, direction: -1 | 1) {
    const current = verticalScale.get(id) ?? 1;
    updateVerticalScale(id, step125(current, direction));
  }

  function commitEngineeringScale(id: number, displayed: string, factor: number) {
    const value = Number(displayed);
    if (!Number.isFinite(value) || value <= 0) return;
    updateVerticalScale(id, value * factor);
  }

  function scopeViewSettings(): ScopeViewSettings {
    return {
      version: 1,
      timePerDiv,
      cursorEnabled,
      activeChannelId,
      channels: plotChannels.map((channel) => ({
        id: channel.id,
        selected: selectedIds.has(channel.id),
        rate: channelRates.get(channel.id),
        color: channelColors.get(channel.id),
        scalePerDiv: verticalScale.get(channel.id),
        yPosition: verticalOffset.get(channel.id),
      })),
    };
  }

  function scheduleScopeViewSave() {
    if (!connection || plotChannels.length === 0) return;
    if (scopeViewSaveTimer) clearTimeout(scopeViewSaveTimer);
    scopeViewSaveTimer = setTimeout(() => {
      scopeViewSaveTimer = undefined;
      saveScopeViewSettings(plotChannels, scopeViewSettings());
    }, 250);
  }

  function validTimePerDiv(value: number | undefined): value is number {
    return value !== undefined && Number.isFinite(value) && value > 0;
  }

  function restoreScopeView(): boolean {
    const saved = loadScopeViewSettings(plotChannels);
    if (!saved) return false;

    const knownIds = new Set(plotChannels.map((channel) => channel.id));
    const selected = new Set<number>();
    const rates = new Map<number, ScopeRate>();
    const colors = new Map<number, number>();
    const scales = new Map(verticalScale);
    const offsets = new Map(verticalOffset);

    for (const setting of saved.channels) {
      if (!knownIds.has(setting.id)) continue;
      const channel = plotChannels.find((candidate) => candidate.id === setting.id);
      if (!channel) continue;

      if (setting.selected) selected.add(setting.id);
      if (setting.rate && rateAllowed(channel, setting.rate)) rates.set(setting.id, setting.rate);
      if (setting.color !== undefined && Number.isInteger(setting.color) && setting.color >= 0) {
        colors.set(setting.id, setting.color % traceColors.length);
      }
      if (validTimePerDiv(setting.scalePerDiv)) scales.set(setting.id, setting.scalePerDiv);
      if (setting.yPosition !== undefined && Number.isFinite(setting.yPosition)) offsets.set(setting.id, setting.yPosition);
    }

    if (selected.size === 0) return false;

    for (const id of selected) {
      const channel = plotChannels.find((candidate) => candidate.id === id);
      if (!channel) continue;
      if (!rates.has(id)) rates.set(id, defaultRate(channel));
    }

    selectedIds = selected;
    channelRates = rates;
    channelColors = colors;
    verticalScale = scales;
    verticalOffset = offsets;
    timePerDiv = validTimePerDiv(saved.timePerDiv) ? saved.timePerDiv : timePerDiv;
    cursorEnabled = Boolean(saved.cursorEnabled);
    activeChannelId = saved.activeChannelId !== undefined && selected.has(saved.activeChannelId)
      ? saved.activeChannelId
      : plotChannels.find((channel) => selected.has(channel.id))?.id;

    for (const id of selected) ensureChannelColor(id, selected);
    return true;
  }

  function initializeCursorPositions() {
    if (!cursorEnabled) return;
    const end = -horizontalOffset;
    const start = end - windowSeconds();
    cursorA = start + windowSeconds() * 0.3;
    cursorB = start + windowSeconds() * 0.7;
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
    channelColors = new Map(defaults.map((channel, index) => [channel.id, index % traceColors.length]));
    activeChannelId = defaults[0]?.id;
    restoreScopeView();
    initializeCursorPositions();
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
      ensureChannelColor(id, next);
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
    scheduleViewRefresh();
  }

  function updateHorizontalOffset(value: number) {
    horizontalOffset = Math.min(Math.max(0, value), maxHorizontalOffset());
    applyHorizontalScale();
    scheduleViewRefresh();
  }

  function goLatest() {
    horizontalOffset = 0;
    applyHorizontalScale();
    scheduleViewRefresh();
  }

  function scheduleViewRefresh(delay = 80) {
    viewNavigationActive = true;
    if (interactionRefreshTimer) clearTimeout(interactionRefreshTimer);
    interactionRefreshTimer = setTimeout(() => {
      interactionRefreshTimer = undefined;
      viewNavigationActive = false;
      void refreshSnapshot();
    }, delay);
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

  type PeriodCandidate = {
    period: number;
    stability: number;
    correlation: number;
  };

  function median(values: number[]): number {
    if (values.length === 0) return Number.NaN;
    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
      ? (sorted[middle - 1] + sorted[middle]) / 2
      : sorted[middle];
  }

  function estimatePeriod(times: number[], values: number[]): PeriodCandidate | undefined {
    if (times.length < 48 || times.length !== values.length) return undefined;

    const finiteValues = values.filter(Number.isFinite);
    if (finiteValues.length < 48) return undefined;

    const mean = finiteValues.reduce((sum, value) => sum + value, 0) / finiteValues.length;
    let min = Infinity;
    let max = -Infinity;
    let energy = 0;
    for (const value of finiteValues) {
      min = Math.min(min, value);
      max = Math.max(max, value);
      const centered = value - mean;
      energy += centered * centered;
    }

    const span = max - min;
    const rms = Math.sqrt(energy / finiteValues.length);
    if (!Number.isFinite(span) || span <= 1e-9 || rms <= 1e-9) return undefined;

    const hysteresis = Math.max(rms * 0.18, span * 0.04);
    const crossings: Array<{ time: number; index: number }> = [];
    let armed = false;

    for (let index = 0; index < values.length; index += 1) {
      const value = values[index];
      if (!Number.isFinite(value)) continue;
      const centered = value - mean;
      if (centered <= -hysteresis) {
        armed = true;
      } else if (armed && centered >= hysteresis) {
        crossings.push({ time: times[index], index });
        armed = false;
      }
    }

    if (crossings.length < 4) return undefined;

    const periods: number[] = [];
    const lagSamples: number[] = [];
    for (let index = 1; index < crossings.length; index += 1) {
      const period = crossings[index].time - crossings[index - 1].time;
      const lag = crossings[index].index - crossings[index - 1].index;
      if (period > 0 && lag >= 8) {
        periods.push(period);
        lagSamples.push(lag);
      }
    }
    if (periods.length < 3) return undefined;

    const period = median(periods);
    const lag = Math.round(median(lagSamples));
    if (!Number.isFinite(period) || period <= 0 || lag < 8) return undefined;

    const deviations = periods.map((value) => Math.abs(value - period));
    const stability = median(deviations) / period;
    if (!Number.isFinite(stability) || stability > 0.08) return undefined;

    let numerator = 0;
    let leftEnergy = 0;
    let rightEnergy = 0;
    let pairs = 0;
    for (let index = 0; index + lag < values.length; index += 1) {
      const left = values[index];
      const right = values[index + lag];
      if (!Number.isFinite(left) || !Number.isFinite(right)) continue;
      const a = left - mean;
      const b = right - mean;
      numerator += a * b;
      leftEnergy += a * a;
      rightEnergy += b * b;
      pairs += 1;
    }
    if (pairs < 32 || leftEnergy <= 0 || rightEnergy <= 0) return undefined;

    const correlation = numerator / Math.sqrt(leftEnergy * rightEnergy);
    if (!Number.isFinite(correlation) || correlation < 0.82) return undefined;

    return { period, stability, correlation };
  }

  function consensusPeriod(candidates: PeriodCandidate[], visibleCount: number): number | undefined {
    if (candidates.length === 0) return undefined;

    if (visibleCount <= 1) {
      const candidate = candidates[0];
      return candidate.correlation >= 0.9 && candidate.stability <= 0.05
        ? candidate.period
        : undefined;
    }

    let best: PeriodCandidate[] = [];
    for (const seed of candidates) {
      const cluster = candidates.filter((candidate) => {
        const reference = Math.max(seed.period, candidate.period);
        return reference > 0 && Math.abs(candidate.period - seed.period) / reference <= 0.08;
      });
      if (cluster.length > best.length) best = cluster;
    }

    if (best.length < 2) return undefined;
    return median(best.map((candidate) => candidate.period));
  }

  function nearestTimePerDiv(target: number): number {
    return timeDivOptions.reduce((best, candidate) =>
      Math.abs(candidate - target) < Math.abs(best - target) ? candidate : best,
    timeDivOptions[0]);
  }

  function autoVertical(source: ScopeSnapshot) {
    const scales = new Map(verticalScale);
    const offsets = new Map(verticalOffset);

    for (const channel of visibleChannels) {
      const values = source.series.find((series) => series.id === channel.id)?.values ?? [];
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
      scales.set(channel.id, ceil125(span / (verticalDivisions * 0.75)));
    }

    verticalScale = scales;
    verticalOffset = offsets;
    for (const id of selectedIds) applyVerticalScale(id);
  }

  async function autoSet() {
    if (!snapshot || !scopeConfig || commandBusy) return;

    commandBusy = true;
    try {
      horizontalOffset = 0;

      const analysisWindow = Math.min(
        historySeconds(),
        Math.max(windowSeconds(), 3.0),
      );
      const analysis = await readScopeSnapshot(analysisWindow, 0, 10_000);

      autoVertical(analysis);

      const candidates: PeriodCandidate[] = [];
      for (const channel of visibleChannels) {
        const series = analysis.series.find((item) => item.id === channel.id);
        if (!series) continue;
        const candidate = estimatePeriod(series.times, series.values);
        if (candidate) candidates.push(candidate);
      }

      const period = consensusPeriod(candidates, visibleChannels.length);
      if (period !== undefined) {
        const targetScreen = period * 2.5;
        const nextTimePerDiv = nearestTimePerDiv(targetScreen / horizontalDivisions);
        timePerDiv = nextTimePerDiv;
      }

      horizontalOffset = Math.min(horizontalOffset, maxHorizontalOffset());
      applyHorizontalScale();
      await refreshSnapshot();
    } catch (error) {
      onError(error);
    } finally {
      commandBusy = false;
    }
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
      initializeCursorPositions();
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
          autoVertical(next);
          initialVerticalFit = false;
        }
      }
    }
    catch (error) { if (revision === snapshotRevision) onError(error); }
    finally { snapshotBusy = false; }
  }

  function ensureChannelColor(id: number, selected = selectedIds): number {
    const preferred = channelColors.get(id);
    const used = new Set(
      Array.from(selected)
        .filter((selectedId) => selectedId !== id)
        .map((selectedId) => channelColors.get(selectedId))
        .filter((index): index is number => index !== undefined),
    );

    if (preferred !== undefined && !used.has(preferred)) return preferred;

    const available = traceColors.findIndex((_, index) => !used.has(index));
    const assigned = available >= 0 ? available : (preferred ?? 0);
    channelColors = new Map(channelColors).set(id, assigned);
    return assigned;
  }

  function traceColor(channel: PlotChannel): string {
    const index = channelColors.get(channel.id);
    if (index === undefined) return "#666666";
    return traceColors[index % traceColors.length];
  }

  function sampleAtTime(id: number, time: number): number | undefined {
    const series = snapshot?.series.find((item) => item.id === id);
    if (!series || series.times.length === 0 || series.times.length !== series.values.length) return undefined;

    let low = 0;
    let high = series.times.length - 1;
    if (time <= series.times[low]) return series.values[low];
    if (time >= series.times[high]) return series.values[high];

    while (high - low > 1) {
      const middle = Math.floor((low + high) / 2);
      if (series.times[middle] <= time) low = middle;
      else high = middle;
    }
    return Math.abs(series.times[low] - time) <= Math.abs(series.times[high] - time)
      ? series.values[low]
      : series.values[high];
  }

  function formatCursorValue(value: number | undefined, unit: string): string {
    if (value === undefined || !Number.isFinite(value)) return "—";
    return `${value.toFixed(3)}${unit ? ` ${unit}` : ""}`;
  }

  function formatFrequency(value: number): string {
    if (!Number.isFinite(value) || value <= 0) return "—";
    if (value >= 1000) return `${(value / 1000).toFixed(value >= 10_000 ? 1 : 2)} kHz`;
    return `${value.toFixed(value >= 100 ? 1 : 2)} Hz`;
  }

  function cursorIntervalLabel(): string | undefined {
    if (cursorA === undefined || cursorB === undefined || cursorA === cursorB) return undefined;
    const delta = Math.abs(cursorB - cursorA);
    return `${formatTime(delta)} · ${formatFrequency(1 / delta)}`;
  }

  function cursorPlugin(): uPlot.Plugin {
    return {
      hooks: {
        draw: [
          (u) => {
            const ctx = u.ctx;
            const px = window.devicePixelRatio || 1;
            const marker = 7 * px;

            ctx.save();

            for (const channel of visibleChannels) {
              const y = u.valToPos(0, yScaleKey(channel.id), true);
              if (!Number.isFinite(y)) continue;
              const top = u.bbox.top;
              const bottom = u.bbox.top + u.bbox.height;
              const clampedY = Math.min(Math.max(y, top + marker), bottom - marker);
              const x = u.bbox.left;
              ctx.fillStyle = traceColor(channel);
              ctx.beginPath();
              ctx.moveTo(x, clampedY - marker);
              ctx.lineTo(x + marker, clampedY);
              ctx.lineTo(x, clampedY + marker);
              ctx.closePath();
              ctx.fill();
            }

            if (hoverTime !== undefined && !dragState) {
              const hoverX = u.valToPos(hoverTime, "x", true);
              const top = u.bbox.top;
              const hoverMarker = 5 * px;
              ctx.fillStyle = "#858585";
              ctx.beginPath();
              ctx.moveTo(hoverX - hoverMarker, top);
              ctx.lineTo(hoverX + hoverMarker, top);
              ctx.lineTo(hoverX, top + hoverMarker);
              ctx.closePath();
              ctx.fill();
              ctx.font = `${9 * px}px "SFMono-Regular", Consolas, monospace`;
              ctx.textAlign = "center";
              ctx.textBaseline = "top";
              ctx.fillText(formatSignedTime(hoverTime), hoverX, top + hoverMarker + 2 * px);
            }

            cursorGroupHit = undefined;
            if (cursorEnabled) {
              const values: Array<{ value: number | undefined; color: string; label: string }> = [
                { value: cursorA, color: cursorColors[0], label: "X1" },
                { value: cursorB, color: cursorColors[1], label: "X2" },
              ];
              for (const item of values) {
                if (item.value === undefined) continue;
                const x = u.valToPos(item.value, "x", true);
                const top = u.bbox.top;
                const bottom = u.bbox.top + u.bbox.height;

                ctx.strokeStyle = item.color;
                ctx.lineWidth = px;
                ctx.setLineDash([4 * px, 4 * px]);
                ctx.beginPath();
                ctx.moveTo(x, top);
                ctx.lineTo(x, bottom);
                ctx.stroke();

                ctx.setLineDash([]);
                ctx.fillStyle = item.color;
                ctx.beginPath();
                ctx.moveTo(x - marker, top);
                ctx.lineTo(x + marker, top);
                ctx.lineTo(x, top + marker);
                ctx.closePath();
                ctx.fill();

                ctx.beginPath();
                ctx.moveTo(x - marker, bottom);
                ctx.lineTo(x + marker, bottom);
                ctx.lineTo(x, bottom - marker);
                ctx.closePath();
                ctx.fill();

                ctx.font = `${9 * px}px "SFMono-Regular", Consolas, monospace`;
                ctx.textAlign = "left";
                ctx.textBaseline = "top";
                ctx.fillText(item.label, x + marker + 2 * px, top + 2 * px);
              }

              const intervalLabel = cursorIntervalLabel();
              if (intervalLabel && cursorA !== undefined && cursorB !== undefined) {
                const cssX1 = u.valToPos(cursorA, "x");
                const cssX2 = u.valToPos(cursorB, "x");
                const cssMid = (cssX1 + cssX2) / 2;
                const cssWidth = Math.max(94, intervalLabel.length * 6.2 + 18);
                cursorGroupHit = {
                  left: cssMid - cssWidth / 2,
                  right: cssMid + cssWidth / 2,
                  top: 10,
                  bottom: 30,
                };

                const canvasMid = (u.valToPos(cursorA, "x", true) + u.valToPos(cursorB, "x", true)) / 2;
                const canvasWidth = cssWidth * px;
                const labelTop = u.bbox.top + 10 * px;
                const labelHeight = 20 * px;
                ctx.fillStyle = "rgba(30, 30, 30, 0.92)";
                ctx.strokeStyle = "#555b64";
                ctx.lineWidth = px;
                ctx.fillRect(canvasMid - canvasWidth / 2, labelTop, canvasWidth, labelHeight);
                ctx.strokeRect(canvasMid - canvasWidth / 2, labelTop, canvasWidth, labelHeight);
                ctx.fillStyle = "#c4c4c4";
                ctx.font = `${9 * px}px "SFMono-Regular", Consolas, monospace`;
                ctx.textAlign = "center";
                ctx.textBaseline = "middle";
                ctx.fillText(intervalLabel, canvasMid, labelTop + labelHeight / 2);
              }
            }

            ctx.restore();
          },
        ],
      },
    };
  }

  function plotPointerPosition(event: PointerEvent | WheelEvent) {
    const chart = plot;
    if (!chart) return undefined;
    const rect = chart.over.getBoundingClientRect();
    return {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
      width: rect.width,
      height: rect.height,
    };
  }

  function hitCursor(x: number): "a" | "b" | undefined {
    if (!cursorEnabled || !plot) return undefined;
    if (cursorA !== undefined && Math.abs(plot.valToPos(cursorA, "x") - x) <= markerHitPixels) return "a";
    if (cursorB !== undefined && Math.abs(plot.valToPos(cursorB, "x") - x) <= markerHitPixels) return "b";
    return undefined;
  }

  function hitCursorGroup(x: number, y: number): boolean {
    const hit = cursorGroupHit;
    return Boolean(hit && x >= hit.left && x <= hit.right && y >= hit.top && y <= hit.bottom);
  }

  function hitVerticalMarker(x: number, y: number): number | undefined {
    if (!plot || x > markerHitPixels * 2) return undefined;
    const height = plot.over.getBoundingClientRect().height;
    for (const channel of visibleChannels) {
      const rawY = plot.valToPos(0, yScaleKey(channel.id));
      if (!Number.isFinite(rawY)) continue;
      const markerY = Math.min(Math.max(rawY, markerHitPixels), Math.max(markerHitPixels, height - markerHitPixels));
      if (Math.abs(markerY - y) <= markerHitPixels) return channel.id;
    }
    return undefined;
  }

  function handlePlotPointerDown(event: PointerEvent) {
    if (!plot || event.button !== 0) return;
    const point = plotPointerPosition(event);
    if (!point) return;

    const cursor = hitCursor(point.x);
    if (hitCursorGroup(point.x, point.y) && cursorA !== undefined && cursorB !== undefined) {
      dragState = {
        kind: "cursor-group",
        pointerId: event.pointerId,
        startClientX: event.clientX,
        startA: cursorA,
        startB: cursorB,
      };
      plot.over.style.cursor = "ew-resize";
    } else if (cursor) {
      dragState = { kind: "cursor", pointerId: event.pointerId, cursor };
      plot.over.style.cursor = "ew-resize";
    } else {
      const channelId = hitVerticalMarker(point.x, point.y);
      if (channelId !== undefined) {
        dragState = {
          kind: "vertical",
          pointerId: event.pointerId,
          id: channelId,
          startClientY: event.clientY,
          startOffset: verticalOffset.get(channelId) ?? 0,
        };
        plot.over.style.cursor = "ns-resize";
      } else {
        dragState = {
          kind: "pan",
          pointerId: event.pointerId,
          startClientX: event.clientX,
          startOffset: horizontalOffset,
        };
        viewNavigationActive = true;
        plot.over.style.cursor = "grabbing";
      }
    }

    plot.over.setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function handlePlotPointerMove(event: PointerEvent) {
    const chart = plot;
    const point = plotPointerPosition(event);
    if (!chart || !point) return;

    if (!dragState) {
      const nextHover = chart.posToVal(Math.min(Math.max(0, point.x), point.width), "x");
      if (hoverTime !== nextHover) {
        hoverTime = nextHover;
        chart.redraw(false, false);
      }
      const cursor = hitCursor(point.x);
      const group = hitCursorGroup(point.x, point.y);
      const channelId = hitVerticalMarker(point.x, point.y);
      chart.over.style.cursor = group || cursor ? "ew-resize" : channelId !== undefined ? "ns-resize" : "grab";
      return;
    }
    if (dragState.pointerId !== event.pointerId) return;

    if (dragState.kind === "cursor") {
      const scale = chart.scales.x;
      if (scale.min === undefined || scale.max === undefined) return;
      const x = Math.min(Math.max(0, point.x), point.width);
      const value = Math.min(Math.max(chart.posToVal(x, "x"), scale.min), scale.max);
      if (dragState.cursor === "a") cursorA = value;
      else cursorB = value;
      chart.redraw(false, false);
      return;
    }

    if (dragState.kind === "cursor-group") {
      const scale = chart.scales.x;
      if (scale.min === undefined || scale.max === undefined) return;
      const secondsPerPixel = (scale.max - scale.min) / Math.max(point.width, 1);
      const requested = (event.clientX - dragState.startClientX) * secondsPerPixel;
      const low = Math.min(dragState.startA, dragState.startB);
      const high = Math.max(dragState.startA, dragState.startB);
      const delta = Math.min(Math.max(requested, scale.min - low), scale.max - high);
      cursorA = dragState.startA + delta;
      cursorB = dragState.startB + delta;
      chart.redraw(false, false);
      return;
    }

    if (dragState.kind === "vertical") {
      const perDiv = verticalScale.get(dragState.id) ?? 1;
      const unitsPerPixel = perDiv * verticalDivisions / Math.max(point.height, 1);
      const nextOffset = dragState.startOffset + (event.clientY - dragState.startClientY) * unitsPerPixel;
      updateVerticalOffset(dragState.id, nextOffset);
      return;
    }

    const secondsPerPixel = windowSeconds() / Math.max(point.width, 1);
    horizontalOffset = Math.min(
      Math.max(0, dragState.startOffset + (event.clientX - dragState.startClientX) * secondsPerPixel),
      maxHorizontalOffset(),
    );
    applyHorizontalScale();
  }

  function finishPlotDrag(event: PointerEvent) {
    const chart = plot;
    if (!chart || !dragState || dragState.pointerId !== event.pointerId) return;
    const completed = dragState;
    dragState = undefined;
    if (chart.over.hasPointerCapture(event.pointerId)) chart.over.releasePointerCapture(event.pointerId);
    chart.over.style.cursor = "grab";
    if (completed.kind === "pan") scheduleViewRefresh(0);
  }

  function handlePlotWheel(event: WheelEvent) {
    const chart = plot;
    const point = plotPointerPosition(event);
    if (!chart || !point || event.deltaY === 0) return;

    const verticalChannel = hitVerticalMarker(point.x, point.y);
    if (verticalChannel !== undefined) {
      event.preventDefault();
      stepVerticalScale(verticalChannel, event.deltaY > 0 ? 1 : -1);
      return;
    }

    const currentIndex = timeDivOptions.findIndex((value) => value === timePerDiv);
    if (currentIndex < 0) return;
    const nextIndex = Math.min(
      timeDivOptions.length - 1,
      Math.max(0, currentIndex + (event.deltaY > 0 ? 1 : -1)),
    );
    if (nextIndex === currentIndex) return;

    event.preventDefault();

    const ratio = Math.min(Math.max(point.x / Math.max(point.width, 1), 0), 1);
    const oldWindow = windowSeconds();
    const oldRight = -horizontalOffset;
    const oldLeft = oldRight - oldWindow;
    const anchorTime = oldLeft + ratio * oldWindow;

    timePerDiv = timeDivOptions[nextIndex];
    const newWindow = windowSeconds();
    const newLeft = anchorTime - ratio * newWindow;
    const newRight = newLeft + newWindow;
    horizontalOffset = Math.min(Math.max(0, -newRight), maxHorizontalOffset());

    applyHorizontalScale();
    scheduleViewRefresh();
  }

  function handlePlotPointerLeave() {
    if (dragState || hoverTime === undefined) return;
    hoverTime = undefined;
    plot?.redraw(false, false);
  }

  function bindPlotInteractions() {
    if (!plot) return;
    plot.over.style.cursor = "grab";
    plot.over.addEventListener("pointerdown", handlePlotPointerDown);
    plot.over.addEventListener("pointermove", handlePlotPointerMove);
    plot.over.addEventListener("pointerup", finishPlotDrag);
    plot.over.addEventListener("pointercancel", finishPlotDrag);
    plot.over.addEventListener("pointerleave", handlePlotPointerLeave);
    plot.over.addEventListener("wheel", handlePlotWheel, { passive: false });
  }

  function unbindPlotInteractions() {
    if (!plot) return;
    plot.over.removeEventListener("pointerdown", handlePlotPointerDown);
    plot.over.removeEventListener("pointermove", handlePlotPointerMove);
    plot.over.removeEventListener("pointerup", finishPlotDrag);
    plot.over.removeEventListener("pointercancel", finishPlotDrag);
    plot.over.removeEventListener("pointerleave", handlePlotPointerLeave);
    plot.over.removeEventListener("wheel", handlePlotWheel);
  }

  function toggleCursor() {
    cursorEnabled = !cursorEnabled;
    if (cursorEnabled) {
      const scale = plot?.scales.x;
      const min = scale?.min ?? (-horizontalOffset - windowSeconds());
      const max = scale?.max ?? -horizontalOffset;
      const span = max - min;
      cursorA = min + span * 0.3;
      cursorB = min + span * 0.7;
    } else {
      cursorA = undefined;
      cursorB = undefined;
    }
    plot?.redraw(false, false);
  }

  function rebuildPlot() {
    if (!plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    unbindPlotInteractions();
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
        label: channel.label,
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

    bindPlotInteractions();
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
        if (!viewNavigationActive) void refreshSnapshot();
      } else if (++hiddenTicks >= 10) {
        hiddenTicks = 0;
        void refreshSnapshot();
      }
    }, 50);
    return () => {
      if (refreshTimer) clearInterval(refreshTimer);
      if (reconfigureTimer) clearTimeout(reconfigureTimer);
      if (interactionRefreshTimer) clearTimeout(interactionRefreshTimer);
      if (scopeViewSaveTimer) clearTimeout(scopeViewSaveTimer);
      resizeObserver?.disconnect();
      unbindPlotInteractions();
      plot?.destroy();
      split?.destroy();
    };
  });
</script>

<section class="page-toolbar">
  <div class="page-title">ANALYSIS / SCOPE</div>
  <div class="toolbar-actions">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <vscode-button disabled={!connection || selectedIds.size === 0 || commandBusy} onclick={toggleRunStop}>
      <i class={`codicon ${isRunning ? "codicon-debug-stop" : "codicon-play"}`}></i>&nbsp;{isRunning ? "Stop" : "Run"}
    </vscode-button>
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <vscode-button secondary disabled={!scopeConfig || commandBusy} onclick={autoSet}>Auto Set</vscode-button>
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <vscode-button secondary class:scope-tool-active={cursorEnabled} disabled={!scopeConfig} onclick={toggleCursor}>Cursor</vscode-button>
  </div>
</section>

<div class="scope-shell">
  <aside id="scope-sidebar" class="scope-sidebar">
    <section class="side-section">
      <div class="section-heading">CHANNELS</div>
      <div class="channel-list">
        {#if connection}
          {#each plotChannels as channel}
            <div class="channel-entry">
              <div class:active-channel={activeChannelId === channel.id} class="channel-row">
                <input
                  id={`scope-channel-${channel.id}`}
                  class="scope-channel-checkbox"
                  type="checkbox"
                  checked={selectedIds.has(channel.id)}
                  onchange={() => toggleChannel(channel.id)}
                />
                <button class="channel-name channel-select" onclick={() => selectActiveChannel(channel.id)}>
                  <span
                    class:inactive={!selectedIds.has(channel.id)}
                    class="channel-color-mark"
                    style={selectedIds.has(channel.id) ? `background:${traceColor(channel)}` : ""}
                  ></span>
                  <span class="channel-label">{channel.label}</span>
                </button>
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
              {#if selectedIds.has(channel.id)}
                {@const scaleDisplay = engineeringScale(verticalScale.get(channel.id) ?? 1, channel.unit ?? "")}
                <div class="channel-y-controls">
                  <label class="channel-y-field">
                    <span>Scale/div</span>
                    <input
                      type="number"
                      min="0.000001"
                      step="any"
                      value={scaleDisplay.value}
                      onfocus={() => selectActiveChannel(channel.id)}
                      onchange={(event) => commitEngineeringScale(channel.id, (event.currentTarget as HTMLInputElement).value, scaleDisplay.factor)}
                    />
                    <em>{scaleDisplay.unit}/div</em>
                  </label>
                  <label class="channel-y-field">
                    <span>Y Pos</span>
                    <input
                      type="number"
                      step="any"
                      value={verticalOffset.get(channel.id) ?? 0}
                      onfocus={() => selectActiveChannel(channel.id)}
                      oninput={(event) => updateVerticalOffset(channel.id, Number((event.currentTarget as HTMLInputElement).value))}
                    />
                    <em>{channel.unit ?? ""}</em>
                  </label>
                </div>
              {/if}
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
        <div class="scope-readout">
          <span>Position</span>
          <div class="scope-position-readout">
            <strong>{horizontalOffset === 0 ? "Latest" : `-${formatTime(horizontalOffset)}`}</strong>
            {#if horizontalOffset > 0}
              <button class="scope-latest-button" onclick={goLatest}>Latest</button>
            {/if}
          </div>
        </div>
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

    {#if cursorEnabled}
      <section class="side-section">
        <div class="section-heading">CURSOR</div>
        <div class="property-grid">
          <span>X1</span><strong style={`color:${cursorColors[0]}`}>{cursorA === undefined ? "—" : formatSignedTime(cursorA)}</strong>
          <span>X2</span><strong style={`color:${cursorColors[1]}`}>{cursorB === undefined ? "—" : formatSignedTime(cursorB)}</strong>
          <span>Δt</span><strong>{cursorA === undefined || cursorB === undefined ? "—" : formatTime(Math.abs(cursorB - cursorA))}</strong>
          <span>1/Δt</span><strong>{cursorA === undefined || cursorB === undefined || cursorA === cursorB ? "—" : `${(1 / Math.abs(cursorB - cursorA)).toFixed(2)} Hz`}</strong>
        </div>
        {#if cursorReadouts.length > 0}
          <div class="scope-cursor-readouts">
            <div class="scope-cursor-readout-header">
              <span>Channel</span><span>X1</span><span>X2</span><span>ΔY</span>
            </div>
            {#each cursorReadouts as row}
              <div class="scope-cursor-readout-row">
                <span class="scope-cursor-channel"><i style={`background:${row.color}`}></i>{row.label}</span>
                <strong>{formatCursorValue(row.a, row.unit)}</strong>
                <strong>{formatCursorValue(row.b, row.unit)}</strong>
                <strong>{formatCursorValue(row.delta, row.unit)}</strong>
              </div>
            {/each}
          </div>
        {/if}
        <div class="scope-pending">Drag X1/X2 lines or their top/bottom markers. Drag empty plot space to browse history.</div>
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

  </aside>

  <section id="scope-workspace" class="scope-workspace">
    <div class="editor-tabs"><div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div></div>
    <div class="plot-header">
      {#if visibleChannels.length === 0}
        <div class="plot-placeholder">{connection ? "Select channels and press Run." : "Connect a device before using Scope."}</div>
      {/if}
      <div class="plot-meta">{snapshot?.sampleCount ?? 0} samples · loss {snapshot?.lostFrames ?? 0}</div>
    </div>
    <div bind:this={plotHost} class="plot-host scope-plot-interactive"></div>
  </section>
</div>
