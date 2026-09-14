import { invoke } from "@tauri-apps/api/core";
import type { ScopeConfig, ScopeSnapshot } from "./types";

export function configureScope(parameterIds: number[], historySeconds = 10): Promise<ScopeConfig> {
  return invoke<ScopeConfig>("scope_configure", { parameterIds, historySeconds });
}

export function startScope(): Promise<void> {
  return invoke("scope_live");
}

export function pauseScope(): Promise<void> {
  return invoke("scope_pause");
}

export function clearScope(): Promise<void> {
  return invoke("scope_clear");
}

export function readScopeSnapshot(windowSeconds = 0.5, maxPoints = 2500): Promise<ScopeSnapshot> {
  return invoke<ScopeSnapshot>("scope_snapshot", { windowSeconds, maxPoints });
}
