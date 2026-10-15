import { writable } from "svelte/store";
import { enMessages } from "./i18n/locales/en";
import { zhCNMessages } from "./i18n/locales/zh-CN";

export type UiLanguage = "zh-CN" | "en";
export type MessageKey = keyof typeof enMessages;
const STORAGE_KEY = "nmixx.ui.language";
const locales: Record<UiLanguage, Record<MessageKey, string>> = { "zh-CN": zhCNMessages, en: enMessages };
const byEnglish = new Map<string, MessageKey>(
  (Object.entries(enMessages) as Array<[MessageKey, string]>).map(([key, value]) => [value, key]),
);
export const uiLanguage = writable<UiLanguage>("zh-CN");

export function restoreUiLanguage(): void {
  try {
    const selected = localStorage.getItem(STORAGE_KEY);
    if (selected === "zh-CN" || selected === "en") uiLanguage.set(selected);
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
