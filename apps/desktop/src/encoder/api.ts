import { invoke } from "@tauri-apps/api/core";
import type { ActionHandle } from "../actions/types";

/** Start servo phase alignment through the application-level semantic action. */
export function startPhaseSearch(): Promise<ActionHandle> {
  return invoke<ActionHandle>("phase_search_start");
}
