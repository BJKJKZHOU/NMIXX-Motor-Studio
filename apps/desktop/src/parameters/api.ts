import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ParameterMetadata, ParameterRead, ParameterReadResult, ParameterValue } from "./types";

// Transport adapter only. GUI views use state.ts and editor.ts.
export function listParameters(): Promise<ParameterMetadata[]> {
  return invoke<ParameterMetadata[]>("parameter_list");
}

export function readParameters(ids: number[]): Promise<ParameterReadResult[]> {
  return invoke<ParameterReadResult[]>("parameter_read_many", { ids });
}

export function readCachedParameters(ids: number[]): Promise<ParameterReadResult[]> {
  return invoke<ParameterReadResult[]>("parameter_cached_many", { ids });
}

export function refreshAllParameters(): Promise<ParameterReadResult[]> {
  return invoke<ParameterReadResult[]>("parameter_refresh_all");
}

export function onParametersChanged(handler: (ids: number[]) => void): Promise<UnlistenFn> {
  return listen<number[]>("parameters-changed", (event) => handler(event.payload));
}

export function writeParameter(id: number, value: ParameterValue): Promise<ParameterRead> {
  return invoke<ParameterRead>("parameter_write", { id, value });
}
