import { writable } from "svelte/store";
import { equalParameterValue } from "./codec";
import type { ParameterReadResult, ParameterValue } from "./types";

// Presentation baseline only. Firmware owns which configuration is actually persisted.
const baseline = new Map<number, ParameterValue>();
const current = new Map<number, ParameterValue>();
export const modifiedParameterIds = writable<Set<number>>(new Set());

function publish() {
  const modified = new Set<number>();
  for (const [id, value] of current) {
    const saved = baseline.get(id);
    if (saved && !equalParameterValue(saved, value)) modified.add(id);
  }
  modifiedParameterIds.set(modified);
}

export function initializeParameterPersistence(results: ParameterReadResult[]) {
  baseline.clear();
  current.clear();
  for (const result of results) {
    if (!result.value) continue;
    baseline.set(result.id, result.value);
    current.set(result.id, result.value);
  }
  publish();
}

export function observeParameterResults(results: ParameterReadResult[]) {
  for (const result of results) {
    if (result.value) current.set(result.id, result.value);
  }
  publish();
}

export function commitParameterPersistence(results: ParameterReadResult[]) {
  baseline.clear();
  for (const result of results) {
    if (result.value) baseline.set(result.id, result.value);
  }
  publish();
}

export function clearParameterPersistence() {
  baseline.clear();
  current.clear();
  modifiedParameterIds.set(new Set());
}
