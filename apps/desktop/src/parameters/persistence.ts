import { writable } from "svelte/store";
import type { ParameterReadResult, ParameterValue } from "./types";

const baseline = new Map<number, ParameterValue>();
const current = new Map<number, ParameterValue>();

export const modifiedParameterIds = writable<Set<number>>(new Set());

function nearlyEqual(a: number, b: number): boolean {
  const scale = Math.max(1, Math.abs(a), Math.abs(b));
  return Math.abs(a - b) <= 1e-6 * scale;
}

function equalValue(a: ParameterValue | undefined, b: ParameterValue | undefined): boolean {
  if (!a || !b || a.type !== b.type) return false;

  switch (a.type) {
    case "f32":
      return b.type === "f32" && nearlyEqual(Number(a.value), Number(b.value));
    case "position":
      return b.type === "position"
        && a.value.turns === b.value.turns
        && nearlyEqual(Number(a.value.theta), Number(b.value.theta));
    default:
      return b.type === a.type && Number(a.value) === Number(b.value);
  }
}

function publish() {
  const modified = new Set<number>();
  for (const [id, value] of current) {
    const saved = baseline.get(id);
    if (saved && !equalValue(saved, value)) modified.add(id);
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

export function observeParameterValue(id: number, value: ParameterValue) {
  current.set(id, value);
  publish();
}

export function commitParameterPersistence() {
  baseline.clear();
  for (const [id, value] of current) baseline.set(id, value);
  publish();
}

export function clearParameterPersistence() {
  baseline.clear();
  current.clear();
  modifiedParameterIds.set(new Set());
}
