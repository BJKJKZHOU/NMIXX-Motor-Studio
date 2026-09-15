import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ActionCompletion, ActionHandle, ActionMetadata } from "./types";

export function listActions(): Promise<ActionMetadata[]> {
  return invoke<ActionMetadata[]>("action_list");
}

export function startAction(key: string): Promise<ActionHandle> {
  return invoke<ActionHandle>("action_start", { key });
}

export function onActionCompleted(handler: (completion: ActionCompletion) => void): Promise<UnlistenFn> {
  return listen<ActionCompletion>("action-completed", (event) => handler(event.payload));
}
