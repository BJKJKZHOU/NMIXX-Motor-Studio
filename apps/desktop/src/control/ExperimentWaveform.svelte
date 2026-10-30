<script lang="ts">
  import { onDestroy } from "svelte";
  import { uiLanguage, translate } from "../i18n";
  import ScopeEchartsView from "../analysis/scope/ScopeEchartsView.svelte";
  import { tuningWaveform, tuningTimeRange } from "./tuningWaveform";
  import type { TuningExperimentSnapshot } from "./tuningExperiment";

  type Props = {
    result: TuningExperimentSnapshot | undefined;
    timePerDiv?: number;
    onViewRequest?: (windowSeconds: number, endOffsetSeconds: number, maxPoints: number) => void;
  };
  let { result, timePerDiv = $bindable(20e-3), onViewRequest = () => undefined }: Props = $props();
  type Range = [number, number];
  const horizontalDivisions = 10;
  let selectedId = $state<number | undefined>();
  let manualRange = $state<Range | undefined>();
  let multipliers = $state<Record<number, number>>({});
  let manualTimeRange = $state<Range | undefined>();
  let lastCaptureId = $state<number | undefined>();
  let timeRange = $derived<Range>(result ? tuningTimeRange(result) : [0, 3]);
  let viewRange = $derived<Range>(manualTimeRange ?? timeRange);
  let viewRequestTimer: ReturnType<typeof setTimeout> | undefined;
  let display = $derived(result ? tuningWaveform(result) : undefined);
  let channels = $derived(display?.channels ?? []);
  let active = $derived(channels.find((channel) => channel.id === selectedId) ?? channels[0]);
  let defaultRange = $derived.by((): Range => {
    let min = Infinity;
    let max = -Infinity;
    for (const channel of channels) {
      const source = display?.snapshot.series.find((series) => series.id === channel.id);
      for (const values of [source?.values, source?.envelopeMin, source?.envelopeMax]) {
        for (const value of values ?? []) {
          if (Number.isFinite(value)) { min = Math.min(min, value); max = Math.max(max, value); }
        }
      }
    }
    if (!Number.isFinite(min) || !Number.isFinite(max)) return [-1, 1];
    const margin = Math.max((max - min) * 0.1, Math.abs(max) * 0.01, 0.001);
    return [min - margin, max + margin];
  });
  let sharedYRange = $derived(manualRange ?? defaultRange);
  let valueMultipliers = $derived(new Map(channels.map((channel) => [channel.id, multipliers[channel.id] ?? 1])));

  $effect(() => {
    if (result && result.captureId !== lastCaptureId) {
      // 新运动恢复全程视图；同次实验刷新时保留用户放大的区间，不被采样时长反复重置。
      lastCaptureId = result.captureId;
      manualTimeRange = undefined;
    }
  });
  $effect(() => { timePerDiv = (viewRange[1] - viewRange[0]) / horizontalDivisions; });

  function numberText(value: number): string { return String(Number(value.toPrecision(7))); }
  function setMultiplier(input: HTMLInputElement) {
    if (!active) return;
    const value = Number(input.value);
    const samples = display?.snapshot.series.find((series) => series.id === active.id)?.values ?? [];
    if (input.value.trim() && Number.isFinite(value) && value > 0
      && samples.every((sample) => !Number.isFinite(sample) || Number.isFinite(sample * value))) {
      // A gain changes only this curve; freeze the common axis so it cannot cancel the gain.
      manualRange = [...sharedYRange];
      multipliers[active.id] = value;
    }
    input.value = numberText(multipliers[active.id] ?? 1);
  }
  function setBound(index: 0 | 1, input: HTMLInputElement) {
    const next: Range = [...sharedYRange];
    next[index] = Number(input.value);
    if (input.value.trim() && next.every(Number.isFinite) && next[0] < next[1]
      && Number.isFinite(next[1] - next[0])) {
      manualRange = next;
    }
    input.value = numberText(sharedYRange[index]);
  }
  function commitOnEnter(event: KeyboardEvent) {
    if (event.key === "Enter") (event.currentTarget as HTMLInputElement).blur();
  }
  function scheduleViewRequest(range: Range, delay = 80) {
    if (viewRequestTimer) clearTimeout(viewRequestTimer);
    viewRequestTimer = setTimeout(() => {
      viewRequestTimer = undefined;
      const windowSeconds = Math.max(range[1] - range[0], 0.0005);
      // 图上是相对采集起点的正时间；IPC 仍使用距最新记录的尾部偏移。
      const endOffsetSeconds = Math.max(0, (result?.recordedSeconds ?? 0) - range[1]);
      timePerDiv = windowSeconds / horizontalDivisions;
      onViewRequest(windowSeconds, endOffsetSeconds, 3000);
    }, delay);
  }
  function handleViewRangeChange(range: Range) {
    manualTimeRange = range;
    scheduleViewRequest(range);
  }
  function returnToFull() {
    manualTimeRange = undefined;
    scheduleViewRequest(timeRange, 0);
  }
  onDestroy(() => { if (viewRequestTimer) clearTimeout(viewRequestTimer); });
</script>

{#if !result}
  <div class="waveform-empty">
    <i class="codicon codicon-graph-line"></i>
    <div>{translate("tuning.experiment.empty", $uiLanguage)}</div>
  </div>
{:else}
  <div class="waveform-view">
    {#if active}
      <div class="waveform-y-controls">
        <label class="axis-channel">{translate("tuning.waveform.channel", $uiLanguage)}
          <select value={active.id} onchange={(event) => selectedId = Number(event.currentTarget.value)}>
            {#each channels as channel (channel.id)}<option value={channel.id}>{channel.label}{channel.unit ? ` (${channel.unit})` : ""}</option>{/each}
          </select>
        </label>
        <label>{translate("tuning.waveform.zoom", $uiLanguage)}
          <input type="number" min="0.000001" step="any" value={multipliers[active.id] ?? 1} onchange={(event) => setMultiplier(event.currentTarget)} onkeydown={commitOnEnter} />
        </label>
        <label>{translate("tuning.waveform.y.min", $uiLanguage)}
          <input type="number" step="any" value={numberText(sharedYRange[0])} onchange={(event) => setBound(0, event.currentTarget)} onkeydown={commitOnEnter} />
        </label>
        <label>{translate("tuning.waveform.y.max", $uiLanguage)}
          <input type="number" step="any" value={numberText(sharedYRange[1])} onchange={(event) => setBound(1, event.currentTarget)} onkeydown={commitOnEnter} />
        </label>
      </div>
    {/if}
    <div class="waveform-chart">
      <ScopeEchartsView {channels} snapshot={display?.snapshot} {sharedYRange} {valueMultipliers}
        activeChannelId={active?.id} {viewRange} {timeRange} showLegend={true}
        onViewRangeChange={handleViewRangeChange} onDoubleClick={returnToFull} />
    </div>
  </div>
{/if}

<style>
  .waveform-empty { flex: 1; min-height: 260px; display: grid; place-items: center; align-content: center; gap: 8px; color: var(--vscode-descriptionForeground); border: 1px dashed var(--vscode-panel-border); text-align: center; }
  .waveform-empty .codicon { font-size: 24px; }
  .waveform-view { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 8px; }
  .waveform-chart { flex: 1; min-height: 240px; }
  .waveform-y-controls { display: flex; flex-wrap: wrap; gap: 8px 12px; align-items: end; font-size: 11px; color: var(--vscode-descriptionForeground); }
  .waveform-y-controls label { display: grid; gap: 4px; }
  .axis-channel { flex: 1; min-width: 110px; }
  .waveform-y-controls input { width: 80px; height: 25px; box-sizing: border-box; padding: 0 6px; border: 1px solid var(--vscode-input-border, #454545); border-radius: 2px; background: var(--vscode-input-background); color: var(--vscode-input-foreground); font: inherit; font-variant-numeric: tabular-nums; }
  .waveform-y-controls input:focus { outline: 1px solid var(--vscode-focusBorder); }
  .waveform-y-controls select { max-width: 100%; height: 25px; background: var(--vscode-dropdown-background); color: var(--vscode-dropdown-foreground); border: 1px solid var(--vscode-dropdown-border); font: inherit; }
</style>
