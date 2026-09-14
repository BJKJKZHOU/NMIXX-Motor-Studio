export type ScopeChannel = { id: number; symbol: string; unit?: string };

export type ScopeConfig = {
  sampleRateHz: number;
  historySeconds: number;
  channels: ScopeChannel[];
};

export type ScopeSnapshot = {
  sampleRateHz: number;
  sampleCount: number;
  lostFrames: number;
  state: string;
  times: number[];
  series: number[][];
};

export type ScopeSummary = {
  state: string;
  selectedChannels: number;
  lostFrames: number;
};
