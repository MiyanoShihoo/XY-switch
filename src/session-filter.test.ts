import { describe, expect, it } from "vitest";
import { filterSessions, type SessionFilters } from "./lib/session-filter";
import { sessionKey } from "./lib/session-format";
import type { SessionMeta } from "./types/session";

const old: SessionMeta = { providerId: "codex", sessionId: "old", title: "Build workspace", lastActiveAt: 1_790_000_000, projectDir: "QA:/project" };
const recent: SessionMeta = { providerId: "claude", sessionId: "new", summary: "Review results", lastActiveAt: 1_790_000_100_000 };
const missing: SessionMeta = { providerId: "codex", sessionId: "missing" };
const filters = (changes: Partial<SessionFilters> = {}): SessionFilters => ({ app: "all", search: "", favoritesOnly: false, favoriteKeys: new Set(), ...changes });

describe("session browsing filters", () => {
  it("puts favorites before recent non-favorites without mutating the catalog", () => {
    const input = [recent, old, missing];
    expect(filterSessions(input, filters({ favoriteKeys: new Set([sessionKey(old)]) }))).toEqual([old, recent, missing]);
    expect(input).toEqual([recent, old, missing]);
  });
  it("sorts recent activity within the favorite group, handling seconds and milliseconds", () => {
    expect(filterSessions([old, missing, recent], filters({ favoriteKeys: new Set([sessionKey(old), sessionKey(recent)]) }))).toEqual([recent, old, missing]);
  });
  it("combines client, text and favorites filters", () => {
    const favoriteKeys = new Set([sessionKey(old), sessionKey(recent)]);
    expect(filterSessions([old, recent, missing], filters({ app: "codex", search: "  WORKSPACE  ", favoritesOnly: true, favoriteKeys }))).toEqual([old]);
    expect(filterSessions([old, recent], filters({ app: "claude", search: "workspace", favoritesOnly: true, favoriteKeys }))).toEqual([]);
  });
  it.each(["BUILD", "old", "qa:/PROJECT"])("finds the title, identifier or project directory: %s", search => {
    expect(filterSessions([old, recent, missing], filters({ search }))).toEqual([old]);
  });
  it("searches summaries and tolerates missing optional metadata", () => {
    expect(filterSessions([missing, recent], filters({ search: "review" }))).toEqual([recent]);
    expect(filterSessions([missing], filters({ search: " " }))).toEqual([missing]);
  });
  it("does not insert unavailable favorites into the catalog or conflate clients", () => {
    const sameId = { ...old, providerId: "claude" };
    const favoriteKeys = new Set([sessionKey(old), sessionKey({ providerId: "gemini", sessionId: "absent" })]);
    expect(filterSessions([old, sameId, recent], filters({ favoritesOnly: true, favoriteKeys }))).toEqual([old]);
  });
  it("sorts equal or missing timestamps deterministically across refreshes", () => {
    const a = { providerId: "codex", sessionId: "a", createdAt: 0 }, b = { providerId: "codex", sessionId: "b", lastActiveAt: Number.NaN };
    expect(filterSessions([b, a], filters())).toEqual([a, b]);
    expect(filterSessions([a, b], filters())).toEqual([a, b]);
  });
  it("uses created time when the last-activity timestamp is absent", () => {
    const created = { ...recent, lastActiveAt: null, createdAt: recent.lastActiveAt };
    expect(filterSessions([old, created], filters())).toEqual([created, old]);
  });
});
