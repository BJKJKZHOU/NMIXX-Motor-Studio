import { get, writable } from "svelte/store";
import { getMotion, getMotionPreview, runMotion, setMotion, stopMotion } from "./api";
import type { MotionPreview, MotionState } from "./types";

const defaultMotion: MotionState = {
  positionCommand: "incremental",
  incrementalDeltaTurn: 1,
  repeat: false,
};

export const motionState = writable<MotionState>(defaultMotion);
export const motionPreview = writable<MotionPreview | undefined>(undefined);

let revision = 0;
let writeChain: Promise<void> = Promise.resolve();

export async function initializeMotion(): Promise<void> {
  motionState.set(await getMotion());
  motionPreview.set(await getMotionPreview());
}

export async function updateMotion<K extends keyof MotionState>(
  key: K,
  value: MotionState[K],
): Promise<void> {
  const previous = get(motionState);
  const next = { ...previous, [key]: value };
  const currentRevision = ++revision;
  motionState.set(next);

  const write = writeChain.then(async () => {
    const canonical = await setMotion(next);
    if (currentRevision !== revision) return;
    motionState.set(canonical);
    motionPreview.set(await getMotionPreview());
  });
  writeChain = write.catch(() => undefined);

  try {
    await write;
  } catch (error) {
    if (currentRevision === revision) motionState.set(previous);
    throw error;
  }
}

export async function refreshMotionPreview(): Promise<void> {
  motionPreview.set(await getMotionPreview());
}

export async function executeMotion(): Promise<void> {
  await writeChain;
  await runMotion();
}

export async function stopMotionExecution(): Promise<void> {
  await stopMotion();
}
