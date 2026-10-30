import type { TuningExperimentSnapshot } from "./tuningExperiment";

export function tuningWaveform(result: TuningExperimentSnapshot) {
  // Scope 的时间相对“最新样本”为负数；调参必须固定在本次采集起点，避免历史曲线不断左移。
  const snapshot = { ...result.snapshot, series: result.snapshot.series.map((series) => ({
    ...series, times: series.times.map((time) => time + result.recordedSeconds),
  })) };
  const origin = result.positionOriginTurns;
  if (origin == null || !Number.isFinite(origin)) {
    return { channels: result.config.channels, snapshot };
  }
  // 通道由当前 HostSchema 的 position 类型确定，不能靠名称、单位或固定 ID 猜测。
  const positionIds = new Set(result.positionChannelIds);
  const relative = (values: number[]) => values.map((value) => value - origin);
  // 只构造显示副本，原始采集数据继续保留累计位置，供分析或导出使用。
  return {
    channels: result.config.channels.map((channel) => positionIds.has(channel.id)
      ? { ...channel, label: `Δ ${channel.label}` } : channel),
    snapshot: {
      ...snapshot,
      series: snapshot.series.map((series) => positionIds.has(series.id) ? {
        ...series,
        // 先减去本次实验的实际起点，再由绘图层乘比例；否则几千转的累计值仍会掩盖几十转位移。
        // 参考与反馈共用这个零点，不能各减自身首点，否则会隐藏两者的跟踪误差。
        // 不能使用当前窗口首点：缩放、平移、刷新时它会变化，导致同一运动的位移跳变。
        values: relative(series.values),
        envelopeMin: series.envelopeMin ? relative(series.envelopeMin) : undefined,
        envelopeMax: series.envelopeMax ? relative(series.envelopeMax) : undefined,
      } : series),
    },
  };
}

export function tuningTimeRange(result: TuningExperimentSnapshot): [number, number] {
  const planned = result.plannedDurationSeconds;
  const initial = planned != null && Number.isFinite(planned) && planned > 0 ? planned : 3;
  const recorded = Math.max(0, result.recordedSeconds);
  // 预计终点之前保持全程横轴不变；若实际采集超时则分段扩展，不能截掉减速/停机数据。
  // 未知总时长的速度/转矩实验也从 0 开始扩展，不能伪造由参数决定的结束时刻。
  const step = Math.max(initial * 0.1, 0.5);
  const extension = Math.ceil(Math.max(0, recorded - initial) / step) * step;
  return [0, initial + extension];
}
