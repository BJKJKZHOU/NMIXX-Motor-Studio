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

export function startTuningExperiment(): Promise<TuningExperimentStatus> {
  return invoke<TuningExperimentStatus>("tuning_experiment_start");
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
