import { invoke } from "@tauri-apps/api/core";
import type { ConnectionInfo } from "./types";

export function listDevices(): Promise<string[]> {
  return invoke<string[]>("device_list");
}

export function connectDevice(port: string, schemaPath: string, baud?: number): Promise<ConnectionInfo> {
  return invoke<ConnectionInfo>("device_connect", { port, schemaPath, baud });
}

export function disconnectDevice(): Promise<void> {
  return invoke("device_disconnect");
}
