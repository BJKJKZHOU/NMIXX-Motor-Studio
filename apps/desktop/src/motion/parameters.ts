import type { ParameterMetadata, ParameterValue, PositionValue } from "../parameters/types";
import type { MotionMode } from "./types";

export const MOTION_MODE = "PARAM_MOTOR_MODE";
export const MOTION_MAX_SPEED = "PARAM_MOTION_WM_MAX";
export const MOTION_ACCEL = "PARAM_MOTION_WM_ACC";
export const MOTION_DECEL = "PARAM_MOTION_WM_DEC";
export const TARGET_POSITION = "PARAM_TARGET_POSITION";
export const TARGET_SPEED = "PARAM_TARGET_SPEED";
export const TARGET_TORQUE = "PARAM_TARGET_TORQUE";
export const TORQUE_RAMP = "PARAM_MOTION_TORQUE_RAMP";
export const SENSORLESS_STARTUP_CURRENT = "PARAM_SENSORLESS_STARTUP_CURRENT";
export const SENSORLESS_ENTRY_SPEED = "PARAM_SENSORLESS_ENTRY_SPEED";

export const MOTION_PARAMETER_SYMBOLS = [
  MOTION_MODE,
  MOTION_MAX_SPEED,
  MOTION_ACCEL,
  MOTION_DECEL,
  TARGET_POSITION,
  TARGET_SPEED,
  TARGET_TORQUE,
  TORQUE_RAMP,
  SENSORLESS_STARTUP_CURRENT,
  SENSORLESS_ENTRY_SPEED,
] as const;

const MODE_SYMBOL: Record<MotionMode, string> = {
  position: "POSITION",
  speed: "SPEED",
  "sensorless-speed": "SENSORLESS_SPEED",
  torque: "TORQUE",
};

export function modeFromParameter(
  meta: ParameterMetadata | undefined,
  value: ParameterValue | null | undefined,
): MotionMode | undefined {
  if (!meta || !value || value.type !== "u8") return undefined;
  const numeric = Number(value.value);
  for (const [mode, symbol] of Object.entries(MODE_SYMBOL) as Array<[MotionMode, string]>) {
    const index = meta.allowedSymbols.indexOf(symbol);
    if (index < 0) continue;
    const allowed = meta.allowed[index] ?? index;
    if (Number(allowed) === numeric) return mode;
  }
  return undefined;
}

export function modeParameterValue(
  meta: ParameterMetadata | undefined,
  mode: MotionMode,
): number | undefined {
  if (!meta) return undefined;
  const symbol = MODE_SYMBOL[mode];
  const index = meta.allowedSymbols.indexOf(symbol);
  if (index < 0) return undefined;
  return Number(meta.allowed[index] ?? index);
}

export function positionToTurns(value: PositionValue): number {
  return Number(value.turns) + Number(value.theta) / (2 * Math.PI);
}

export function turnsToPosition(turns: number): PositionValue {
  if (!Number.isFinite(turns)) throw new Error("Position must be finite.");
  const whole = Math.floor(turns);
  return {
    turns: whole,
    theta: (turns - whole) * 2 * Math.PI,
  };
}

export function motionParameterText(
  meta: ParameterMetadata | undefined,
  value: ParameterValue | null | undefined,
): string {
  if (!meta || !value) return "";
  if (value.type === "position") {
    return String(Number(positionToTurns(value.value).toPrecision(9)));
  }
  if (value.type === "f32") {
    return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
  }
  return String(value.value);
}

export function parseMotionParameter(
  meta: ParameterMetadata,
  text: string,
): ParameterValue {
  const parsed = Number(text.trim());
  if (!Number.isFinite(parsed)) throw new Error(`${meta.label}: value must be finite.`);

  switch (meta.typeName) {
    case "position":
      return { type: "position", value: turnsToPosition(parsed) };
    case "f32":
      return { type: "f32", value: parsed };
    case "u8":
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) {
        throw new Error(`${meta.label}: expected u8.`);
      }
      return { type: "u8", value: parsed };
    case "i8":
    case "i32":
      if (!Number.isInteger(parsed)) throw new Error(`${meta.label}: expected integer.`);
      return { type: meta.typeName, value: parsed };
    case "u32":
      if (!Number.isInteger(parsed) || parsed < 0) throw new Error(`${meta.label}: expected u32.`);
      return { type: "u32", value: parsed };
  }
}
