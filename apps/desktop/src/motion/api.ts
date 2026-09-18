import { invoke } from "@tauri-apps/api/core";
import type { MotionPreview, MotionState } from "./types";

export function getMotion(): Promise<MotionState> {
  return invoke<MotionState>("motion_get");
}

export function setMotion(config: MotionState): Promise<MotionState> {
  return invoke<MotionState>("motion_set", { config });
}

export function getMotionPreview(): Promise<MotionPreview> {
  return invoke<MotionPreview>("motion_preview");
}


export function runMotion(): Promise<void> {
  return invoke<void>("motion_run");
}

export function stopMotion(): Promise<void> {
  return invoke<void>("motion_stop");
}
