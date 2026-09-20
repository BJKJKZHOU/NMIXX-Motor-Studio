import { invoke } from "@tauri-apps/api/core";
import type { ScopeConfig, ScopeSnapshot } from "../analysis/scope/types";

export type TuningExperimentState =
  | "IDLE"
  | "PREPARING"
  | "RUNNING"
  | "STOPPING"
  | "COMPLETED"
  | "FAILED";

export type TuningExperimentStatus = {
  state: TuningExperimentState;
  message?: string | null;
};

export type TuningExperimentSnapshot = {
  status: TuningExperimentStatus;
  config: ScopeConfig;
  snapshot: ScopeSnapshot;
};

export type TuningExperimentSelection = {
  id: number;
  rate: "fast" | "normal";
};

export function tuningExperimentDefaults(): Promise<TuningExperimentSelection[]> {
  return invoke<TuningExperimentSelection[]>("tuning_experiment_defaults");
}

export function startTuningExperiment(selections: TuningExperimentSelection[]): Promise<TuningExperimentStatus> {
  return invoke<TuningExperimentStatus>("tuning_experiment_start", { selections });
}

export function stopTuningExperiment(): Promise<TuningExperimentStatus> {
  return invoke<TuningExperimentStatus>("tuning_experiment_stop");
}

export function tuningExperimentStatus(): Promise<TuningExperimentStatus> {
  return invoke<TuningExperimentStatus>("tuning_experiment_status");
}

export function readTuningExperimentSnapshot(maxPoints = 5000): Promise<TuningExperimentSnapshot> {
  return invoke<TuningExperimentSnapshot>("tuning_experiment_snapshot", { maxPoints });
}
