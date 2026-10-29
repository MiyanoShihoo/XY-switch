import { useEffect, useState, useSyncExternalStore } from "react";
import { createSessionFavoritesStore, EMPTY_FAVORITES, SESSION_FAVORITES_STORAGE_KEY } from "../../lib/session-favorites";

export function useSessionFavorites() {
  const [store] = useState(() => createSessionFavoritesStore(() => typeof window === "undefined" ? undefined : window.localStorage));
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot, () => EMPTY_FAVORITES);
  useEffect(() => {
    const synchronize = (event: StorageEvent) => {
      if (event.key === SESSION_FAVORITES_STORAGE_KEY || event.key === null) store.refresh();
    };
    window.addEventListener("storage", synchronize);
    // Also catch changes made while this page was not mounted.
    store.refresh();
    return () => window.removeEventListener("storage", synchronize);
  }, [store]);
  return { ...snapshot, toggle: store.toggle, remove: store.remove, retry: store.refresh };
}
