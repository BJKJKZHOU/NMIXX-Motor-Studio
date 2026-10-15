import { parameterEnumSymbol } from "../parameters/codec";
import type { ParameterMetadata, ParameterValue } from "../parameters/types";

export type MotorState = "DISABLED" | "ENABLED" | "RUN";

export function motorStateFromParameter(
  meta: ParameterMetadata | undefined,
  value: ParameterValue | null | undefined,
): MotorState | null {
  if (!meta || !value || value.type !== "u8") return null;
  const symbol = parameterEnumSymbol(meta, Number(value.value));
  return symbol === "DISABLED" || symbol === "ENABLED" || symbol === "RUN" ? symbol : null;
}

// The global power toggle shows the *reported enable state*, not the next
// Action. RUN is still enabled; stop/disable behavior remains independent.
export function motorEnableStatus(state: MotorState | null, ready: boolean): "DISABLED" | "ENABLED" | "UNKNOWN" {
  if (!ready || state === null) return "UNKNOWN";
  return state === "DISABLED" ? "DISABLED" : "ENABLED";
}
