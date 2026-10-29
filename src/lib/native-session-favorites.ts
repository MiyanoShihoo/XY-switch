import { sessionKey } from "./session-format";
import { EMPTY_FAVORITES, type FavoritesIssue, type FavoritesSnapshot } from "./session-favorites";
import type { SessionIdPair } from "../types/session";

export interface NativeFavoritesDriver {
  load(): Promise<SessionIdPair[]>;
  set(favorite: SessionIdPair, enabled: boolean): Promise<void>;
}

/** One serial queue; optimistic edits survive late loads, failed writes and page changes. */
export function createNativeSessionFavoritesStore(driver: NativeFavoritesDriver) {
  let snapshot = EMPTY_FAVORITES;
  let saved = new Set<string>();
  const pending = new Map<string, { favorite: SessionIdPair; enabled: boolean }>();
  const listeners = new Set<() => void>();
  let reload = true;
  let running: Promise<void> | undefined;
  function publish(issue: FavoritesIssue) {
    const keys = new Set(saved);
    for (const [key, edit] of pending) { if (edit.enabled) keys.add(key); else keys.delete(key); }
    snapshot = { keys, issue, unsaved: pending.size > 0 };
    for (const listener of listeners) listener();
  }
  function pump(): Promise<void> {
    if (running) return running;
    // Start after assigning running, including when the queue is initially empty.
    let completed = false;
    running = Promise.resolve().then(async () => {
      try {
        while (reload || pending.size) {
          if (reload) {
            reload = false;
            saved = new Set((await driver.load()).map(sessionKey));
            publish(null);
          }
          const first = pending.entries().next().value;
          if (!first) continue;
          const [key, edit] = first;
          await driver.set(edit.favorite, edit.enabled);
          if (edit.enabled) saved.add(key); else saved.delete(key);
          if (pending.get(key) === edit) pending.delete(key);
          publish(null);
        }
        completed = true;
      } catch (error) {
        reload = true;
        publish(String(error).includes("Invalid legacy favorites data") ? "invalid" : "unavailable");
      }
    }).finally(() => {
      running = undefined;
      if (completed && (reload || pending.size)) return pump();
    });
    return running;
  }
  function setFavorite(favorite: SessionIdPair, enabled: boolean) {
    pending.set(sessionKey(favorite), { favorite: { providerId: favorite.providerId, sessionId: favorite.sessionId }, enabled });
    publish(snapshot.issue);
    void pump();
  }
  return {
    getSnapshot: (): FavoritesSnapshot => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    async refresh() { reload = true; await pump(); },
    toggle(favorite: SessionIdPair) { setFavorite(favorite, !snapshot.keys.has(sessionKey(favorite))); },
    remove(favorite: SessionIdPair) { setFavorite(favorite, false); },
  };
}
