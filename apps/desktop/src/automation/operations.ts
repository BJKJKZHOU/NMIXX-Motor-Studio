import { writable } from "svelte/store";
import type { ScopeRate } from "../analysis/scope/types";

export type ScopeOperation = {
  config: { historySeconds: number; channels: Array<{ id: number; rate: ScopeRate }> };
  state: string;
};
// A notification of a completed shared operation, not another device cache.
export const scopeOperation = writable<ScopeOperation | undefined>(undefined);
