import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { synchronizeParameters } from "../parameters/state";
import type { ActionCompletion, ActionHandle, ActionMetadata } from "./types";
import type { PreflightIssue } from "../preflight/api";

export type IdentificationKind = "rsLs" | "flux" | "jb";
export type IdentificationStartResult =
  | { status: "blocked"; issues: PreflightIssue[] }
  | { status: "requires_enable" }
  | { status: "started"; handle: ActionHandle };

async function reflected<T>(operation: Promise<T>): Promise<T> {
  const result = await operation;
  await synchronizeParameters();
  return result;
}
export function listActions(): Promise<ActionMetadata[]> { return invoke<ActionMetadata[]>("action_list"); }
export function startAction(key: string): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("action_start", { key })); }
export function startImmediateAction(key: string): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("action_start_immediate", { key })); }
export function enableMotor(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("motor_enable")); }
export function disableMotor(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("motor_disable")); }
export function stopMotor(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("motor_stop")); }
export function canSaveParameters(): Promise<boolean> { return invoke<boolean>("config_save_available"); }
export function saveParameters(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("config_save")); }
export async function onActionCompleted(handler: (completion: ActionCompletion) => void): Promise<UnlistenFn> {
  let disposed = false;
  const stop = await listen<ActionCompletion>("action-completed", (event) => {
    // Application publishes completion only after its shared readback. The client
    // mirrors that snapshot before a page inspects identification result Parameters.
    void synchronizeParameters().then(
      () => { if (!disposed) handler(event.payload); },
      () => { if (!disposed) handler(event.payload); },
    );
  });
  return () => { disposed = true; stop(); };
}
export function onMotorStopIssued(handler: () => void): Promise<UnlistenFn> { return listen("motor-stop-issued", () => handler()); }
export function startIdentification(kind: IdentificationKind, allowEnable: boolean): Promise<IdentificationStartResult> {
  return reflected(invoke<IdentificationStartResult>("identification_start", { kind: kind === "rsLs" ? "rs_ls" : kind, allowEnable }));
}
export function applyIdentification(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("identification_apply")); }
