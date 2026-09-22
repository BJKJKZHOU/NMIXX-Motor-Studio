<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { LineChart } from "echarts/charts";
  import { DataZoomComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
  import { init, use, type ECharts, type EChartsOption } from "echarts/core";
  import { CanvasRenderer } from "echarts/renderers";
  import type { ScopeSnapshot } from "./types";

  type ScopeDisplayChannel = {
    id: number;
    label: string;
    unit?: string;
  };

  export let channels: ScopeDisplayChannel[] = [];
  export let snapshot: ScopeSnapshot | undefined;
  export let verticalScale = new Map<number, number>();
  export let verticalOffset = new Map<number, number>();
  export let activeChannelId: number | undefined;
  export let viewRange: [number, number] = [-0.5, 0];
  export let valueMultiplier = new Map<number, number>();
  export let showLegend = true;
  export let onViewRangeChange: (range: [number, number]) => void = () => undefined;
  export let onDoubleClick: () => void = () => undefined;

  use([LineChart, DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

  const verticalDivisions = 8;
  const traceColors = [
    "#5470c6", "#b6d72c", "#586080", "#ff9845", "#73c0de", "#3ba272",
    "#fc8452", "#9a60b4", "#ea7ccc", "#91cc75", "#fac858", "#ee6666",
  ];

  let host: HTMLDivElement;
  let chart: ECharts | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let applyingOption = false;

  $: if (host) {
    channels;
    snapshot;
    verticalScale;
    verticalOffset;
    activeChannelId;
    viewRange;
    valueMultiplier;
    showLegend;
    updateChart();
  }

  function multiplier(id: number): number {
    const value = valueMultiplier.get(id);
    return Number.isFinite(value) && (value ?? 0) > 0 ? value! : 1;
  }

  function defaultScale(channel: ScopeDisplayChannel): number {
    const unit = channel.unit?.trim().toLowerCase() ?? "";
    if (unit === "a") return 0.5;
    if (unit === "v") return 5;
    if (unit.includes("rad/s")) return 10;
    if (unit.includes("turn")) return 0.5;
    return 1;
  }

  function traceColor(channel: ScopeDisplayChannel): string {
    const index = Math.max(0, channels.findIndex((item) => item.id === channel.id));
    return traceColors[index % traceColors.length];
  }

  function availableSeconds(): number {
    return Math.max(snapshot?.recordedSeconds ?? 0.5, 0.0005);
  }

  function makeOption(): EChartsOption {
    const visibleAxisChannel = channels.find((channel) => channel.id === activeChannelId) ?? channels[0];
    const axes = channels.map((channel) => {
      const gain = multiplier(channel.id);
      const perDiv = (verticalScale.get(channel.id) ?? defaultScale(channel)) * gain;
      const center = (verticalOffset.get(channel.id) ?? 0) * gain;
      const half = perDiv * verticalDivisions / 2;
      const visible = channel.id === visibleAxisChannel?.id;
      const color = traceColor(channel);
      return {
        type: "value" as const,
        name: visible ? `${channel.label}${channel.unit ? ` (${channel.unit})` : ""}` : "",
        show: visible,
        position: "left" as const,
        min: center - half,
        max: center + half,
        interval: perDiv,
        axisLine: { show: visible, lineStyle: { color } },
        axisTick: { show: visible },
        axisLabel: { show: visible, color },
        nameTextStyle: { color },
        splitLine: { show: visible, lineStyle: { color: "#303030" } },
      };
    });

    const series = channels.map((channel, index) => {
      const source = snapshot?.series.find((item) => item.id === channel.id);
      const gain = multiplier(channel.id);
      const color = traceColor(channel);
      return {
        name: channel.label,
        type: "line" as const,
        yAxisIndex: index,
        showSymbol: false,
        symbol: "none",
        sampling: "none",
        animation: false,
        lineStyle: { color, width: channel.id === visibleAxisChannel?.id ? 1.5 : 1.1 },
        itemStyle: { color },
        data: (source?.times ?? []).map((time, pointIndex) => [
          time,
          (source?.values[pointIndex] ?? 0) * gain,
        ]),
      };
    });

    return {
      animation: false,
      backgroundColor: "transparent",
      grid: { left: 76, right: 36, top: showLegend ? 42 : 18, bottom: 42, containLabel: false },
      legend: showLegend ? { top: 4, right: 14, textStyle: { color: "#bdbdbd" } } : { show: false },
      tooltip: {
        trigger: "axis",
        axisPointer: { type: "cross" },
        backgroundColor: "rgba(30,30,30,0.94)",
        borderColor: "#4a4a4a",
        textStyle: { color: "#d0d0d0" },
      },
      xAxis: {
        type: "value",
        min: -availableSeconds(),
        max: 0,
        axisLine: { lineStyle: { color: "#6e6e6e" } },
        axisLabel: { color: "#9a9a9a", formatter: (value: number) => `${value.toFixed(3)} s` },
        splitLine: { lineStyle: { color: "#303030" } },
      },
      yAxis: axes.length > 0 ? axes : [{ type: "value" as const }],
      dataZoom: [{
        type: "inside",
        xAxisIndex: 0,
        zoomOnMouseWheel: true,
        moveOnMouseMove: true,
        moveOnMouseWheel: false,
        preventDefaultMouseMove: true,
        filterMode: "none",
        startValue: viewRange[0],
        endValue: viewRange[1],
      }],
      series,
    };
  }

  function updateChart() {
    if (!chart) return;
    applyingOption = true;
    try {
      chart.setOption(makeOption(), {
        notMerge: false,
        replaceMerge: ["series", "yAxis"],
        lazyUpdate: false,
      });
    } finally {
      applyingOption = false;
    }
  }

  function zoomRange(event: any): [number, number] | undefined {
    const payload = event?.batch?.[0] ?? event ?? {};
    const option = chart?.getOption() as any;
    const zoom = option?.dataZoom?.[0] ?? {};
    const min = -availableSeconds();
    const width = -min;
    const startValue = Number(payload.startValue ?? zoom.startValue);
    const endValue = Number(payload.endValue ?? zoom.endValue);
    if (Number.isFinite(startValue) && Number.isFinite(endValue)) {
      return [Math.min(startValue, endValue), Math.max(startValue, endValue)];
    }
    const start = Number(payload.start ?? zoom.start);
    const end = Number(payload.end ?? zoom.end);
    if (Number.isFinite(start) && Number.isFinite(end)) {
      return [min + width * start / 100, min + width * end / 100];
    }
    return undefined;
  }

  function ensureChart() {
    if (!host) return;
    const rect = host.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return;
    if (!chart) {
      chart = init(host, undefined, { renderer: "canvas" });
      chart.on("datazoom", (event: unknown) => {
        if (applyingOption) return;
        const range = zoomRange(event);
        if (range) onViewRangeChange(range);
      });
      chart.getZr().on("dblclick", onDoubleClick);
    }
    chart.resize();
    updateChart();
  }

  onMount(() => {
    resizeObserver = new ResizeObserver(ensureChart);
    resizeObserver.observe(host);
    ensureChart();
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    chart?.dispose();
  });
</script>

<div bind:this={host} class="scope-echarts-view"></div>

<style>
  .scope-echarts-view {
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    background: #1e1e1e;
  }
</style>
