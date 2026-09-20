<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import uPlot from "uplot";
  import type { ScopeChannel, ScopeSeries } from "../analysis/scope/types";
  import type { TuningExperimentSnapshot } from "./tuningExperiment";

  export let result: TuningExperimentSnapshot | undefined;
  export let multipliers: Record<number, number> = {};
  export let timePerDiv = 20e-3;

  const horizontalDivisions = 10;
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

  let host: HTMLDivElement;
  let plot: uPlot | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let baseRanges = new Map<number, number>();
  let panOffset = 0;
  let dragStart: { x: number; offset: number } | undefined;

  $: channels = result?.config.channels ?? [];
  $: series = result?.snapshot.series ?? [];
  $: rebuildKey = channels.map((channel) => channel.id).join(",");
  $: if (host && rebuildKey && result) {
    series;
    updatePlot();
  }
  $: if (plot && result) {
    multipliers;
    timePerDiv;
    applyRanges();
  }

  function multiplier(id: number): number {
    const value = multipliers[id];
    return Number.isFinite(value) && value > 0 ? value : 1;
  }

  function yScaleKey(id: number): string {
    return `y-${id}`;
  }

  function ensureBaseRange(channel: ScopeChannel, source: ScopeSeries | undefined): number {
    let peak = 0;
    if (source) {
      for (const value of source.values) {
        if (Number.isFinite(value)) peak = Math.max(peak, Math.abs(value));
      }
    }
    const fallback = channel.unit === "A" ? 1 : channel.unit === "rad/s" ? 10 : 1;
    const candidate = Math.max(peak * 1.1, fallback * 0.1, 1e-6);
    const existing = baseRanges.get(channel.id);
    const range = existing === undefined ? candidate : Math.max(existing, candidate);
    if (existing !== range) baseRanges.set(channel.id, range);
    return range;
  }

  function alignedData(): uPlot.AlignedData {
    const timeKeys = new Map<string, number>();
    for (const entry of series) {
      for (const time of entry.times) timeKeys.set(time.toFixed(7), time);
    }
    const times = Array.from(timeKeys.values()).sort((a, b) => a - b);
    const indexByKey = new Map(times.map((time, index) => [time.toFixed(7), index]));

    const values = channels.map((channel) => {
      const output: Array<number | null> = times.map(() => null);
      const source = series.find((entry) => entry.id === channel.id);
      if (!source) return output;
      for (let index = 0; index < source.times.length; index += 1) {
        const target = indexByKey.get(source.times[index].toFixed(7));
        if (target !== undefined) output[target] = source.values[index];
      }
      return output;
    });
    return [times, ...values] as uPlot.AlignedData;
  }

  function fullTimeRange(): { min: number; max: number } {
    let min = 0;
    let max = 0;
    let initialized = false;
    for (const entry of series) {
      if (entry.times.length === 0) continue;
      const first = entry.times[0];
      const last = entry.times[entry.times.length - 1];
      if (!initialized) {
        min = first;
        max = last;
        initialized = true;
      } else {
        min = Math.min(min, first);
        max = Math.max(max, last);
      }
    }
    return initialized ? { min, max } : { min: -timePerDiv * horizontalDivisions, max: 0 };
  }

  function applyRanges() {
    if (!plot) return;

    for (const channel of channels) {
      const source = series.find((entry) => entry.id === channel.id);
      const base = ensureBaseRange(channel, source);
      const half = base / multiplier(channel.id);
      plot.setScale(yScaleKey(channel.id), { min: -half, max: half });
    }

    const full = fullTimeRange();
    const span = Math.min(timePerDiv * horizontalDivisions, Math.max(full.max - full.min, 1e-9));
    const maxOffset = Math.max(0, full.max - full.min - span);
    panOffset = Math.min(Math.max(panOffset, 0), maxOffset);
    plot.setScale("x", { min: full.max - panOffset - span, max: full.max - panOffset });
  }

  function options(): uPlot.Options {
    const scales: Record<string, uPlot.Scale> = {
      x: { time: false, auto: false },
    };
    for (const channel of channels) {
      const source = series.find((entry) => entry.id === channel.id);
      const base = ensureBaseRange(channel, source);
      const half = base / multiplier(channel.id);
      scales[yScaleKey(channel.id)] = { auto: false, range: [-half, half] };
    }

    return {
      width: Math.max(320, host?.clientWidth ?? 640),
      height: Math.max(260, host?.clientHeight ?? 300),
      legend: { show: false },
      cursor: { drag: { x: false, y: false } },
      scales,
      axes: [
        { label: "s", stroke: "#858585", grid: { stroke: "#2d2d2d" } },
      ],
      series: [
        {},
        ...channels.map((channel, index) => ({
          label: channel.label,
          stroke: traceColors[index % traceColors.length],
          width: 1.25,
          scale: yScaleKey(channel.id),
          spanGaps: true,
        })),
      ],
    };
  }

  function updatePlot() {
    if (!host || !result) return;
    const data = alignedData();
    const expectedSeries = channels.length + 1;
    if (!plot || plot.series.length !== expectedSeries) {
      unbindInteractions();
      plot?.destroy();
      plot = new uPlot(options(), data, host);
      bindInteractions();
    } else {
      plot.setData(data);
    }
    applyRanges();
  }

  function handleWheel(event: WheelEvent) {
    if (!plot || event.deltaY === 0) return;
    event.preventDefault();
    const current = timeDivOptions.findIndex((value) => value === timePerDiv);
    const index = current >= 0 ? current : timeDivOptions.findIndex((value) => value >= timePerDiv);
    const nextIndex = Math.min(
      timeDivOptions.length - 1,
      Math.max(0, (index >= 0 ? index : timeDivOptions.length - 1) + (event.deltaY > 0 ? 1 : -1)),
    );
    timePerDiv = timeDivOptions[nextIndex];
    applyRanges();
  }

  function handlePointerDown(event: PointerEvent) {
    if (!plot || event.button !== 0) return;
    dragStart = { x: event.clientX, offset: panOffset };
    plot.over.setPointerCapture(event.pointerId);
    plot.over.style.cursor = "grabbing";
  }

  function handlePointerMove(event: PointerEvent) {
    if (!plot || !dragStart) return;
    const full = fullTimeRange();
    const span = Math.min(timePerDiv * horizontalDivisions, Math.max(full.max - full.min, 1e-9));
    const rect = plot.over.getBoundingClientRect();
    const secondsPerPixel = span / Math.max(rect.width, 1);
    const delta = (event.clientX - dragStart.x) * secondsPerPixel;
    const maxOffset = Math.max(0, full.max - full.min - span);
    panOffset = Math.min(Math.max(dragStart.offset + delta, 0), maxOffset);
    applyRanges();
  }

  function finishPointer(event: PointerEvent) {
    if (!plot || !dragStart) return;
    dragStart = undefined;
    if (plot.over.hasPointerCapture(event.pointerId)) plot.over.releasePointerCapture(event.pointerId);
    plot.over.style.cursor = "grab";
  }

  function bindInteractions() {
    if (!plot) return;
    plot.over.style.cursor = "grab";
    plot.over.addEventListener("wheel", handleWheel, { passive: false });
    plot.over.addEventListener("pointerdown", handlePointerDown);
    plot.over.addEventListener("pointermove", handlePointerMove);
    plot.over.addEventListener("pointerup", finishPointer);
    plot.over.addEventListener("pointercancel", finishPointer);
  }

  function unbindInteractions() {
    if (!plot) return;
    plot.over.removeEventListener("wheel", handleWheel);
    plot.over.removeEventListener("pointerdown", handlePointerDown);
    plot.over.removeEventListener("pointermove", handlePointerMove);
    plot.over.removeEventListener("pointerup", finishPointer);
    plot.over.removeEventListener("pointercancel", finishPointer);
  }

  onMount(() => {
    if (result) updatePlot();
    resizeObserver = new ResizeObserver(() => {
      if (plot && host.clientWidth > 0 && host.clientHeight > 0) {
        plot.setSize({
          width: host.clientWidth,
          height: Math.max(260, host.clientHeight),
        });
      }
    });
    resizeObserver.observe(host);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    unbindInteractions();
    plot?.destroy();
  });
</script>

{#if !result}
  <div class="waveform-empty">
    <i class="codicon codicon-graph-line"></i>
    <div>Run an experiment to capture the tuning response.</div>
  </div>
{:else}
  <div class="waveform-view">
    <div class="waveform-legend">
      {#each channels as channel, index (channel.id)}
        <span>
          <i style:background={traceColors[index % traceColors.length]}></i>
          {channel.label}
          {#if multiplier(channel.id) !== 1}<em>×{multiplier(channel.id)}</em>{/if}
        </span>
      {/each}
    </div>
    <div class="plot-host" bind:this={host}></div>
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
    min-height: 0;
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
  }

  .waveform-legend {
    display: flex;
    flex-wrap: wrap;
    gap: 5px 14px;
    min-height: 24px;
    align-items: center;
    color: var(--vscode-descriptionForeground);
    font-size: 10px;
  }

  .waveform-legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .waveform-legend i {
    width: 13px;
    height: 2px;
    display: inline-block;
  }

  .waveform-legend em {
    font-style: normal;
    color: var(--vscode-foreground);
  }

  .plot-host {
    min-width: 0;
    min-height: 260px;
    height: 100%;
  }
</style>
