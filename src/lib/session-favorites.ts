import { sessionKey } from "./session-format";
import type { SessionIdPair } from "../types/session";

export const SESSION_FAVORITES_STORAGE_KEY = "xy-switch.session-favorites.v1";
export type FavoritesStorage = Pick<Storage, "getItem" | "setItem">;
export type FavoritesIssue = "unavailable" | "invalid" | null;
export interface FavoritesSnapshot {
  keys: ReadonlySet<string>;
  issue: FavoritesIssue;
  unsaved: boolean;
}

export const EMPTY_FAVORITES: FavoritesSnapshot = { keys: new Set(), issue: null, unsaved: false };

function validKey(key: unknown): key is string {
  if (typeof key !== "string") return false;
  try {
    const pair: unknown = JSON.parse(key);
    return Array.isArray(pair) && pair.length === 2
      && pair.every(value => typeof value === "string" && value.trim().length > 0)
      && JSON.stringify(pair) === key;
  } catch { return false; }
}

/** Only client/session identifiers are stored, never titles, paths or message bodies. */
export function parseSessionFavorites(raw: string | null): Set<string> {
  if (raw === null) return new Set();
  const value: unknown = JSON.parse(raw);
  if (!value || typeof value !== "object" || !("version" in value) || value.version !== 1
    || !("keys" in value) || !Array.isArray(value.keys) || !value.keys.every(validKey)) {
    throw new Error("Invalid session favorites format");
  }
  return new Set(value.keys);
}

export function serializeSessionFavorites(keys: ReadonlySet<string>): string {
  if (![...keys].every(validKey)) throw new Error("Invalid session identifier");
  return JSON.stringify({ version: 1, keys: [...keys].sort() });
}

/** Pending edits are merged with the latest saved keys before retrying a failed write. */
export function createSessionFavoritesStore(getStorage: () => FavoritesStorage | undefined) {
  let snapshot = EMPTY_FAVORITES;
  const listeners = new Set<() => void>();
  const pending = new Map<string, boolean>();

  function withPending(keys: ReadonlySet<string>) {
    const next = new Set(keys);
    for (const [key, favorite] of pending) {
      if (favorite) next.add(key); else next.delete(key);
    }
    return next;
  }

  function publish(keys: ReadonlySet<string>, issue: FavoritesIssue) {
    const unsaved = pending.size > 0;
    if (snapshot.issue === issue && snapshot.unsaved === unsaved
      && snapshot.keys.size === keys.size && [...keys].every(key => snapshot.keys.has(key))) return;
    snapshot = { keys, issue, unsaved };
    listeners.forEach(listener => listener());
  }

  function refresh() {
    let storage: FavoritesStorage;
    let raw: string | null;
    try {
      const available = getStorage();
      if (!available) throw new Error("Storage unavailable");
      storage = available;
      raw = storage.getItem(SESSION_FAVORITES_STORAGE_KEY);
    } catch {
      publish(withPending(snapshot.keys), "unavailable");
      return;
    }
    let saved: Set<string>;
    try { saved = parseSessionFavorites(raw); }
    catch {
      // Do not overwrite corrupt data or a newer schema with an empty collection.
      publish(withPending(snapshot.keys), "invalid");
      return;
    }
    const next = withPending(saved);
    if (pending.size) {
      try { storage.setItem(SESSION_FAVORITES_STORAGE_KEY, serializeSessionFavorites(next)); }
      catch { publish(next, "unavailable"); return; }
      pending.clear();
    }
    publish(next, null);
  }

  function setFavorite(session: SessionIdPair, favorite: boolean) {
    const key = sessionKey(session);
    if (!validKey(key)) throw new Error("Invalid session identifier");
    pending.set(key, favorite);
    refresh();
  }

  refresh();
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    refresh,
    toggle(session: SessionIdPair) { setFavorite(session, !snapshot.keys.has(sessionKey(session))); },
    remove(session: SessionIdPair) { setFavorite(session, false); },
  };
}
