import type { MotorState } from "../motor/state";
import type { ParameterMetadata } from "./types";

/** Respect firmware HostSchema write-state requirements in business pages.
 * The firmware still performs the authoritative check at write time.
 */
export function canWriteInMotorState(meta: ParameterMetadata | undefined, state: MotorState | null): boolean {
  if (!meta?.access.includes("w")) return false;
  if (meta.writeState === "disabled") return state === "DISABLED";
  if (meta.writeState === "not_running") return state === "DISABLED" || state === "ENABLED";
  return true;
}
