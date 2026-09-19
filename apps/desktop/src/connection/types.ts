export type MotionCapabilities = {
  position: boolean;
  speed: boolean;
  sensorlessSpeed: boolean;
  torque: boolean;
  mit: boolean;

  trajectoryTrapezoidal: boolean;
  trajectorySCurve: boolean;
  trajectoryFiltered: boolean;

  maxSpeed: boolean;
  acceleration: boolean;
  deceleration: boolean;
  filterTime: boolean;

  run: boolean;
  stop: boolean;
  enable: boolean;
  disable: boolean;

  positionTarget: boolean;
  speedTarget: boolean;
  torqueTarget: boolean;
  torqueRamp: boolean;
  sensorlessStartupCurrent: boolean;
  sensorlessEntrySpeed: boolean;
};

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
  motion: MotionCapabilities;
};
