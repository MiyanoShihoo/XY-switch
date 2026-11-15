import { useCallback, useEffect, useRef, useState } from "react";
import { getAgentEvents } from "../../lib/agent-service";
import type { AgentEvent, AgentEventQuery } from "../../types/agents";
import { errorMessage } from "./format";

interface EventState { key: string; events: AgentEvent[]; total: number | null; cursor: string | null; loading: boolean; error: string | null; expanded: boolean; hasUpdates: boolean }
const empty = (key: string): EventState => ({ key, events: [], total: null, cursor: null, loading: false, error: null, expanded: false, hasUpdates: false });

export function useEvents(query: AgentEventQuery, enabled: boolean, changeVersion: number, replayAt: number | null = null) {
  const key = JSON.stringify([query.teamId, query.agentId, query.taskId, query.since, query.until, replayAt]);
  const current = useRef({ query, enabled, key, changeVersion, replayAt });
  current.current = { query, enabled, key, changeVersion, replayAt };
  const mounted = useRef(false), sequence = useRef(0), busy = useRef(false);
  const stateRef = useRef<EventState>(empty(key));
  const [state, setState] = useState(stateRef.current);
  const deadline = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const update = useCallback((value: EventState) => { stateRef.current = value; setState(value); }, []);
  const request = useCallback((more = false) => {
    const scope = current.current;
    if (!scope.enabled || !mounted.current || (more && busy.current)) return;
    const previous = stateRef.current.key === scope.key ? stateRef.current : empty(scope.key);
    if (more && !previous.cursor) return;
    const token = ++sequence.current;
    if (deadline.current) clearTimeout(deadline.current);
    busy.current = true;
    update({ ...previous, loading: true, error: null });
    const valid = () => mounted.current && token === sequence.current && scope.key === current.current.key && current.current.enabled;
    deadline.current = setTimeout(() => {
      if (!valid()) return;
      ++sequence.current; busy.current = false;
      update({ ...stateRef.current, loading: false, error: "AGENT_READ_TIMEOUT" });
    }, 15000);
    void getAgentEvents({ ...scope.query, cursor: more ? previous.cursor ?? undefined : undefined, limit: 60 }, { at: scope.replayAt }).then(page => {
      if (!valid()) return;
      if (deadline.current) clearTimeout(deadline.current);
      busy.current = false;
      const events = new Map((more ? previous.events : []).map(event => [event.id, event]));
      page.events.forEach(event => events.set(event.id, event));
      update({ key: scope.key, events: [...events.values()].sort((left, right) => right.timestamp - left.timestamp || right.id.localeCompare(left.id)),
        total: page.total, cursor: page.nextCursor, loading: false, error: null, expanded: more,
        hasUpdates: scope.changeVersion !== current.current.changeVersion || (more && stateRef.current.hasUpdates) });
    }).catch(error => {
      if (!valid()) return;
      if (deadline.current) clearTimeout(deadline.current);
      busy.current = false;
      update({ ...stateRef.current, loading: false, error: errorMessage(error) });
    });
  }, [update]);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; ++sequence.current; if (deadline.current) clearTimeout(deadline.current); };
  }, []);
  useEffect(() => {
    ++sequence.current; busy.current = false;
    if (deadline.current) clearTimeout(deadline.current);
    if (stateRef.current.key !== key) update(empty(key));
    else update({ ...stateRef.current, loading: false });
    if (enabled) request();
  }, [key, enabled, request, update]);
  useEffect(() => {
    if (!enabled || !changeVersion) return;
    if (busy.current || stateRef.current.expanded) {
      update({ ...stateRef.current, hasUpdates: true });
      return;
    }
    const timer = setTimeout(() => request(), 350);
    return () => clearTimeout(timer);
  }, [changeVersion, enabled, request, update]);
  return { ...(state.key === key ? state : empty(key)), refresh: () => request(), loadMore: () => request(true) };
}
