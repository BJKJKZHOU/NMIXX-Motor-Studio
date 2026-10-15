import { writable } from "svelte/store";
import { enMessages } from "./i18n/locales/en";
import { zhCNMessages } from "./i18n/locales/zh-CN";

/** Add another locale by registering one language file here.
 * The statusbar toggle and type-safe messages update from this registry.
 */
export const languageOptions = [
  { code: "zh-CN", label: "中", name: "简体中文", messages: zhCNMessages },
  { code: "en", label: "EN", name: "English", messages: enMessages },
] as const;
export type UiLanguage = (typeof languageOptions)[number]["code"];
export type MessageKey = keyof typeof enMessages;
const STORAGE_KEY = "nmixx.ui.language";
const locales: Record<UiLanguage, Record<MessageKey, string>> =
  Object.fromEntries(languageOptions.map(({ code, messages }) => [code, messages])) as Record<UiLanguage, Record<MessageKey, string>>;

export function nextUiLanguage(language: UiLanguage): UiLanguage {
  const index = languageOptions.findIndex(option => option.code === language);
  return languageOptions[(index + 1) % languageOptions.length].code;
}
export function languageBadge(language: UiLanguage): string {
  return languageOptions.find(option => option.code === language)?.label ?? "中";
}
export function languageSwitchTitle(language: UiLanguage): string {
  return languageOptions.find(option => option.code === nextUiLanguage(language))?.name ?? "简体中文";
}
export function cycleUiLanguage(current: UiLanguage): void {
  setUiLanguage(nextUiLanguage(current));
}
const byEnglish = new Map<string, MessageKey>(
  (Object.entries(enMessages) as Array<[MessageKey, string]>).map(([key, value]) => [value, key]),
);
export const uiLanguage = writable<UiLanguage>("zh-CN");

export function restoreUiLanguage(): void {
  try {
    const selected = localStorage.getItem(STORAGE_KEY);
    const option = languageOptions.find(item => item.code === selected);
    if (option) uiLanguage.set(option.code);
  } catch { /* Retain the Chinese default. */ }
}
export function setUiLanguage(language: UiLanguage): void {
  uiLanguage.set(language);
  try { localStorage.setItem(STORAGE_KEY, language); } catch { /* In-memory choice still works. */ }
}
/** Page-authored text uses stable keys; firmware values never pass this interface. */
export function translate(key: MessageKey, language: UiLanguage): string {
  return locales[language]?.[key] ?? enMessages[key];
}
/** External HostSchema label compatibility: unknown labels remain unchanged. */
export function localize(value: string, language: UiLanguage): string {
  const key = byEnglish.get(value);
  return key ? translate(key, language) : value;
}
/** Translate known device status text while preserving unknown diagnostics and raw codes. */
export function localizeFeedback(value: string, language: UiLanguage): string {
  if (language === "en") return value;
  if (value.includes("\n")) return value.split("\n").map(line => localizeFeedback(line, language)).join("\n");
  const key = byEnglish.get(value);
  if (key) return translate(key, language);
  for (const [english, token] of byEnglish) {
    if (english.endsWith(": ") && value.startsWith(english))
      return translate(token, language) + value.slice(english.length);
  }
  return value;
}
