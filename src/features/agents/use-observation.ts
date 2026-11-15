import { useEffect, useLayoutEffect, useState, useSyncExternalStore } from "react";
import type { AgentSnapshotQuery } from "../../types/agents";
import { createObservationStore } from "./observation-store";

export function useObservation(query: AgentSnapshotQuery) {
  const [store] = useState(() => createObservationStore());
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  useEffect(() => {
    store.start(query);
    const reconcile = () => { if (document.visibilityState === "visible") store.refresh(); };
    document.addEventListener("visibilitychange", reconcile);
    return () => { document.removeEventListener("visibilitychange", reconcile); store.stop(); };
  }, [store]);
  useLayoutEffect(() => { store.setQuery(query); }, [store, query.teamId, query.at]);
  return { ...state, refresh: store.refresh };
}

export function useObservationClock(at: number | null, frozenAt: number | null) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (at != null || frozenAt != null) return;
    setNow(Date.now());
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, [at, frozenAt]);
  return at ?? frozenAt ?? now;
}
