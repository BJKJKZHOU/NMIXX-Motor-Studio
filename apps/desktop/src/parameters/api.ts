import { invoke } from "@tauri-apps/api/core";
import type { ParameterMetadata, ParameterRead, ParameterValue } from "./types";

export function listParameters(): Promise<ParameterMetadata[]> {
  return invoke<ParameterMetadata[]>("parameter_list");
}

export function readParameter(id: number): Promise<ParameterRead> {
  return invoke<ParameterRead>("parameter_read", { id });
}

export function readParameters(ids: number[]): Promise<ParameterRead[]> {
  return invoke<ParameterRead[]>("parameter_read_many", { ids });
}

export function writeParameter(id: number, value: ParameterValue): Promise<void> {
  return invoke<void>("parameter_write", { id, value });
}
