import { getAgentSnapshot, isAgentObservationAvailable, subscribeAgentChanges } from "../../lib/agent-service";
import type { AgentSnapshot, AgentSnapshotQuery, AgentTeam } from "../../types/agents";
import { errorMessage } from "./format";

export interface ObservationState {
  snapshot: AgentSnapshot | null;
  snapshotQuery: AgentSnapshotQuery | null;
  query: AgentSnapshotQuery;
  teams: AgentTeam[];
  history: AgentSnapshot["history"] | null;
  connection: "connecting" | "connected" | "disconnected" | "native-required";
  notifications: "connecting" | "connected" | "disconnected" | "unavailable";
  loading: boolean;
  error: string | null;
  notificationError: string | null;
  lastSuccessAt: number | null;
  changeVersion: number;
}
export interface ObservationService {
  available: () => boolean;
  snapshot: (query: AgentSnapshotQuery) => Promise<AgentSnapshot>;
  listen: (callback: () => void) => Promise<() => void>;
}
const nativeService: ObservationService = { available: isAgentObservationAvailable, snapshot: getAgentSnapshot, listen: subscribeAgentChanges };
export const queryKey = (query: AgentSnapshotQuery) => JSON.stringify([query.teamId ?? null, query.at ?? null]);

export function createObservationStore(service: ObservationService = nativeService) {
  const available = service.available();
  let state: ObservationState = {
    snapshot: null, snapshotQuery: null, query: {}, teams: [], history: null,
    connection: available ? "connecting" : "native-required", notifications: available ? "connecting" : "unavailable", loading: false,
    error: null, notificationError: null, lastSuccessAt: null, changeVersion: 0,
  };
  const listeners = new Set<() => void>();
  let active = false, lifetime = 0, sequence = 0, notificationSequence = 0;
  let inFlight = false, queued = false, attaching = false;
  let unlisten: (() => void) | undefined;
  let interval: ReturnType<typeof setInterval> | undefined;
  let debounce: ReturnType<typeof setTimeout> | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  let notificationDeadline: ReturnType<typeof setTimeout> | undefined;
  const publish = (patch: Partial<ObservationState>) => {
    state = { ...state, ...patch };
    listeners.forEach(listener => listener());
  };
  const schedule = () => {
    if (!active || debounce) return;
    debounce = setTimeout(() => { debounce = undefined; refresh(); }, 180);
  };
  const refresh = (force = false) => {
    if (!active || !service.available()) return;
    if (inFlight && !force) { queued = true; return; }
    if (deadline) clearTimeout(deadline);
    if (debounce) { clearTimeout(debounce); debounce = undefined; }
    const request = ++sequence, run = lifetime, query = { ...state.query };
    inFlight = true; queued = false;
    publish({ loading: true, error: null, connection: state.snapshot ? state.connection : "connecting" });
    const current = () => active && run === lifetime && request === sequence;
    const finish = () => {
      if (deadline) clearTimeout(deadline);
      deadline = undefined; inFlight = false;
      if (queued) { queued = false; schedule(); }
    };
    deadline = setTimeout(() => {
      if (!current()) return;
      ++sequence;
      finish();
      publish({ loading: false, connection: "disconnected", error: "AGENT_READ_TIMEOUT" });
    }, 15000);
    void Promise.resolve().then(() => service.snapshot(query)).then(snapshot => {
      if (!current()) return;
      finish();
      const teams = query.teamId || query.at != null ? new Map(state.teams.map(team => [team.id, team])) : new Map<string, AgentTeam>();
      snapshot.teams.forEach(team => teams.set(team.id, team));
      const changed = state.snapshot?.revision !== snapshot.revision;
      publish({ snapshot, snapshotQuery: query, loading: false, connection: "connected", error: null,
        changeVersion: state.changeVersion + (changed ? 1 : 0),
        lastSuccessAt: Date.now(), teams: [...teams.values()], history: query.at == null ? snapshot.history : state.history ?? snapshot.history });
    }).catch(error => {
      if (!current()) return;
      finish();
      publish({ loading: false, connection: "disconnected", error: errorMessage(error) });
    });
  };
  const attach = () => {
    if (!active || !service.available() || attaching || unlisten) return;
    attaching = true;
    const run = lifetime, attempt = ++notificationSequence;
    publish({ notifications: "connecting" });
    notificationDeadline = setTimeout(() => {
      if (!active || run !== lifetime || attempt !== notificationSequence) return;
      ++notificationSequence; attaching = false; notificationDeadline = undefined;
      publish({ notifications: "disconnected", notificationError: "AGENT_READ_TIMEOUT" });
    }, 15000);
    void Promise.resolve().then(() => service.listen(() => {
      if (!active || run !== lifetime || attempt !== notificationSequence) return;
      publish({ changeVersion: state.changeVersion + 1 });
      if (state.query.at == null) schedule();
    })).then(dispose => {
      if (!active || run !== lifetime || attempt !== notificationSequence) { dispose(); return; }
      if (notificationDeadline) clearTimeout(notificationDeadline);
      notificationDeadline = undefined;
      attaching = false; unlisten = dispose;
      publish({ notifications: "connected", notificationError: null });
      schedule();
    }).catch(error => {
      if (!active || run !== lifetime || attempt !== notificationSequence) return;
      if (notificationDeadline) clearTimeout(notificationDeadline);
      notificationDeadline = undefined;
      attaching = false;
      publish({ notifications: "disconnected", notificationError: errorMessage(error) });
    });
  };
  return {
    getSnapshot: () => state,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    start(query: AgentSnapshotQuery) {
      if (active) return;
      active = true; ++lifetime;
      publish({ query: { ...query } });
      if (!service.available()) {
        publish({ connection: "native-required", notifications: "unavailable", loading: false });
        return;
      }
      attach(); refresh();
      interval = setInterval(() => { attach(); refresh(); }, 30000);
    },
    setQuery(query: AgentSnapshotQuery) {
      if (queryKey(query) === queryKey(state.query)) return;
      publish({ query: { ...query } });
      refresh(true);
    },
    refresh() { attach(); refresh(true); },
    stop() {
      active = false; ++lifetime; ++sequence; ++notificationSequence;
      if (interval) clearInterval(interval);
      if (debounce) clearTimeout(debounce);
      if (deadline) clearTimeout(deadline);
      if (notificationDeadline) clearTimeout(notificationDeadline);
      interval = undefined; debounce = undefined; deadline = undefined;
      notificationDeadline = undefined;
      unlisten?.(); unlisten = undefined;
      attaching = false; inFlight = false; queued = false;
    },
  };
}
