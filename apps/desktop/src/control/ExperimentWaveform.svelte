<script lang="ts">
  import ScopeEchartsView from "../analysis/scope/ScopeEchartsView.svelte";
  import type { ScopeChannel } from "../analysis/scope/types";
  import type { TuningExperimentSnapshot } from "./tuningExperiment";

  export let result: TuningExperimentSnapshot | undefined;
  export let multipliers: Record<number, number> = {};
  export let timePerDiv = 20e-3;
  export let onViewRequest: (windowSeconds: number, endOffsetSeconds: number, maxPoints: number) => void = () => undefined;

  const horizontalDivisions = 10;
  const verticalScale = new Map<number, number>();
  const verticalOffset = new Map<number, number>();
  let viewRange: [number, number] = [-timePerDiv * horizontalDivisions, 0];
  let viewRequestTimer: ReturnType<typeof setTimeout> | undefined;
  let lastRecordedSeconds = -1;

  $: channels = result?.config.channels ?? [];
  $: if (result && result.recordedSeconds !== lastRecordedSeconds) {
    lastRecordedSeconds = result.recordedSeconds;
    const window = Math.max(result.windowSeconds, 0.0005);
    const end = -Math.max(result.endOffsetSeconds, 0);
    viewRange = [end - window, end];
    timePerDiv = window / horizontalDivisions;
  }

  function multiplierMap(): Map<number, number> {
    const values = new Map<number, number>();
    for (const channel of channels) {
      const value = multipliers[channel.id];
      values.set(channel.id, Number.isFinite(value) && value > 0 ? value : 1);
    }
    return values;
  }

  function activeChannelId(): number | undefined {
    return channels[0]?.id;
  }

  function scheduleViewRequest(range: [number, number], delay = 80) {
    if (viewRequestTimer) clearTimeout(viewRequestTimer);
    viewRequestTimer = setTimeout(() => {
      viewRequestTimer = undefined;
      const windowSeconds = Math.max(range[1] - range[0], 0.0005);
      const endOffsetSeconds = Math.max(0, -range[1]);
      timePerDiv = windowSeconds / horizontalDivisions;
      onViewRequest(windowSeconds, endOffsetSeconds, 3000);
    }, delay);
  }

  function handleViewRangeChange(range: [number, number]) {
    viewRange = range;
    scheduleViewRequest(range);
  }

  function returnToLatest() {
    const span = Math.max(viewRange[1] - viewRange[0], 0.0005);
    viewRange = [-span, 0];
    scheduleViewRequest(viewRange, 0);
  }
</script>

{#if !result}
  <div class="waveform-empty">
    <i class="codicon codicon-graph-line"></i>
    <div>Run an experiment to capture the tuning response.</div>
  </div>
{:else}
  <div class="waveform-view">
    <ScopeEchartsView
      {channels}
      snapshot={result.snapshot}
      {verticalScale}
      {verticalOffset}
      activeChannelId={activeChannelId()}
      {viewRange}
      valueMultiplier={multiplierMap()}
      showLegend={true}
      onViewRangeChange={handleViewRangeChange}
      onDoubleClick={returnToLatest}
    />
  </div>
{/if}

<style>
  .waveform-empty {
    height: 100%;
    min-height: 300px;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 8px;
    color: var(--vscode-descriptionForeground);
    border: 1px dashed var(--vscode-panel-border);
    text-align: center;
  }

  .waveform-empty .codicon {
    font-size: 24px;
  }

  .waveform-view {
    min-width: 0;
    min-height: 260px;
    height: 100%;
  }
</style>
