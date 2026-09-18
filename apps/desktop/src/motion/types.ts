export type MotionMode = "position" | "speed" | "sensorless-speed" | "torque" | "mit";
export type TrajectoryType = "trapezoidal" | "s-curve" | "filtered";

export interface MotionState {
  mode: MotionMode;
  trajectory: TrajectoryType;
  acceleration: number;
  deceleration: number;
  filterTimeMs: number;
  repeat: boolean;

  positionCommand: "absolute" | "incremental";
  positionTargetTurn: number;
  positionMaxSpeed: number;

  speedTarget: number;
  sensorlessSpeedTarget: number;
  sensorlessStartupCurrent: number;
  sensorlessEntrySpeed: number;

  torqueTargetNm: number;
  torqueRampNmPerS: number;

  mitPositionRef: number;
  mitVelocityRef: number;
  mitKp: number;
  mitKd: number;
  mitTorqueFeedforward: number;
}
