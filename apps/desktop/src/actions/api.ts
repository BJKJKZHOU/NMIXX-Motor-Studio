import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ActionCompletion, ActionHandle, ActionMetadata } from "./types";

export type IdentificationKind = "rsLs" | "flux" | "jb";
export type IdentificationStartResult =
  | { status: "requires_enable" }
  | { status: "started"; handle: ActionHandle };

export function listActions(): Promise<ActionMetadata[]> {
  return invoke<ActionMetadata[]>("action_list");
}

export function startAction(key: string): Promise<ActionHandle> {
  return invoke<ActionHandle>("action_start", { key });
}

export function enableMotor(): Promise<ActionHandle> {
  return invoke<ActionHandle>("motor_enable");
}

export function disableMotor(): Promise<ActionHandle> {
  return invoke<ActionHandle>("motor_disable");
}

export function stopMotor(): Promise<ActionHandle> {
  return invoke<ActionHandle>("motor_stop");
}

export function canSaveParameters(): Promise<boolean> {
  return invoke<boolean>("config_save_available");
}

export function saveParameters(): Promise<ActionHandle> {
  return invoke<ActionHandle>("config_save");
}

export function onActionCompleted(handler: (completion: ActionCompletion) => void): Promise<UnlistenFn> {
  return listen<ActionCompletion>("action-completed", (event) => handler(event.payload));
}

export function startIdentification(
  kind: IdentificationKind,
  allowEnable: boolean,
): Promise<IdentificationStartResult> {
  const wireKind = kind === "rsLs" ? "rs_ls" : kind;
  return invoke<IdentificationStartResult>("identification_start", {
    kind: wireKind,
    allowEnable,
  });
}
