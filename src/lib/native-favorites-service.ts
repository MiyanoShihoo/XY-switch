import { invoke } from "@tauri-apps/api/core";
import { createNativeSessionFavoritesStore } from "./native-session-favorites";
import { SESSION_FAVORITES_STORAGE_KEY } from "./session-favorites";
import type { SessionIdPair } from "../types/session";

let nativeStore: ReturnType<typeof createNativeSessionFavoritesStore> | undefined;
export function getNativeFavoritesStore() {
  nativeStore ??= createNativeSessionFavoritesStore({
    async load() {
      let legacy: string | null = null, legacyAvailable = false;
      try { legacy = window.localStorage.getItem(SESSION_FAVORITES_STORAGE_KEY); legacyAvailable = true; }
      catch { /* Backend only permits this if an earlier migration already succeeded. */ }
      return invoke<SessionIdPair[]>("load_session_favorites", { legacy, legacyAvailable });
    },
    async set(favorite, enabled) { await invoke("set_session_favorite", { favorite, enabled }); },
  });
  return nativeStore;
}
export async function prepareNativeFavorites() {
  const store = getNativeFavoritesStore();
  await store.refresh();
  const snapshot = store.getSnapshot();
  if (snapshot.issue || snapshot.unsaved) throw new Error("Local favorites could not be synchronized; retry before encrypted file exchange");
}
export async function refreshNativeFavorites() { await prepareNativeFavorites(); }
