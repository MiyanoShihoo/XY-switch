import { sessionKey, timestampMs } from "./session-format";
import type { SessionMeta } from "../types/session";

export interface SessionFilters {
  search: string;
  app: string;
  favoritesOnly: boolean;
  favoriteKeys: ReadonlySet<string>;
}

export function filterSessions(sessions: readonly SessionMeta[], filters: SessionFilters): SessionMeta[] {
  const needle = filters.search.trim().toLowerCase();
  return sessions.filter(session => (filters.app === "all" || session.providerId === filters.app)
    && (!filters.favoritesOnly || filters.favoriteKeys.has(sessionKey(session)))
    && [session.title, session.summary, session.sessionId, session.projectDir]
      .some(value => (value ?? "").toLowerCase().includes(needle)))
    .sort((left, right) => {
      const favoriteOrder = Number(filters.favoriteKeys.has(sessionKey(right))) - Number(filters.favoriteKeys.has(sessionKey(left)));
      if (favoriteOrder) return favoriteOrder;
      const activityOrder = timestampMs(right.lastActiveAt ?? right.createdAt) - timestampMs(left.lastActiveAt ?? left.createdAt);
      if (activityOrder) return activityOrder;
      // Stable tie-breaking avoids row jumps when a refreshed catalog changes order.
      const leftKey = sessionKey(left), rightKey = sessionKey(right);
      return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
    });
}
