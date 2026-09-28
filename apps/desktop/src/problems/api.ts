import { invoke } from "@tauri-apps/api/core";
import type { ProblemSnapshot } from "./types";

export function readProblems(): Promise<ProblemSnapshot> {
  return invoke<ProblemSnapshot>("problems_snapshot");
}
export function recheckProblems(): Promise<ProblemSnapshot> {
  return invoke<ProblemSnapshot>("problems_recheck");
}
export function clearProblemHistory(): Promise<ProblemSnapshot> {
  return invoke<ProblemSnapshot>("problems_clear_history");
}
