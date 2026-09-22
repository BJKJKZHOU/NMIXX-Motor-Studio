import type { PlotChannel } from "../../connection/types";
import type { ScopeRate } from "./types";

export type ScopeChannelViewSetting = {
  id: number;
  selected: boolean;
  rate?: ScopeRate;
  color?: number;
  scalePerDiv?: number;
  yPosition?: number;
};

export type ScopeViewSettings = {
  version: 1;
  timePerDiv: number;
  cursorEnabled: boolean;
  activeChannelId?: number;
  channels: ScopeChannelViewSetting[];
};

const storagePrefix = "nmixx.scope.view.v1";

function channelSignature(channels: PlotChannel[]): string {
  return channels
    .map((channel) =>
      [
        channel.id.toString(16).padStart(4, "0"),
        channel.label,
        channel.supportsFast ? "F" : "",
        channel.supportsNormal ? "N" : "",
      ].join(":"))
    .join("|");
}

function storageKey(channels: PlotChannel[]): string {
  return `${storagePrefix}:${channelSignature(channels)}`;
}

export function loadScopeViewSettings(channels: PlotChannel[]): ScopeViewSettings | undefined {
  if (typeof localStorage === "undefined" || channels.length === 0) return undefined;
  try {
    const raw = localStorage.getItem(storageKey(channels));
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as Partial<ScopeViewSettings>;
    if (parsed.version !== 1 || !Array.isArray(parsed.channels)) return undefined;
    return parsed as ScopeViewSettings;
  } catch {
    return undefined;
  }
}

export function saveScopeViewSettings(channels: PlotChannel[], settings: ScopeViewSettings): void {
  if (typeof localStorage === "undefined" || channels.length === 0) return;
  try {
    localStorage.setItem(storageKey(channels), JSON.stringify(settings));
  } catch {
    // View persistence is optional. Scope operation must not fail if storage is unavailable.
  }
}
