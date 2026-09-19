export type ScopeRate = "fast" | "normal";

export type ScopeSelection = {
  id: number;
  rate: ScopeRate;
};

export type ScopeChannel = {
  id: number;
  label: string;
  unit?: string;
  rate: ScopeRate;
  sampleRateHz: number;
};

export type ScopeConfig = {
  historySeconds: number;
  channels: ScopeChannel[];
};

export type ScopeSeries = {
  id: number;
  sampleRateHz: number;
  times: number[];
  values: number[];
};

export type ScopeSnapshot = {
  sampleCount: number;
  lostFrames: number;
  state: string;
  series: ScopeSeries[];
};

export type ScopeSummary = {
  state: string;
  selectedChannels: number;
  lostFrames: number;
};
