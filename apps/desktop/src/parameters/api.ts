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

export async function readCurrentParameters(ids: number[]): Promise<ParameterReadResult[]> {
  const cached = await readCachedParameters(ids);
  const missing = cached.filter((item) => item.value === null).map((item) => item.id);
  if (missing.length === 0) return cached;

  const fresh = await readParameters(missing);
  const freshById = new Map(fresh.map((item) => [item.id, item]));
  return cached.map((item) => freshById.get(item.id) ?? item);
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
