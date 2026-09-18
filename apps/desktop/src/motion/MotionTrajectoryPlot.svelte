<script lang="ts">
  import { onMount } from "svelte";
  import uPlot from "uplot";
  import type { SCurveMode, TrajectoryType } from "./types";

  export let trajectory: TrajectoryType;
  export let sCurveMode: SCurveMode;
  export let acceleration: number;
  export let deceleration: number;
  export let filterTimeMs: number;

  let host: HTMLDivElement;
  let plot: uPlot | undefined;
  let resizeObserver: ResizeObserver | undefined;

  const points = 241;

  function smoothstep(x: number): number {
    return x * x * (3 - 2 * x);
  }

  function profileValue(t: number): number {
    if (trajectory === "trapezoidal") {
      if (t < 0.25) return t / 0.25;
      if (t <= 0.75) return 1;
      return Math.max(0, (1 - t) / 0.25);
    }

    if (trajectory === "s-curve") {
      const edge = sCurveMode === "peak-accel" ? 0.34 : 0.25;
      if (t < edge) return smoothstep(t / edge);
      if (t <= 1 - edge) return 1;
      return smoothstep((1 - t) / edge);
    }

    const filter = Math.min(0.18, Math.max(0.035, filterTimeMs / 1000 / 2));
    const edge = Math.min(0.4, 0.25 + filter);
    if (t < edge) return smoothstep(t / edge);
    if (t <= 1 - edge) return 1;
    return smoothstep((1 - t) / edge);
  }

  function data(): uPlot.AlignedData {
    const time: number[] = [];
    const command: number[] = [];
    const accelScale = Math.max(Math.abs(acceleration), Math.abs(deceleration), 0.001);

    for (let index = 0; index < points; index += 1) {
      const t = index / (points - 1);
      time.push(t);
      command.push(profileValue(t));
    }

    // Keep the preview normalized, but let parameter changes remain part of the
    // generated model so this component can later accept the Application
    // Motion preview without changing the plotting layer.
    void accelScale;
    return [time, command] as uPlot.AlignedData;
  }

  function rebuild() {
    if (!host) return;
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
        x: { time: false, range: [0, 1] },
        y: { range: [-0.06, 1.12] },
      },
      axes: [
        {
          label: "Time",
          stroke: "#777",
          grid: { stroke: "#2b2b2b", width: 1 },
          ticks: { stroke: "#444" },
          values: (_u, vals) => vals.map((value) => value.toFixed(1)),
        },
        {
          label: "Command",
          stroke: "#777",
          grid: { stroke: "#2b2b2b", width: 1 },
          ticks: { stroke: "#444" },
          values: (_u, vals) => vals.map((value) => value.toFixed(1)),
        },
      ],
      series: [
        {},
        { label: "Command", stroke: "#8a9bb6", width: 2 },
      ],
    }, data(), host);
  }

  function refresh() {
    if (!plot) {
      rebuild();
      return;
    }
    plot.setData(data());
  }

  $: trajectory, sCurveMode, acceleration, deceleration, filterTimeMs, refresh();

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
