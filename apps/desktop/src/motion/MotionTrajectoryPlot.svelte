<script lang="ts">
  import { onMount } from "svelte";
  import uPlot from "uplot";
  import type { MotionPreview } from "./types";

  export let preview: MotionPreview;

  let host: HTMLDivElement;
  let plot: uPlot | undefined;
  let resizeObserver: ResizeObserver | undefined;

  function alignedData(): uPlot.AlignedData {
    if (preview.secondary.length === preview.times.length && preview.secondary.length > 0) {
      return [preview.times, preview.primary, preview.secondary] as uPlot.AlignedData;
    }
    return [preview.times, preview.primary] as uPlot.AlignedData;
  }

  function series(): uPlot.Series[] {
    const result: uPlot.Series[] = [
      {},
      { label: preview.primaryLabel, stroke: "#8a9bb6", width: 2, scale: "primary" },
    ];
    if (preview.secondary.length === preview.times.length && preview.secondary.length > 0) {
      result.push({
        label: preview.secondaryLabel ?? "Secondary",
        stroke: "#c8b77a",
        width: 1.5,
        scale: "secondary",
      });
    }
    return result;
  }

  function axes(): uPlot.Axis[] {
    const result: uPlot.Axis[] = [
      {
        label: "Time (s)",
        stroke: "#777",
        grid: { stroke: "#2b2b2b", width: 1 },
        ticks: { stroke: "#444" },
        values: (_u, values) => values.map((value) => value.toFixed(2)),
      },
      {
        label: `${preview.primaryLabel} (${preview.primaryUnit})`,
        scale: "primary",
        stroke: "#8a9bb6",
        grid: { stroke: "#2b2b2b", width: 1 },
        ticks: { stroke: "#444" },
      },
    ];
    if (preview.secondary.length === preview.times.length && preview.secondary.length > 0) {
      result.push({
        side: 1,
        label: `${preview.secondaryLabel ?? "Secondary"} (${preview.secondaryUnit ?? ""})`,
        scale: "secondary",
        stroke: "#c8b77a",
        grid: { show: false },
        ticks: { stroke: "#444" },
      });
    }
    return result;
  }

  function rebuild() {
    if (!host || preview.times.length < 2) return;
    const rect = host.getBoundingClientRect();
    if (rect.width < 20 || rect.height < 20) return;

    plot?.destroy();
    plot = new uPlot({
      width: Math.max(320, Math.floor(rect.width)),
      height: Math.max(220, Math.floor(rect.height)),
      legend: { show: false },
      cursor: { show: false },
      select: { show: false },
      scales: {
        x: { time: false },
        primary: { auto: true },
        secondary: { auto: true },
      },
      axes: axes(),
      series: series(),
    }, alignedData(), host);
  }

  $: preview, rebuild();

  onMount(() => {
    rebuild();
    resizeObserver = new ResizeObserver(() => rebuild());
    resizeObserver.observe(host);
    return () => {
      resizeObserver?.disconnect();
      plot?.destroy();
    };
  });
</script>

<div class="motion-uplot-host" bind:this={host}></div>
