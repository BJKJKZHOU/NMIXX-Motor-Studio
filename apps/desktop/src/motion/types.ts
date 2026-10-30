export type MotionMode = "position" | "speed" | "sensorless-speed" | "torque";

export interface MotionState {
  positionCommand: "absolute" | "incremental";
  incrementalDeltaTurn: number;
  repeat: boolean;
}

export interface MotionPreview {
  times: number[];
  primary: number[];
  secondary: number[];
  primaryLabel: string;
  secondaryLabel?: string | null;
  primaryUnit: string;
  secondaryUnit?: string | null;
}

export type MotionRunResult =
  | { kind: "runAccepted"; handle: { txn: number; actionId: number } }
  | { kind: "targetUpdated" };
