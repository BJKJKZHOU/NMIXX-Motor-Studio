<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import uPlot from "uplot";
  import type { ScopeChannel, ScopeSeries } from "../analysis/scope/types";

  export let title: string;
  export let unit: string;
  export let channels: ScopeChannel[];
  export let series: ScopeSeries[];

  const traceColors = ["#7aa2c8", "#c8b77a", "#9b8ac8", "#7fa68a", "#c28b73", "#aa829a"];

  let host: HTMLDivElement;
  let plot: uPlot | undefined;
  let resizeObserver: ResizeObserver | undefined;

  $: rebuildKey = channels.map((channel) => channel.id).join(",");
  $: if (host && rebuildKey) updatePlot();

  function alignedData(): uPlot.AlignedData {
    const first = series.find((entry) => entry.id === channels[0]?.id);
    if (!first) return [[], ...channels.map(() => [])] as uPlot.AlignedData;

    const count = first.times.length;
    const data: (number[] | null[])[] = [first.times];
    for (const channel of channels) {
      const entry = series.find((candidate) => candidate.id === channel.id);
      if (!entry || entry.values.length !== count) {
        data.push(new Array(count).fill(null));
      } else {
        data.push(entry.values);
      }
    }
    return data as uPlot.AlignedData;
  }

  function options(): uPlot.Options {
    return {
      width: Math.max(320, host?.clientWidth ?? 640),
      height: 170,
      scales: {
        x: { time: false },
        y: { auto: true },
      },
      axes: [
        { label: "s", stroke: "#858585", grid: { stroke: "#2d2d2d" } },
        { label: unit, stroke: "#858585", grid: { stroke: "#2d2d2d" } },
      ],
      cursor: { drag: { x: true, y: false } },
      legend: { show: true },
      series: [
        {},
        ...channels.map((channel, index) => ({
          label: channel.label,
          stroke: traceColors[index % traceColors.length],
          width: 1.25,
          scale: "y",
        })),
      ],
    };
  }

  function updatePlot() {
    const data = alignedData();
    const expectedSeries = channels.length + 1;
    if (!plot || plot.series.length !== expectedSeries) {
      plot?.destroy();
      plot = new uPlot(options(), data, host);
      return;
    }
    plot.setData(data);
  }

  onMount(() => {
    updatePlot();
    resizeObserver = new ResizeObserver(() => {
      if (plot && host.clientWidth > 0) {
        plot.setSize({ width: host.clientWidth, height: 170 });
      }
    });
    resizeObserver.observe(host);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    plot?.destroy();
  });
</script>

<section class="waveform-group">
  <div class="group-header">
    <strong>{title}</strong>
    <span>{channels[0]?.rate?.toUpperCase()} · {channels[0]?.sampleRateHz ?? 0} Hz</span>
  </div>
  <div class="plot-host" bind:this={host}></div>
</section>

<style>
  .waveform-group {
    min-width: 0;
    border-top: 1px solid color-mix(in srgb, var(--vscode-panel-border) 65%, transparent);
    padding-top: 8px;
  }

  .waveform-group:first-child {
    border-top: 0;
    padding-top: 0;
  }

  .group-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 5px;
    font-size: 11px;
  }

  .group-header strong {
    font-size: 12px;
  }

  .group-header span {
    color: var(--vscode-descriptionForeground);
  }

  .plot-host {
    width: 100%;
    min-height: 170px;
  }
</style>
