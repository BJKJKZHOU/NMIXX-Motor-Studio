import { writable } from "svelte/store";
import { subscribeRefresh } from "../refreshScheduler";
import { readRuntimeTelemetry } from "./api";
import type { RuntimeTelemetry } from "./types";

const EMPTY: RuntimeTelemetry = {
  currentIq: null,
  speedWm: null,
  positionTurns: null,
  vbus: null,
};

export const runtimeTelemetry = writable<RuntimeTelemetry>(EMPTY);

let reading = false;
let repoll = false;
let revision = 0;

export function clearRuntimeTelemetry(): void {
  ++revision;
  repoll = false;
  runtimeTelemetry.set(EMPTY);
}

export async function refreshRuntimeTelemetry(): Promise<void> {
  if (reading) {
    repoll = true;
    return;
  }
  reading = true;
  const token = revision;
  try {
    const next = await readRuntimeTelemetry();
    if (token === revision) runtimeTelemetry.set(next);
  } catch {
    // Disconnect/reconnect may invalidate a memory-only IPC already in flight.
    // The next scheduler tick (or repoll below) will use the current session.
  } finally {
    reading = false;
    if (repoll) {
      repoll = false;
      void refreshRuntimeTelemetry();
    }
  }
}

export function startRuntimeTelemetryTracking(connected: () => boolean): () => void {
  return subscribeRefresh(16, () => {
    if (connected()) void refreshRuntimeTelemetry();
  });
}
