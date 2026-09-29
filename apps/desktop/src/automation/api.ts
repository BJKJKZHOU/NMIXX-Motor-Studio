import { invoke } from "@tauri-apps/api/core";

export type ScriptDocument = { name: string; source: string; workingDirectory: string | null };
export type LogLine = { sequence: number; elapsedMs: number; source: string; text: string };
export type TaskState = "IDLE" | "STARTING" | "RUNNING" | "CANCELLING" | "COMPLETED" | "CANCELLED" | "FAILED";
export type TaskSnapshot = {
  taskId: number; state: TaskState; name: string; exitCode: number | null; message: string | null;
  firstSequence: number; nextSequence: number; logs: LogLine[];
  effects: Array<{ sequence: number; kind: string; payload: unknown }>;
};
export function builtinScript(kind = "diagnosis") { return invoke<ScriptDocument>("automation_builtin", { kind }); }
export function openScript(path: string) { return invoke<ScriptDocument>("automation_open_script", { path }); }
export function runScript(document: ScriptDocument, program: string, timeoutSeconds: number) {
  return invoke<TaskSnapshot>("automation_start", { spec: { ...document, program, arguments: [], timeoutSeconds } });
}
export function readTask(after: number) { return invoke<TaskSnapshot>("automation_snapshot", { after }); }
export function cancelTask(taskId: number) { return invoke<void>("automation_cancel", { taskId }); }
export function exportLog(path: string) { return invoke<void>("automation_export_log", { path }); }
