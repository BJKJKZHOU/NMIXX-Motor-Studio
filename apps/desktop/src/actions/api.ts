import { invoke } from "@tauri-apps/api/core";
import { get } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { parameterState, synchronizeParameters } from "../parameters/state";
import type { ActionCompletion, ActionHandle } from "./types";
import type { PreflightIssue } from "../preflight/api";

export type IdentificationKind = "rsLs" | "flux" | "jb";
function identificationWireKind(kind: IdentificationKind): string { return kind === "rsLs" ? "rs_ls" : kind; }
export type IdentificationStartResult =
  | { status: "blocked"; issues: PreflightIssue[] }
  | { status: "requires_enable" }
  | { status: "started"; handle: ActionHandle; readbackError?: string };

// A finite Action has an accepted handle even if its subsequent cache read fails.
async function reflectedAccepted<T extends object>(operation: Promise<T>): Promise<T & { readbackError?: string }> {
  const result = await operation;
  try { await synchronizeParameters(); }
  catch (error) { return { ...result, readbackError: String(error) }; }
  return result;
}

async function reflected<T>(operation: Promise<T>): Promise<T> {
  const result = await operation;
  try {
    await synchronizeParameters();
  } catch (error) {
    // The firmware accepted the Action. A later cache read failed; reporting
    // the whole command as rejected would mislead the operator.
    throw new Error(`Device accepted the command, but Parameter readback failed: ${String(error)}`);
  }
  return result;
}
export function enableMotor(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("motor_enable")); }
export function disableMotor(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("motor_disable")); }
export function stopMotor(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("motor_stop")); }
export function canSaveParameters(): Promise<boolean> { return invoke<boolean>("config_save_available"); }
export function saveParameters(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("config_save")); }
export function clearProtection(): Promise<ActionHandle> { return reflected(invoke<ActionHandle>("protection_clear")); }
export async function onActionCompleted(handler: (completion: ActionCompletion) => void): Promise<UnlistenFn> {
  let disposed = false;
  const stop = await listen<ActionCompletion>("action-completed", (event) => {
    const epoch = get(parameterState).epoch;
    // Device completion is authoritative for the Action; cache synchronization
    // is a separate result and must never manufacture a verified UI success.
    void (async () => {
      let readbackError: string | undefined;
      try { await synchronizeParameters(); }
      catch (error) { readbackError = String(error); }
      if (!disposed && get(parameterState).epoch === epoch) {
        handler(readbackError ? { ...event.payload, readbackError } : event.payload);
      }
    })();
  });
  return () => { disposed = true; stop(); };
}
export function onMotorStopIssued(handler: () => void): Promise<UnlistenFn> { return listen("motor-stop-issued", () => handler()); }
export async function startIdentification(kind: IdentificationKind, allowEnable: boolean): Promise<IdentificationStartResult> {
  const result = await invoke<IdentificationStartResult>("identification_start", { kind: identificationWireKind(kind), allowEnable });
  return result.status === "started" ? reflectedAccepted(Promise.resolve(result)) : result;
}
export function identificationApplyCandidate(): Promise<IdentificationKind | null> {
  return invoke<IdentificationKind | null>("identification_apply_candidate");
}
export function applyIdentification(kind: IdentificationKind): Promise<ActionHandle & { readbackError?: string }> {
  return reflectedAccepted(invoke<ActionHandle>("identification_apply", { kind: identificationWireKind(kind) }));
}
