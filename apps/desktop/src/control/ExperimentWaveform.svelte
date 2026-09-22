<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import uPlot from "uplot";
  import type { TuningExperimentSnapshot } from "./tuningExperiment";

  export let result: TuningExperimentSnapshot | undefined;
  export let multipliers: Record<number, number> = {};
  export let timePerDiv = 20e-3;
  export let onViewRequest: (windowSeconds: number, endOffsetSeconds: number, maxPoints: number) => void = () => undefined;

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
  let panOffset = 0;
  let dragStart: { x: number; offset: number } | undefined;
  let viewRequestTimer: ReturnType<typeof setTimeout> | undefined;

  $: channels = result?.config.channels ?? [];
  $: series = result?.snapshot.series ?? [];
  $: rebuildKey = channels.map((channel) => channel.id).join(",");
  $: if (host && rebuildKey && result) {
    series;
    multipliers;
    updatePlot();
  }
  $: if (plot && result) {
    timePerDiv;
    applyRanges();
  }

  function multiplier(id: number): number {
    const value = multipliers[id];
    return Number.isFinite(value) && value > 0 ? value : 1;
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
        if (target !== undefined) output[target] = source.values[index] * multiplier(channel.id);
      }
      return output;
    });
    return [times, ...values] as uPlot.AlignedData;
  }

  function fullTimeRange(): { min: number; max: number } {
    const recorded = result?.recordedSeconds ?? 0;
    return recorded > 0
      ? { min: -recorded, max: 0 }
      : { min: -timePerDiv * horizontalDivisions, max: 0 };
  }

  function currentWindowSeconds(): number {
    return Math.max(0.0005, timePerDiv * horizontalDivisions);
  }

  function scheduleViewRequest(delay = 80) {
    if (viewRequestTimer) clearTimeout(viewRequestTimer);
    viewRequestTimer = setTimeout(() => {
      viewRequestTimer = undefined;
      const width = Math.max(320, Math.floor(host?.clientWidth ?? 1000));
      onViewRequest(currentWindowSeconds(), panOffset, Math.min(20_000, Math.max(1000, width * 3)));
    }, delay);
  }

  function applyRanges() {
    if (!plot) return;

    let yMin = Number.POSITIVE_INFINITY;
    let yMax = Number.NEGATIVE_INFINITY;
    for (const channel of channels) {
      const source = series.find((entry) => entry.id === channel.id);
      if (!source) continue;
      const scale = multiplier(channel.id);
      const mins = source.envelopeMin ?? source.values;
      const maxs = source.envelopeMax ?? source.values;
      for (const value of mins) {
        if (Number.isFinite(value)) yMin = Math.min(yMin, value * scale);
      }
      for (const value of maxs) {
        if (Number.isFinite(value)) yMax = Math.max(yMax, value * scale);
      }
    }
    if (Number.isFinite(yMin) && Number.isFinite(yMax)) {
      const span = Math.max(yMax - yMin, Math.max(Math.abs(yMin), Math.abs(yMax)) * 0.02, 1e-9);
      const pad = span * 0.05;
      plot.setScale("y", { min: yMin - pad, max: yMax + pad });
    }

    const full = fullTimeRange();
    const span = Math.min(currentWindowSeconds(), Math.max(full.max - full.min, 1e-9));
    const maxOffset = Math.max(0, full.max - full.min - span);
    panOffset = Math.min(Math.max(result?.endOffsetSeconds ?? panOffset, 0), maxOffset);
    plot.setScale("x", { min: -panOffset - span, max: -panOffset });
  }

  function envelopePlugin(): uPlot.Plugin {
    return {
      hooks: {
        draw: [
          (u) => {
            const ctx = u.ctx;
            const px = window.devicePixelRatio || 1;
            ctx.save();
            ctx.lineWidth = Math.max(1, px * 0.75);
            for (let channelIndex = 0; channelIndex < channels.length; channelIndex += 1) {
              const channel = channels[channelIndex];
              const source = series.find((entry) => entry.id === channel.id);
              if (!source?.envelopeMin || !source.envelopeMax) continue;
              if (source.envelopeMin.length !== source.times.length || source.envelopeMax.length !== source.times.length) continue;

              ctx.strokeStyle = traceColors[channelIndex % traceColors.length];
              ctx.globalAlpha = 0.42;
              const scale = multiplier(channel.id);
              for (let index = 0; index < source.times.length; index += 1) {
                const x = u.valToPos(source.times[index], "x", true);
                const yMin = u.valToPos(source.envelopeMin[index] * scale, "y", true);
                const yMax = u.valToPos(source.envelopeMax[index] * scale, "y", true);
                if (!Number.isFinite(x) || !Number.isFinite(yMin) || !Number.isFinite(yMax)) continue;
                ctx.beginPath();
                ctx.moveTo(x, yMin);
                ctx.lineTo(x, yMax);
                ctx.stroke();
              }
            }
            ctx.restore();
          },
        ],
      },
    };
  }

  function options(): uPlot.Options {
    const scales: Record<string, uPlot.Scale> = {
      x: { time: false, auto: false },
      y: { auto: false },
    };

    return {
      width: Math.max(320, host?.clientWidth ?? 640),
      height: Math.max(260, host?.clientHeight ?? 300),
      legend: { show: false },
      cursor: { drag: { x: false, y: false } },
      scales,
      axes: [
        { label: "s", stroke: "#858585", grid: { stroke: "#2d2d2d" } },
      ],
      plugins: [envelopePlugin()],
      series: [
        {},
        ...channels.map((channel, index) => ({
          label: channel.label,
          stroke: traceColors[index % traceColors.length],
          width: 1.25,
          scale: "y",
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
    scheduleViewRequest();
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
    const span = Math.min(currentWindowSeconds(), Math.max(full.max - full.min, 1e-9));
    const rect = plot.over.getBoundingClientRect();
    const secondsPerPixel = span / Math.max(rect.width, 1);
    const delta = (event.clientX - dragStart.x) * secondsPerPixel;
    const maxOffset = Math.max(0, full.max - full.min - span);
    panOffset = Math.min(Math.max(dragStart.offset + delta, 0), maxOffset);
    plot.setScale("x", { min: -panOffset - span, max: -panOffset });
  }

  function finishPointer(event: PointerEvent) {
    if (!plot || !dragStart) return;
    dragStart = undefined;
    if (plot.over.hasPointerCapture(event.pointerId)) plot.over.releasePointerCapture(event.pointerId);
    plot.over.style.cursor = "grab";
    scheduleViewRequest(0);
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
    if (viewRequestTimer) clearTimeout(viewRequestTimer);
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
