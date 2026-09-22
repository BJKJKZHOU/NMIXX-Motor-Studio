import { derived, get, readonly, writable, type Readable } from "svelte/store";
import { subscribeRefresh } from "../refreshScheduler";
import * as transport from "./api";
import { equalParameterValue } from "./codec";
import { clearParameterPersistence, commitParameterPersistence, initializeParameterPersistence, observeParameterResults } from "./persistence";
import type { ParameterMetadata, ParameterRead, ParameterReadResult, ParameterValue } from "./types";

export type ParameterState = {
  epoch: number;
  connected: boolean;
  loading: boolean;
  registry: ParameterMetadata[];
  entries: Record<number, ParameterReadResult>;
  writing: Set<number>;
  saving: boolean;
  error: string | null;
};

export type ParameterView = {
  epoch: number;
  loading: boolean;
  metadata: Record<string, ParameterMetadata>;
  values: Record<string, ParameterValue | null>;
  errors: Record<string, string | null>;
  entries: ParameterReadResult[];
  writing: Set<string>;
  saving: boolean;
};

function empty(epoch: number): ParameterState {
  return { epoch, connected: false, loading: false, registry: [], entries: {}, writing: new Set(), saving: false, error: null };
}

let epoch = 0;
let unlisten: (() => void) | undefined;
let syncTail: Promise<void> = Promise.resolve();
let mutationTail: Promise<void> = Promise.resolve();
const pendingWrites = new Map<number, number>();
const mutable = writable<ParameterState>(empty(epoch));
export const parameterState = readonly(mutable);

function current(token: number) { return token === epoch; }
function requireSession(token: number) {
  if (!current(token) || !get(mutable).connected) throw new Error("Device connection changed.");
}
function report(error: unknown, token: number) {
  if (current(token)) mutable.update((state) => ({ ...state, error: String(error) }));
}
function readableIds() {
  return get(mutable).registry.filter((meta) => meta.access.includes("r")).map((meta) => meta.id);
}
function apply(results: ParameterReadResult[], token: number) {
  if (!current(token)) return;
  mutable.update((state) => {
    const entries = { ...state.entries };
    let changed = false;
    for (const result of results) {
      const previous = entries[result.id];
      if (!previous || previous.error !== result.error || !equalParameterValue(previous.value, result.value)) {
        entries[result.id] = result;
        changed = true;
      }
    }
    const failed = Object.values(entries).find((entry) => entry.error);
    const syncError = failed ? `Parameter synchronization failed: ${failed.error}` : null;
    return changed || state.error !== syncError ? { ...state, entries, error: syncError } : state;
  });
  observeParameterResults(results);
}

// Serialize cache reads so a late response cannot overwrite a newer projection.
function sync(ids: number[], token = epoch): Promise<void> {
  const selected = [...new Set(ids)];
  const request = syncTail.then(async () => {
    if (!current(token) || selected.length === 0) return;
    const results = await transport.readCachedParameters(selected);
    apply(results, token);
  });
  syncTail = request.catch((error) => report(error, token));
  return request;
}

export function disconnectParameters() {
  ++epoch;
  unlisten?.();
  unlisten = undefined;
  syncTail = Promise.resolve();
  mutationTail = Promise.resolve();
  pendingWrites.clear();
  mutable.set(empty(epoch));
  clearParameterPersistence();
}

export async function connectParameters(): Promise<void> {
  disconnectParameters();
  const token = epoch;
  mutable.update((state) => ({ ...state, connected: true, loading: true }));
  try {
    const stop = await transport.onParametersChanged((ids) => {
      if (current(token)) void sync(ids, token).catch((error) => report(error, token));
    });
    if (!current(token)) { stop(); return; }
    unlisten = stop;
    const registry = await transport.listParameters();
    if (!current(token)) return;
    mutable.update((state) => ({ ...state, registry }));
    await sync(readableIds(), token);
    if (!current(token)) return;
    const initial = Object.values(get(mutable).entries);
    const failed = initial.find((entry) => entry.error || entry.value === null);
    if (failed) throw new Error(`Initial Parameter snapshot failed: ${failed.error ?? "value unavailable"}`);
    initializeParameterPersistence(initial);
  } catch (error) {
    report(error, token);
    throw error;
  } finally {
    if (current(token)) mutable.update((state) => ({ ...state, loading: false }));
  }
}

function enqueue<T>(operation: () => Promise<T>, token: number): Promise<T> {
  const result = mutationTail.then(() => { requireSession(token); return operation(); });
  mutationTail = result.then(() => undefined, () => undefined);
  return result;
}

export function commitParameter(id: number, value: ParameterValue): Promise<ParameterRead> {
  const token = epoch;
  if (get(mutable).saving) return Promise.reject(new Error("Parameter Save is in progress."));
  pendingWrites.set(id, (pendingWrites.get(id) ?? 0) + 1);
  mutable.update((state) => ({ ...state, writing: new Set(pendingWrites.keys()) }));
  const result = enqueue(async () => {
    let result: ParameterRead;
    try {
      result = await transport.writeParameter(id, value);
    } catch (error) {
      await sync(readableIds(), token).catch((syncError) => report(syncError, token));
      throw error;
    }
    // The backend owns dependent readback. Mirroring its cache is not a second device read.
    await sync(readableIds(), token);
    requireSession(token);
    return result;
  }, token);
  return result.finally(() => {
    if (!current(token)) return;
    const count = (pendingWrites.get(id) ?? 1) - 1;
    if (count) pendingWrites.set(id, count); else pendingWrites.delete(id);
    mutable.update((state) => ({ ...state, writing: new Set(pendingWrites.keys()) }));
  });
}

export async function refreshParameters(): Promise<void> {
  const token = epoch;
  requireSession(token);
  const results = await transport.refreshAllParameters();
  await sync(readableIds(), token);
  requireSession(token);
  const failure = results.find((result) => result.error);
  if (failure) throw new Error(`Parameter Read failed: ${failure.error}`);
}

export function saveParameterBaseline<T>(save: () => Promise<T>): Promise<T> {
  const token = epoch;
  if (get(mutable).saving) return Promise.reject(new Error("Parameter Save is already in progress."));
  mutable.update((state) => ({ ...state, saving: true }));
  return enqueue(async () => {
    const result = await save();
    requireSession(token);
    await refreshParameters();
    requireSession(token);
    commitParameterPersistence(Object.values(get(mutable).entries));
    return result;
  }, token).finally(() => {
    if (current(token)) mutable.update((state) => ({ ...state, saving: false }));
  });
}

export function pollParameters(symbols: readonly string[], intervalMs: number, onError: (error: unknown) => void): () => void {
  let busy = false;
  let disposed = false;
  const stop = subscribeRefresh(intervalMs, () => {
    const state = get(mutable);
    if (disposed || busy || !state.connected || state.loading) return;
    const ids = state.registry.filter((meta) => symbols.includes(meta.symbol) && meta.access.includes("r")).map((meta) => meta.id);
    if (!ids.length) return;
    const token = epoch;
    busy = true;
    void transport.readParameters(ids).then(() => sync(ids, token)).catch((error) => {
      if (!disposed && current(token)) onError(error);
    }).finally(() => { busy = false; });
  });
  return () => { disposed = true; stop(); };
}

export function selectParameters(symbols?: readonly string[]): Readable<ParameterView> {
  let previous: ParameterView | undefined;
  let selectedMetadata: ParameterMetadata[] = [];
  let selectedEntries: Array<ParameterReadResult | undefined> = [];
  return derived(parameterState, (state, set) => {
    const metadata = state.registry.filter((meta) => !symbols || symbols.includes(meta.symbol));
    const entries = metadata.map((meta) => state.entries[meta.id]);
    const writing = new Set(metadata.filter((meta) => state.writing.has(meta.id)).map((meta) => meta.symbol));
    if (previous && previous.epoch === state.epoch && previous.loading === state.loading && previous.saving === state.saving
      && metadata.length === selectedMetadata.length && metadata.every((meta, index) => meta === selectedMetadata[index])
      && entries.every((entry, index) => entry === selectedEntries[index])
      && writing.size === previous.writing.size && [...writing].every((symbol) => previous!.writing.has(symbol))) return;
    selectedMetadata = metadata;
    selectedEntries = entries;
    previous = {
      epoch: state.epoch, loading: state.loading, saving: state.saving, writing,
      metadata: Object.fromEntries(metadata.map((meta) => [meta.symbol, meta])),
      values: Object.fromEntries(metadata.map((meta) => [meta.symbol, state.entries[meta.id]?.value ?? null])),
      errors: Object.fromEntries(metadata.map((meta) => [meta.symbol, state.entries[meta.id]?.error ?? null])),
      entries: metadata.map((meta) => state.entries[meta.id] ?? { id: meta.id, value: null, error: null }),
    };
    set(previous);
  }, { epoch: -1, loading: false, saving: false, metadata: {}, values: {}, errors: {}, entries: [], writing: new Set<string>() });
}

/** Wait for already submitted GUI edits; never commit a local text draft. */
export async function waitParameterWrites(): Promise<void> {
  const token = epoch;
  await mutationTail;
  requireSession(token);
}

/** Mirror Application-owned Action readback before delivering a domain result. */
export async function synchronizeParameters(): Promise<void> {
  const token = epoch;
  if (!get(mutable).connected) return;
  try {
    await sync(readableIds(), token);
    requireSession(token);
  } catch (error) {
    report(error, token);
    throw error;
  }
}
