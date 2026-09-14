export type PlotChannel = {
  id: number;
  symbol: string;
  unit?: string;
  supportsFast: boolean;
  supportsNormal: boolean;
  fastScale?: number;
};

export type ConnectionInfo = {
  port: string;
  fastMaxChannels: number;
  normalMaxChannels: number;
  fastBlockSamples: number;
  fastRateHz: number;
  normalRateHz: number;
  channels: PlotChannel[];
};
