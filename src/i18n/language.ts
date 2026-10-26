import { useSyncExternalStore } from "react";
import { en } from "./en";

export type Language = "zh-CN" | "en";
export type LanguagePreference = Language | "system";
export type LanguageSyncError = "read" | "write" | null;
export const LANGUAGE_STORAGE_KEY = "xy-switch:language";
export const languages = ["system", "zh-CN", "en"] as const;

const listeners = new Set<() => void>();
let preference: LanguagePreference = readPreference();
let revision = 0;
let nativeInitialized = false;
let initialization: Promise<void> | undefined;
let nativeWriteTail: Promise<void> = Promise.resolve();
let syncError: LanguageSyncError = null;

export function normalizeLanguagePreference(value: unknown): LanguagePreference {
  return value === "zh-CN" || value === "en" ? value : "system";
}
export function resolveLanguage(value: LanguagePreference, systemLanguages: readonly string[] = typeof navigator === "undefined" ? ["zh-CN"] : navigator.languages?.length ? navigator.languages : typeof navigator.language === "string" ? [navigator.language] : ["zh-CN"]): Language {
  if (value !== "system") return value;
  return systemLanguages[0]?.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}
function readPreference(): LanguagePreference {
  try { return normalizeLanguagePreference(typeof localStorage === "undefined" ? null : localStorage.getItem(LANGUAGE_STORAGE_KEY)); }
  catch { return "system"; }
}
function isNative(): boolean { return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window; }
export function languagePreference(): LanguagePreference { return preference; }
export function currentLanguage(): Language { return resolveLanguage(preference); }
export function localeTag(): string { return currentLanguage() === "en" ? "en-US" : "zh-CN"; }
function emit() {
  if (typeof document !== "undefined") document.documentElement.lang = currentLanguage();
  for (const listener of listeners) listener();
}
function subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }
export function useLanguage(): Language { return useSyncExternalStore(subscribe, currentLanguage, currentLanguage); }
export function useLanguagePreference(): LanguagePreference { return useSyncExternalStore(subscribe, languagePreference, languagePreference); }
export function useLanguageSyncError(): LanguageSyncError { return useSyncExternalStore(subscribe, () => syncError, () => null); }
function setSyncError(next: LanguageSyncError) { syncError = next; emit(); }
function apply(next: LanguagePreference) {
  preference = next; revision += 1;
  try { if (typeof localStorage !== "undefined") localStorage.setItem(LANGUAGE_STORAGE_KEY, next); }
  catch { /* Native preferences remain authoritative when WebView storage is restricted. */ }
  emit();
}
function enqueueNativeWrite(work: () => Promise<void>): Promise<void> {
  const result = nativeWriteTail.then(work);
  nativeWriteTail = result.catch(() => undefined);
  return result;
}
async function persistNative(next: LanguagePreference): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_interface_language", { preference: next, resolvedLanguage: resolveLanguage(next) });
}
/** Persist before announcing a change. Serial writes retain the last successful selection. */
export async function setLanguagePreference(next: LanguagePreference): Promise<void> {
  if (!languages.includes(next)) throw new Error("Unsupported interface language");
  revision += 1; // A pending selection must already invalidate late startup hydration.
  if (!isNative()) { apply(next); return; }
  await enqueueNativeWrite(async () => {
    await persistNative(next);
    apply(next);
    nativeInitialized = true;
    setSyncError(null);
  });
}
/** Hydrate once without overwriting an in-flight user choice, then synchronize native menus. */
export async function initializeLanguage(): Promise<void> {
  emit();
  if (nativeInitialized || !isNative()) return;
  if (initialization) return initialization;
  const generation = revision;
  let phase: Exclude<LanguageSyncError, null> = "read";
  initialization = (async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const saved = await invoke<string | null>("get_interface_language");
      if (revision !== generation) return;
      if (saved !== null) apply(normalizeLanguagePreference(saved));
      const hydratedRevision = revision;
      const next = preference;
      phase = "write";
      await enqueueNativeWrite(async () => {
        if (revision === hydratedRevision) await persistNative(next);
      });
      nativeInitialized = true;
      setSyncError(null);
    } catch (error) {
      nativeInitialized = false;
      setSyncError(phase);
      console.error("Could not initialize interface language:", error);
    } finally { initialization = undefined; }
  })();
  return initialization;
}
async function synchronizeNativeLanguage(): Promise<void> {
  if (!isNative()) return;
  const next = preference;
  try {
    await enqueueNativeWrite(async () => {
      if (preference === next) await persistNative(next);
    });
    setSyncError(null);
  } catch (error) {
    setSyncError("write");
    console.error("Could not synchronize native interface language:", error);
  }
}
export async function retryLanguageSynchronization(): Promise<void> {
  if (syncError === "read") { nativeInitialized = false; await initializeLanguage(); }
  else await synchronizeNativeLanguage();
}
if (typeof window !== "undefined" && typeof window.addEventListener === "function") {
  window.addEventListener("storage", event => {
    if (event.key === LANGUAGE_STORAGE_KEY || event.key === null) {
      preference = readPreference(); revision += 1; emit();
      void synchronizeNativeLanguage();
    }
  });
  window.addEventListener("languagechange", () => {
    if (preference === "system") { emit(); void synchronizeNativeLanguage(); }
  });
}

export type TranslationValues = Record<string, string | number | null | undefined>;
export function translate(source: string, values: TranslationValues = {}, language: Language = currentLanguage()): string {
  const message = language === "en" ? en[source] ?? source : source;
  return message.replace(/\{([A-Za-z][A-Za-z0-9_]*)\}/g, (match, key: string) => Object.hasOwn(values, key) ? String(values[key] ?? "—") : match);
}
export const t = translate;
/** Resolve application-owned labels at access time, not at module initialization. */
export function localizedLabels<T extends Record<string, string>>(labels: T): T {
  return new Proxy(labels, { get(target, property, receiver) { const value = Reflect.get(target, property, receiver); return typeof value === "string" ? t(value) : value; } });
}
const reverse = new Map(Object.entries(en).map(([zh, english]) => [english, zh]));
const placeholder = /\{([A-Za-z][A-Za-z0-9_]*)\}/g;
function compileMessage(message: string) {
  const keys: string[] = [];
  const pattern = message.split(/(\{[A-Za-z][A-Za-z0-9_]*\})/).map(part => {
    if (/^\{[A-Za-z][A-Za-z0-9_]*\}$/.test(part)) { keys.push(part.slice(1, -1)); return "([\\s\\S]*?)"; }
    return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }).join("");
  return { regex: new RegExp("^" + pattern + "$"), keys };
}
const patterns = Object.entries(en).filter(([source]) => /\{[A-Za-z]/.test(source)).map(([zh, english]) => ({
  zh, source: compileMessage(zh), target: compileMessage(english),
  specificity: Math.max(zh.replace(placeholder, "").length, english.replace(placeholder, "").length),
})).sort((left, right) => right.specificity - left.specificity);
const prefixes = Object.entries(en).filter(([source]) => source.length > 3 && /[：:]\s*$/.test(source) && !source.includes("{")).sort((left, right) => Math.max(right[0].length, right[1].length) - Math.max(left[0].length, left[1].length));
/** Only for application notifications. Interpolated user values and raw diagnostic tails stay opaque. */
export function translateMessage(message: string): string {
  const source = Object.hasOwn(en, message) ? message : reverse.get(message);
  if (source !== undefined) return t(source);
  for (const pattern of patterns) {
    for (const compiled of [pattern.source, pattern.target]) {
      const match = message.match(compiled.regex);
      if (match) return t(pattern.zh, Object.fromEntries(compiled.keys.map((key, index) => [key, match[index + 1]])));
    }
  }
  for (const [zh, english] of prefixes) {
    for (const prefix of [zh, english]) {
      if (message.startsWith(prefix)) return t(zh) + message.slice(prefix.length);
    }
  }
  return message;
}

