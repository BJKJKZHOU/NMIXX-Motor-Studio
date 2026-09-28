import { get, writable } from "svelte/store";
import { subscribeRefresh } from "../refreshScheduler";
import * as api from "./api";
import type { ProblemSnapshot } from "./types";

const EMPTY: ProblemSnapshot = { active: [], history: [] };
export const problemSnapshot = writable<ProblemSnapshot>(EMPTY);
export const problemError = writable("");
let refreshing = false;
let repoll = false;
let revision = 0;

function apply(snapshot: ProblemSnapshot, token: number) {
  if (token !== revision) return;
  problemSnapshot.set(snapshot);
  problemError.set("");
}

export function clearProblemsProjection(): void {
  ++revision;
  refreshing = false;
  repoll = false;
  problemSnapshot.set(EMPTY);
  problemError.set("");
}

export async function refreshProblems(): Promise<void> {
  if (refreshing) { repoll = true; return; }
  refreshing = true;
  const token = revision;
  try { apply(await api.readProblems(), token); }
  catch (error) { if (token === revision) problemError.set(String(error)); }
  finally {
    refreshing = false;
    if (repoll && token === revision) { repoll = false; void refreshProblems(); }
  }
}

export async function recheckProblems(): Promise<void> {
  const token = revision;
  apply(await api.recheckProblems(), token);
}

export async function clearProblemHistory(): Promise<void> {
  const token = revision;
  apply(await api.clearProblemHistory(), token);
}

export function startProblemsTracking(connected: () => boolean): () => void {
  return subscribeRefresh(250, () => {
    if (connected()) void refreshProblems();
    else if (get(problemSnapshot).active.length || get(problemSnapshot).history.length) clearProblemsProjection();
  });
}
