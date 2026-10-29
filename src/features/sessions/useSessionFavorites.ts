import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { createSessionFavoritesStore, EMPTY_FAVORITES, SESSION_FAVORITES_STORAGE_KEY } from "../../lib/session-favorites";
import { getNativeFavoritesStore } from "../../lib/native-favorites-service";
import { usePlatformCapabilities } from "../../lib/platform-service";

export function useSessionFavorites() {
  const platform = usePlatformCapabilities();
  const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
    && platform.data?.mobile !== true && platform.data?.os !== "browser";
  const [browserStore] = useState(() => createSessionFavoritesStore(() => typeof window === "undefined" ? undefined : window.localStorage));
  const store = useMemo(() => native ? getNativeFavoritesStore() : browserStore, [native, browserStore]);
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot, () => EMPTY_FAVORITES);
  useEffect(() => {
    const synchronize = (event: StorageEvent) => {
      if (event.key === SESSION_FAVORITES_STORAGE_KEY || event.key === null) void store.refresh();
    };
    window.addEventListener("storage", synchronize);
    void store.refresh();
    return () => window.removeEventListener("storage", synchronize);
  }, [store]);
  return { ...snapshot, toggle: store.toggle, remove: store.remove, retry: () => { void store.refresh(); } };
}
