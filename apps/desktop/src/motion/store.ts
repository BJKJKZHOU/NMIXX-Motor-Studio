import { writable } from "svelte/store";
import type { MotionState } from "./types";

export const motionState = writable<MotionState>({
  mode: "position",
  trajectory: "trapezoidal",
  acceleration: 20,
  deceleration: 20,
  filterTimeMs: 20,
  repeat: false,

  positionCommand: "incremental",
  positionTargetTurn: 1,
  positionMaxSpeed: 8,

  speedTarget: 20,
  sensorlessSpeedTarget: 20,
  sensorlessStartupCurrent: 1,
  sensorlessEntrySpeed: 8,

  torqueTargetNm: 0.2,
  torqueRampNmPerS: 1,

  mitPositionRef: 0,
  mitVelocityRef: 0,
  mitKp: 10,
  mitKd: 0.5,
  mitTorqueFeedforward: 0,
});
