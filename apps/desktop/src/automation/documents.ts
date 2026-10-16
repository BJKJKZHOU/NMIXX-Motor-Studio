import { derived, get, writable } from "svelte/store";
import { builtinScript, openScript, saveScript, type ScriptDocument } from "./api";

export type ScriptTab = ScriptDocument & {
  id: string;
  savedSource: string;
  /** A bundled example is opened for editing but can only be saved to a new file. */
  example: boolean;
};

export const scriptTabs = writable<ScriptTab[]>([]);
export const activeScriptId = writable<string | null>(null);
export const activeScript = derived([scriptTabs, activeScriptId],
  ([$tabs, $id]) => $tabs.find((tab) => tab.id === $id) ?? null);

let nextTab = 0;
function add(document: ScriptDocument, example = false, identity?: string): ScriptTab {
  const existing = get(scriptTabs).find((tab) => identity && tab.id === identity
    || document.path && tab.path === document.path);
  if (existing) { activeScriptId.set(existing.id); return existing; }
  const tab: ScriptTab = {
    ...document, id: identity ?? `editor-${++nextTab}`,
    savedSource: document.source, example,
  };
  scriptTabs.update((tabs) => [...tabs, tab]);
  activeScriptId.set(tab.id);
  return tab;
}
export function newScript(): ScriptTab {
  const number = ++nextTab;
  return add({
    name: `Untitled-${number}.py`, source: "from nmixx import client\n\n",
    workingDirectory: null, path: null,
  }, false, `editor-${number}`);
}
/** Browser-selected files are imported as editable tabs. Save As assigns a path. */
export function importScript(name: string, source: string): ScriptTab {
  return add({ name, source, path: null, workingDirectory: null });
}
export async function openLocalScript(path: string): Promise<ScriptTab> {
  const existing = get(scriptTabs).find((tab) => tab.path === path);
  if (existing) { activeScriptId.set(existing.id); return existing; }
  const document = await openScript(path);
  return add(document);
}
export async function openExample(kind: "diagnosis" | "encoder" | "motion"): Promise<ScriptTab> {
  const id = `example:${kind}`;
  const existing = get(scriptTabs).find((tab) => tab.id === id);
  if (existing) { activeScriptId.set(id); return existing; }
  return add(await builtinScript(kind), true, id);
}
export function selectScript(id: string) {
  if (get(scriptTabs).some((tab) => tab.id === id)) activeScriptId.set(id);
}
export function editScript(id: string, source: string) {
  scriptTabs.update((tabs) => tabs.map((tab) => tab.id === id ? { ...tab, source } : tab));
}
export function closeScript(id: string) {
  const tabs = get(scriptTabs);
  const index = tabs.findIndex((tab) => tab.id === id);
  if (index === -1) return;
  const remaining = tabs.filter((tab) => tab.id !== id);
  scriptTabs.set(remaining);
  if (get(activeScriptId) === id)
    activeScriptId.set(remaining[Math.min(index, remaining.length - 1)]?.id ?? null);
}
export function isDirty(tab: ScriptTab): boolean {
  return (!tab.path && !tab.example) || tab.source !== tab.savedSource;
}
export async function saveCurrentScript(path?: string): Promise<ScriptTab> {
  const current = get(activeScript);
  if (!current) throw new Error("Open a script first");
  const target = path?.trim() || current.path;
  if (!target) throw new Error("Choose a file path for Save As");
  const overwrite = !current.example && current.path === target;
  const saved = await saveScript(target, current.source, overwrite);
  // Preserve user edits made while disk I/O was in flight.
  const next: ScriptTab = { ...current, ...saved, source: current.source,
    savedSource: saved.source, example: false };
  scriptTabs.update((tabs) => tabs.map((tab) => tab.id === current.id
    ? { ...tab, name: next.name, path: next.path, workingDirectory: next.workingDirectory,
        savedSource: next.savedSource, example: false }
    : tab));
  return next;
}
