import { get, writable, type Readable } from "svelte/store";
import { parameterText, parseParameterText } from "./codec";
import { ParameterDrafts } from "./drafts";
import { commitParameter, type ParameterView } from "./state";
import type { ParameterMetadata, ParameterRead, ParameterValue } from "./types";

export type ParameterCodec = {
  format: (meta: ParameterMetadata, value: ParameterValue | null | undefined) => string;
  parse: (meta: ParameterMetadata, text: string) => ParameterValue;
};
export const defaultParameterCodec: ParameterCodec = {
  format: (_meta, value) => parameterText(value),
  parse: parseParameterText,
};
export type ParameterEditorState = { drafts: Record<string, string>; writing: Set<string>; dirty: Set<string> };

// Only drafts belong to this editor. Committed values are always read from the shared view.
export function createParameterEditor(view: Readable<ParameterView>, codec = defaultParameterCodec) {
  const edited = new ParameterDrafts();
  let latest = get(view);
  function snapshot(): ParameterEditorState {
    return {
      writing: latest.writing,
      dirty: new Set(Object.values(latest.metadata).filter((meta) => edited.dirty(meta.symbol, codec.format(meta, latest.values[meta.symbol]))).map((meta) => meta.symbol)),
      drafts: Object.fromEntries(Object.values(latest.metadata).map((meta) => [
        meta.symbol, edited.text(meta.symbol, codec.format(meta, latest.values[meta.symbol])),
      ])),
    };
  }
  const store = writable<ParameterEditorState>(snapshot(), () => view.subscribe((value) => {
    latest = value;
    edited.resetForConnection(value.epoch);
    store.set(snapshot());
  }));
  function edit(symbol: string, text: string) { edited.edit(symbol, text); store.set(snapshot()); }
  function discard(symbol: string) { edited.discard(symbol); store.set(snapshot()); }
  function dirty(symbol: string) {
    const meta = latest.metadata[symbol];
    return !!meta && edited.dirty(symbol, codec.format(meta, latest.values[symbol]));
  }
  async function select(symbol: string, value: ParameterValue): Promise<ParameterRead> {
    const meta = latest.metadata[symbol];
    if (!meta || !meta.access.includes("w")) throw new Error(`${meta?.label ?? "Parameter"} is unavailable or read-only.`);
    if (latest.loading) throw new Error("Parameter snapshot is still loading.");
    if (latest.saving || latest.writing.has(symbol)) throw new Error(`${meta.label}: a write or Save is already in progress.`);
    const token = latest.epoch;
    try { return await commitParameter(meta.id, value); }
    finally { if (latest.epoch === token) discard(symbol); }
  }
  async function commitText(symbol: string, text: string): Promise<ParameterRead> {
    const meta = latest.metadata[symbol];
    if (!meta) throw new Error("Parameter is unavailable.");
    const token = latest.epoch;
    try { return await select(symbol, codec.parse(meta, text)); }
    catch (error) { if (latest.epoch === token) discard(symbol); throw error; }
  }
  function commit(symbol: string): Promise<ParameterRead> {
    const meta = latest.metadata[symbol];
    const text = edited.text(symbol, meta ? codec.format(meta, latest.values[symbol]) : "");
    // Capture text before the Enter handler blurs the field or any readback arrives.
    return commitText(symbol, text);
  }
  function keydown(event: KeyboardEvent, symbol: string, onError: (error: unknown) => void) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commit(symbol).catch(onError);
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      discard(symbol);
      (event.currentTarget as HTMLInputElement).blur();
    }
  }
  return { subscribe: store.subscribe, edit, discard, dirty, commit, commitText, select, keydown };
}
