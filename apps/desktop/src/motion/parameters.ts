import type { ParameterMetadata, ParameterValue, PositionValue } from "../parameters/types";
import { parameterEnumSymbol, parameterEnumValue, parameterText, parseParameterText } from "../parameters/codec";
import type { MotionMode } from "./types";

export const MOTION_MODE = "PARAM_MOTOR_MODE";
// Position planning uses the absolute shared Speed Target as its maximum speed.
export const MOTION_MAX_SPEED = "PARAM_TARGET_SPEED";
export const MOTION_ACCEL = "PARAM_MOTION_WM_ACC";
export const MOTION_PROFILE = "PARAM_MOTION_PROFILE";
export const MOTION_DECEL = "PARAM_MOTION_WM_DEC";
export const TARGET_POSITION = "PARAM_TARGET_POSITION";
export const TARGET_SPEED = "PARAM_TARGET_SPEED";
export const TARGET_TORQUE = "PARAM_TARGET_TORQUE";
export const TORQUE_RAMP = "PARAM_MOTION_TE_RATE";
export const SENSORLESS_STARTUP_CURRENT = "PARAM_MOTOR_IF_CURRENT";
export const SENSORLESS_ENTRY_SPEED = "PARAM_SENSORLESS_ENTRY_SPEED";
export const MOTION_PARAMETER_SYMBOLS = [MOTION_MODE, MOTION_MAX_SPEED, MOTION_ACCEL, MOTION_DECEL, MOTION_PROFILE,
  TARGET_POSITION, TARGET_TORQUE, TORQUE_RAMP, SENSORLESS_STARTUP_CURRENT, SENSORLESS_ENTRY_SPEED] as const;
const MODE_SYMBOL: Record<MotionMode, string> = {
  position: "POSITION", speed: "SPEED", "sensorless-speed": "SENSORLESS_SPEED", torque: "TORQUE",
};
export function modeFromParameter(meta: ParameterMetadata | undefined, value: ParameterValue | null | undefined): MotionMode | undefined {
  if (!meta || !value || value.type !== "u8") return undefined;
  const symbol = parameterEnumSymbol(meta, Number(value.value));
  return (Object.entries(MODE_SYMBOL) as Array<[MotionMode, string]>).find(([, candidate]) => candidate === symbol)?.[0];
}
export function modeParameterValue(meta: ParameterMetadata | undefined, mode: MotionMode): number | undefined {
  return parameterEnumValue(meta, MODE_SYMBOL[mode]);
}
export function positionToTurns(value: PositionValue): number {
  return Number(value.turns) + Number(value.theta) / (2 * Math.PI);
}
export function turnsToPosition(turns: number): PositionValue {
  if (!Number.isFinite(turns)) throw new Error("Position must be finite.");
  const whole = Math.floor(turns);
  if (whole < -2147483648 || whole > 2147483647) throw new Error("Position exceeds the supported turn range.");
  return { turns: whole, theta: (turns - whole) * 2 * Math.PI };
}
export function motionParameterText(meta: ParameterMetadata | undefined, value: ParameterValue | null | undefined): string {
  if (!meta || !value) return "";
  return value.type === "position" ? String(Number(positionToTurns(value.value).toPrecision(9))) : parameterText(value);
}
export function parseMotionParameter(meta: ParameterMetadata, text: string): ParameterValue {
  if (meta.typeName !== "position") return parseParameterText(meta, text);
  if (!text.trim()) throw new Error(`${meta.label}: value is required.`);
  return { type: "position", value: turnsToPosition(Number(text.trim())) };
}
export const motionParameterCodec = { format: motionParameterText, parse: parseMotionParameter };

const PROFILE_LABELS: Record<string, string> = {
  MOTION_TRAPEZOID: "Trapezoidal",
  MOTION_S_TIME: "S-curve · same time",
  MOTION_S_PEAK: "S-curve · limited acceleration",
};
export function motionProfileOptions(meta: ParameterMetadata | undefined): Array<{ value: number; label: string }> {
  return (meta?.allowedSymbols ?? []).flatMap((symbol) => {
    const value = parameterEnumValue(meta, symbol);
    const label = PROFILE_LABELS[symbol];
    return value === undefined || !label ? [] : [{ value, label }];
  });
}
export function motionProfileLabel(meta: ParameterMetadata | undefined, value: ParameterValue | null | undefined): string {
  if (!meta || value?.type !== "u8") return "—";
  return PROFILE_LABELS[parameterEnumSymbol(meta, Number(value.value)) ?? ""] ?? "—";
}
