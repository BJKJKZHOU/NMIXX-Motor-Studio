import { get, writable } from "svelte/store";
import { subscribeRefresh } from "../refreshScheduler";
import { acceptSavedParameterBaseline, parameterState } from "../parameters/state";
import { acceptMotionSnapshot, waitMotionUpdates } from "../motion/store";
import type { ParameterReadResult } from "../parameters/types";
import type { MotionState } from "../motion/types";
import { scopeOperation, type ScopeOperation } from "./operations";
import * as api from "./api";
import type { LogLine, ScriptDocument, TaskSnapshot } from "./api";

// Script selection is view state. Execution, ownership and logs remain in Rust.
export const scriptSettings = writable({ path: "", logPath: "", program: "python3", timeoutSeconds: 300, document: null as ScriptDocument | null });
export const taskState = writable<TaskSnapshot | null>(null);
export const logs = writable<LogLine[]>([]);
export const taskError = writable("");
let polling = false;
let repoll = false;
let revision = 0;
let lastId = 0;
let lastSequence = 0;
let taskEpoch = -1;
let launchedTask = 0;
const effectSequences = new Map<string, number>();

export function isActive(state: TaskSnapshot["state"] | undefined): boolean {
  return state === "STARTING" || state === "RUNNING" || state === "CANCELLING";
}
export function clearAutomationProjection(): void {
  taskEpoch = -1;
  effectSequences.clear();
  scopeOperation.set(undefined);
}
function applyEffects(task: TaskSnapshot): void {
  // Never apply an old workflow's Save/Scope result to a reconnected device.
  if (task.taskId !== launchedTask || taskEpoch !== get(parameterState).epoch) return;
  for (const effect of task.effects) {
    if (effect.sequence <= (effectSequences.get(effect.kind) ?? 0)) continue;
    if (effect.kind === "config.saved") acceptSavedParameterBaseline(effect.payload as ParameterReadResult[]);
    else if (effect.kind === "scope.changed") scopeOperation.set(effect.payload as ScopeOperation);
    else if (effect.kind === "motion.changed") acceptMotionSnapshot(effect.payload as MotionState);
    effectSequences.set(effect.kind, effect.sequence);
  }
}
export async function refreshTask(): Promise<void> {
  if (polling) { repoll = true; return; }
  polling = true;
  const token = revision;
  try {
    let next = await api.readTask(lastSequence);
    if (token !== revision) return;
    if (next.taskId !== lastId) {
      next = await api.readTask(0);
      if (token !== revision) return;
      lastId = next.taskId;
      lastSequence = 0;
      logs.set([]);
    }
    const incoming = next.logs.filter((line) => line.sequence > lastSequence);
    logs.update((previous) => [...previous.filter((line) => line.sequence >= next.firstSequence), ...incoming]);
    lastSequence = Math.max(lastSequence, next.nextSequence - 1);
    applyEffects(next);
    taskState.set(next);
    taskError.set("");
  } catch (error) { if (token === revision) taskError.set(String(error)); }
  finally {
    polling = false;
    if (repoll) { repoll = false; void refreshTask(); }
  }
}
export function startAutomationTracking(connected: () => boolean): () => void {
  // Cache-only task IPC, on the existing desktop clock; no independent timer or
  // periodic device read. Page navigation does not stop task-state propagation.
  return subscribeRefresh(100, () => {
    if (isActive(get(taskState)?.state) || (connected() && get(taskState) === null)) void refreshTask();
  });
}
export async function ensureDocument(): Promise<void> {
  if (get(scriptSettings).document) return;
  const document = await api.builtinScript();
  scriptSettings.update((settings) => settings.document ? settings : { ...settings, document });
}
export async function selectBuiltin(kind = "diagnosis"): Promise<void> {
  const document = await api.builtinScript(kind);
  scriptSettings.update((settings) => ({ ...settings, document }));
}
export async function loadDocument(): Promise<void> {
  const document = await api.openScript(get(scriptSettings).path);
  scriptSettings.update((settings) => ({ ...settings, document }));
}
export async function runSelected(): Promise<void> {
  const settings = get(scriptSettings);
  if (!settings.document) throw new Error("Open a script first");
  const epoch = get(parameterState).epoch;
  await waitMotionUpdates(); // Wait for submitted writes; do not commit drafts.
  if (epoch !== get(parameterState).epoch) throw new Error("Device connection changed");
  const result = await api.runScript(settings.document, settings.program, settings.timeoutSeconds);
  ++revision;
  launchedTask = result.taskId;
  taskEpoch = epoch;
  effectSequences.clear();
  lastId = 0;
  lastSequence = 0;
  logs.set([]);
  taskState.set(result);
  applyEffects(result);
  await refreshTask();
}
