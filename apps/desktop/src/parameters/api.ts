import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ParameterMetadata, ParameterRead, ParameterReadResult, ParameterValue } from "./types";
import { initializeParameterPersistence, observeParameterResults, observeParameterValue } from "./persistence";

export function listParameters(): Promise<ParameterMetadata[]> {
  return invoke<ParameterMetadata[]>("parameter_list");
}

export async function readParameter(id: number): Promise<ParameterRead> {
  const result = await invoke<ParameterRead>("parameter_read", { id });
  observeParameterValue(result.id, result.value);
  return result;
}

export async function readParameters(ids: number[]): Promise<ParameterReadResult[]> {
  const results = await invoke<ParameterReadResult[]>("parameter_read_many", { ids });
  observeParameterResults(results);
  return results;
}

export async function readCachedParameters(ids: number[]): Promise<ParameterReadResult[]> {
  const results = await invoke<ParameterReadResult[]>("parameter_cached_many", { ids });
  observeParameterResults(results);
  return results;
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

export async function writeParameter(id: number, value: ParameterValue): Promise<void> {
  await invoke<void>("parameter_write", { id, value });
  observeParameterValue(id, value);
}

export async function initializePersistenceBaseline(ids: number[]): Promise<void> {
  const results = await invoke<ParameterReadResult[]>("parameter_cached_many", { ids });
  initializeParameterPersistence(results);
}
