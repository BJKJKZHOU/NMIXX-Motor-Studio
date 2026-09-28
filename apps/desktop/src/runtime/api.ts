import { invoke } from "@tauri-apps/api/core";
import type { RuntimeTelemetry } from "./types";

export function readRuntimeTelemetry(): Promise<RuntimeTelemetry> {
  return invoke<RuntimeTelemetry>("runtime_telemetry");
}
