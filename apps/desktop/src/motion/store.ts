import { get, writable } from "svelte/store";
import { getMotion, getMotionPreview, runMotion, setMotion, stopMotion } from "./api";
import { parameterState, selectParameters, waitParameterWrites } from "../parameters/state";
import { equalParameterValue } from "../parameters/codec";
import { MOTION_PARAMETER_SYMBOLS } from "./parameters";
import type { MotionPreview, MotionState } from "./types";
import type { ParameterValue } from "../parameters/types";

const defaultMotion: MotionState = { positionCommand: "incremental", incrementalDeltaTurn: 1, repeat: false };
export const motionState = writable<MotionState>(defaultMotion);
export const motionPreview = writable<MotionPreview | undefined>(undefined);
export const motionPreviewError = writable<string | null>(null);
let revision = 0;
let writeChain: Promise<void> = Promise.resolve();
let previewRevision = 0;

/** Mirror a completed Application operation; never submit a second write. */
export function acceptMotionSnapshot(config: MotionState): void {
  ++revision;
  motionState.set(config);
}

export async function initializeMotion(): Promise<void> {
  const epoch = get(parameterState).epoch;
  const before = revision;
  const loaded = await getMotion();
  if (get(parameterState).epoch === epoch && before === revision) motionState.set(loaded);
}

export async function updateMotion<K extends keyof MotionState>(key: K, value: MotionState[K]): Promise<void> {
  const epoch = get(parameterState).epoch;
  const previous = get(motionState);
  const next = { ...previous, [key]: value };
  const currentRevision = ++revision;
  motionState.set(next);
  const write = writeChain.then(async () => {
    if (get(parameterState).epoch !== epoch) throw new Error("Device connection changed.");
    const canonical = await setMotion(next);
    if (currentRevision === revision && get(parameterState).epoch === epoch) motionState.set(canonical);
  });
  writeChain = write.catch(() => undefined);
  try { await write; }
  catch (error) {
    if (currentRevision === revision && get(parameterState).epoch === epoch) motionState.set(previous);
    throw error;
  }
}

export async function refreshMotionPreview(): Promise<void> {
  const epoch = get(parameterState).epoch;
  const token = ++previewRevision;
  try {
    const preview = await getMotionPreview();
    if (epoch !== get(parameterState).epoch || token !== previewRevision) return;
    motionPreview.set(preview);
    motionPreviewError.set(null);
  } catch (error) {
    if (epoch !== get(parameterState).epoch || token !== previewRevision) return;
    motionPreview.set(undefined);
    motionPreviewError.set(String(error));
  }
}

// A preview is a pure projection: watch only its inputs, never feed preview results
// back into Parameter reads. Coalesce changes while a cache-only IPC request is active.
export function observeMotionPreview(): () => void {
  const view = selectParameters([...MOTION_PARAMETER_SYMBOLS, "PARAM_RUN_POSITION", "PARAM_LIMIT_WM_EFFECTIVE"]);
  let lastValues: Record<string, ParameterValue | null> = {};
  let lastEpoch = -1;
  let wasLoading = true;
  let disposed = false;
  let busy = false;
  let pending = false;
  function request() {
    pending = true;
    if (busy || disposed) return;
    busy = true;
    void (async () => {
      try {
        while (pending && !disposed) {
          pending = false;
          if (get(parameterState).connected && !get(parameterState).loading) await refreshMotionPreview();
        }
      } finally { busy = false; }
    })();
  }
  const stopParameters = view.subscribe((next) => {
    if (next.epoch !== lastEpoch) {
      ++previewRevision;
      motionPreview.set(undefined);
      motionPreviewError.set(null);
    }
    if (next.epoch !== lastEpoch || wasLoading !== next.loading || Object.keys(next.values).length !== Object.keys(lastValues).length
      || Object.keys(next.values).some((key) => !equalParameterValue(next.values[key], lastValues[key]))) {
      lastEpoch = next.epoch; lastValues = next.values; wasLoading = next.loading;
      request();
    }
  });
  const stopMotion = motionState.subscribe(request);
  return () => { disposed = true; pending = false; ++previewRevision; stopParameters(); stopMotion(); };
}

export async function waitMotionUpdates(): Promise<void> { await writeChain; await waitParameterWrites(); }
export async function executeMotion(): Promise<void> { await waitMotionUpdates(); await runMotion(); }
export async function stopMotionExecution(): Promise<void> { await stopMotion(); }
