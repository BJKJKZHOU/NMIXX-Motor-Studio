import { get, writable } from "svelte/store";
import { getMotion, getMotionPreview, runMotion, setMotion, stopMotion } from "./api";
import type { MotionPreview, MotionState } from "./types";

const defaultMotion: MotionState = {
  mode: "position",
  trajectory: "trapezoidal",
  acceleration: 20,
  deceleration: 20,
  filterTimeMs: 20,
  sCurveMode: "peak-accel",
  repeat: false,

  positionCommand: "incremental",
  positionTargetTurn: 1,
  positionMaxSpeed: 8,

  speedTarget: 20,
  sensorlessSpeedTarget: 20,
  sensorlessStartupCurrent: 1,
  sensorlessEntrySpeed: 8,

  torqueTargetNm: 0.2,
  torqueRampNmPerS: 1,

  mitPositionRef: 0,
  mitVelocityRef: 0,
  mitKp: 10,
  mitKd: 0.5,
  mitTorqueFeedforward: 0,
};

export const motionState = writable<MotionState>(defaultMotion);
export const motionPreview = writable<MotionPreview | undefined>(undefined);

let initialized = false;
let revision = 0;
let writeChain: Promise<void> = Promise.resolve();

export async function initializeMotion(): Promise<void> {
  if (initialized) return;
  const config = await getMotion();
  motionState.set(config);
  motionPreview.set(await getMotionPreview());
  initialized = true;
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
