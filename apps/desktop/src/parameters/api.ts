import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ParameterMetadata, ParameterRead, ParameterReadResult, ParameterValue } from "./types";

export function listParameters(): Promise<ParameterMetadata[]> {
  return invoke<ParameterMetadata[]>("parameter_list");
}

export function readParameter(id: number): Promise<ParameterRead> {
  return invoke<ParameterRead>("parameter_read", { id });
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

export function onParametersRefreshed(handler: () => void): Promise<UnlistenFn> {
  return listen("parameters-refreshed", () => handler());
}

export function writeParameter(id: number, value: ParameterValue): Promise<void> {
  return invoke<void>("parameter_write", { id, value });
}
