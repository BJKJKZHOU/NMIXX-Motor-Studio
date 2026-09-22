<script lang="ts">
  import { onMount } from "svelte";
  import { LineChart } from "echarts/charts";
  import { GridComponent } from "echarts/components";
  import { init, use, type ECharts } from "echarts/core";
  import { CanvasRenderer } from "echarts/renderers";
  import type { EChartsOption } from "echarts";
  import type { MotionPreview } from "./types";

  export let preview: MotionPreview;

  use([LineChart, GridComponent, CanvasRenderer]);

  let host: HTMLDivElement;
  let plot: ECharts | undefined;
  let resizeObserver: ResizeObserver | undefined;

  function makeOption(next: MotionPreview): EChartsOption {
    const secondary = next.secondary.length > 0 && next.secondary.length === next.times.length;
    const yAxis: NonNullable<EChartsOption["yAxis"]> = [
      {
        type: "value",
        name: `${next.primaryLabel} (${next.primaryUnit})`,
        nameLocation: "middle",
        nameGap: 48,
        position: "left",
        scale: true,
        axisLine: { show: true, lineStyle: { color: "#8a9bb6" } },
        axisLabel: { color: "#8a9bb6" },
        splitLine: { lineStyle: { color: "#2b2b2b" } },
      },
    ];
    const series: NonNullable<EChartsOption["series"]> = [
      {
        name: next.primaryLabel,
        type: "line",
        showSymbol: false,
        silent: true,
        yAxisIndex: 0,
        lineStyle: { color: "#8a9bb6", width: 2 },
        data: next.times.map((time, index) => [time, next.primary[index] ?? null]),
      },
    ];
    if (secondary) {
      yAxis.push({
        type: "value",
        name: `${next.secondaryLabel ?? "Secondary"} (${next.secondaryUnit ?? ""})`,
        nameLocation: "middle",
        nameGap: 48,
        position: "right",
        scale: true,
        axisLine: { show: true, lineStyle: { color: "#c8b77a" } },
        axisLabel: { color: "#c8b77a" },
        splitLine: { show: false },
      });
      series.push({
        name: next.secondaryLabel ?? "Secondary",
        type: "line",
        showSymbol: false,
        silent: true,
        yAxisIndex: 1,
        lineStyle: { color: "#c8b77a", width: 1.5 },
        data: next.times.map((time, index) => [time, next.secondary[index] ?? null]),
      });
    }
    return {
      animation: false,
      backgroundColor: "transparent",
      grid: { left: 76, right: secondary ? 76 : 24, top: 20, bottom: 56 },
      xAxis: {
        type: "value",
        name: "Time (s)",
        nameLocation: "middle",
        nameGap: 30,
        min: next.times[0] ?? 0,
        max: next.times[next.times.length - 1] ?? 1,
        axisLine: { show: true, lineStyle: { color: "#777" } },
        axisLabel: { color: "#777", formatter: (value: number) => value.toFixed(2) },
        splitLine: { lineStyle: { color: "#2b2b2b" } },
      },
      yAxis,
      series,
    };
  }

  function updatePlot(next: MotionPreview) {
    if (!plot) return;
    plot.setOption(makeOption(next), { notMerge: true, lazyUpdate: false });
  }

  function ensurePlot() {
    if (!host) return;
    const rect = host.getBoundingClientRect();
    if (rect.width < 20 || rect.height < 20) return;
    if (!plot) {
      // Reuse the existing chart library. Do not evaluate uPlot's module-level
      // Intl formatter against WebKit's POSIX "C" language, or patch global Intl.
      plot = init(host, undefined, { renderer: "canvas", locale: "EN" });
    }
    plot.resize();
    updatePlot(preview);
  }

  $: if (host) updatePlot(preview);

  onMount(() => {
    ensurePlot();
    resizeObserver = new ResizeObserver(ensurePlot);
    resizeObserver.observe(host);
    return () => {
      resizeObserver?.disconnect();
      plot?.dispose();
    };
  });
</script>

<div class="motion-uplot-host" bind:this={host}></div>
