import { writable } from "svelte/store";

type Range = [number, number];
type TuningViewState = {
  selectedId?: number;
  yRange?: Range;
  multipliers: Record<number, number>;
  timeRange?: Range;
  captureId?: number;
  hiddenChannelIds: number[];
};

const defaults = (): TuningViewState => ({ multipliers: {}, hiddenChannelIds: [] });

export function createTuningViewSettings() {
  // 页面切换会销毁波形组件，因此显示偏好由 App 持有，不能重新放回页面局部变量。
  // 这里只保存显示设置，不缓存实验数据或电机状态；返回页面仍从 Application 读取真实记录。
  const { subscribe, update, set } = writable<TuningViewState>(defaults());
  return {
    subscribe,
    patch: (changes: Partial<TuningViewState>) => update((state) => ({ ...state, ...changes })),
    // 设备重连后采集编号和通道含义可能变化，不能把旧设备的局部视图套到新会话。
    reset: () => set(defaults()),
  };
}

export type TuningViewSettings = ReturnType<typeof createTuningViewSettings>;
