import { describe, expect, it, vi } from "vitest";
import { createSessionFavoritesStore, parseSessionFavorites, serializeSessionFavorites, SESSION_FAVORITES_STORAGE_KEY } from "./lib/session-favorites";
import { sessionKey } from "./lib/session-format";

const codex = { providerId: "codex", sessionId: "same-id" };
const claude = { providerId: "claude", sessionId: "same-id" };
const other = { providerId: "gemini", sessionId: "other-id" };
const keys = (...sessions: typeof codex[]) => new Set(sessions.map(sessionKey));
function memory(raw: string | null = null) {
  const state = { raw, failRead: false, failWrite: false };
  const storage = {
    getItem: vi.fn((_key: string) => { if (state.failRead) throw new Error("Storage denied"); return state.raw; }),
    setItem: vi.fn((_key: string, value: string) => { if (state.failWrite) throw new Error("Quota exceeded"); state.raw = value; }),
  };
  return { state, storage, create: () => createSessionFavoritesStore(() => storage) };
}

describe("session favorites format", () => {
  it("treats an absent preference as an empty collection", () => {
    expect(parseSessionFavorites(null)).toEqual(new Set());
  });
  it("round-trips client-scoped identifiers deterministically", () => {
    expect(parseSessionFavorites(serializeSessionFavorites(keys(codex, claude)))).toEqual(keys(codex, claude));
    expect(serializeSessionFavorites(keys(codex, claude))).toBe(serializeSessionFavorites(keys(claude, codex)));
  });
  it("deduplicates saved identifiers", () => {
    expect(parseSessionFavorites(JSON.stringify({ version: 1, keys: [sessionKey(codex), sessionKey(codex)] }))).toEqual(keys(codex));
  });
  it("does not confuse punctuation inside the client/session pair", () => {
    const first = { providerId: "a:b", sessionId: "c" }, second = { providerId: "a", sessionId: "b:c" };
    expect(parseSessionFavorites(serializeSessionFavorites(keys(first, second))).size).toBe(2);
  });
  it.each(["", "{broken", "null", "[]", '{"version":2,"keys":[]}', '{"version":1,"keys":[123]}',
    JSON.stringify({ version: 1, keys: [JSON.stringify(["codex", ""])] }),
    JSON.stringify({ version: 1, keys: [JSON.stringify(["codex", "id", "extra"])] }),
    JSON.stringify({ version: 1, keys: ['[ "codex", "id" ]'] }),
  ])("rejects unreadable or unsupported data without silently discarding it: %s", raw => {
    expect(() => parseSessionFavorites(raw)).toThrow();
  });
  it("rejects a non-identifier when serializing", () => {
    expect(() => serializeSessionFavorites(new Set(["title or path"]))).toThrow();
  });
});

describe("session favorites persistence", () => {
  it("loads existing favorites without rewriting storage", () => {
    const data = memory(serializeSessionFavorites(keys(codex)));
    const store = data.create();
    expect(store.getSnapshot()).toEqual({ keys: keys(codex), issue: null, unsaved: false });
    expect(data.storage.getItem).toHaveBeenCalledWith(SESSION_FAVORITES_STORAGE_KEY);
    expect(data.storage.setItem).not.toHaveBeenCalled();
  });
  it("saves only identifiers, not private session metadata", () => {
    const data = memory(), store = data.create();
    store.toggle({ ...codex, title: "Private title", projectDir: "QA:/private", sourcePath: "QA:/session.jsonl" } as typeof codex);
    expect(JSON.parse(data.state.raw!)).toEqual({ version: 1, keys: [sessionKey(codex)] });
    expect(data.state.raw).not.toMatch(/Private|QA:|title|projectDir|sourcePath/);
  });
  it("toggles and removes only the intended client-scoped favorite", () => {
    const data = memory(), store = data.create();
    store.toggle(codex); store.toggle(claude); store.toggle(codex);
    expect(store.getSnapshot().keys).toEqual(keys(claude));
    store.remove(claude);
    expect(parseSessionFavorites(data.state.raw)).toEqual(new Set());
  });
  it("preserves favorites outside the current catalog", () => {
    const data = memory(serializeSessionFavorites(keys(other))), store = data.create();
    store.toggle(codex);
    expect(parseSessionFavorites(data.state.raw)).toEqual(keys(codex, other));
  });
  it("merges a change saved by another window before the next local action", () => {
    const data = memory(), store = data.create();
    data.state.raw = serializeSessionFavorites(keys(other));
    store.toggle(codex);
    expect(parseSessionFavorites(data.state.raw)).toEqual(keys(codex, other));
  });
  it("uses a stable snapshot and only notifies subscribers on a change", () => {
    const data = memory(), store = data.create(), listener = vi.fn();
    const initial = store.getSnapshot(), unsubscribe = store.subscribe(listener);
    store.refresh();
    expect(store.getSnapshot()).toBe(initial);
    expect(listener).not.toHaveBeenCalled();
    store.toggle(codex);
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe(); store.toggle(other);
    expect(listener).toHaveBeenCalledTimes(1);
  });
  it("refreshes removals and local-storage clearing without an echo write", () => {
    const data = memory(serializeSessionFavorites(keys(codex))), store = data.create();
    data.state.raw = null; store.refresh();
    expect(store.getSnapshot().keys.size).toBe(0);
    expect(data.storage.setItem).not.toHaveBeenCalled();
  });
  it("keeps failed writes in memory and retries without losing other-window changes", () => {
    const data = memory(serializeSessionFavorites(keys(codex))), store = data.create();
    data.state.failWrite = true; store.toggle(claude); store.remove(codex);
    expect(store.getSnapshot()).toEqual({ keys: keys(claude), issue: "unavailable", unsaved: true });
    expect(parseSessionFavorites(data.state.raw)).toEqual(keys(codex));
    data.state.raw = serializeSessionFavorites(keys(codex, other)); data.state.failWrite = false;
    store.refresh();
    expect(store.getSnapshot()).toEqual({ keys: keys(claude, other), issue: null, unsaved: false });
    expect(parseSessionFavorites(data.state.raw)).toEqual(keys(claude, other));
  });
  it("does not overwrite unread storage when getItem fails", () => {
    const data = memory(serializeSessionFavorites(keys(other)));
    data.state.failRead = true;
    const store = data.create(); store.toggle(codex);
    expect(store.getSnapshot()).toEqual({ keys: keys(codex), issue: "unavailable", unsaved: true });
    expect(data.storage.setItem).not.toHaveBeenCalled();
    data.state.failRead = false; store.refresh();
    expect(parseSessionFavorites(data.state.raw)).toEqual(keys(codex, other));
  });
  it("retains pending edits if storage is missing or its getter throws", () => {
    for (const get of [() => undefined, () => { throw new Error("Denied"); }]) {
      const store = createSessionFavoritesStore(get); store.toggle(codex);
      expect(store.getSnapshot()).toEqual({ keys: keys(codex), issue: "unavailable", unsaved: true });
    }
  });
  it("does not overwrite corrupt data or a newer schema during a toggle or retry", () => {
    const original = '{"version":2,"keys":["future-format"]}', data = memory(original), store = data.create();
    store.toggle(codex); store.refresh();
    expect(data.state.raw).toBe(original);
    expect(data.storage.setItem).not.toHaveBeenCalled();
    expect(store.getSnapshot()).toEqual({ keys: keys(codex), issue: "invalid", unsaved: true });
    data.state.raw = serializeSessionFavorites(keys(other)); store.refresh();
    expect(parseSessionFavorites(data.state.raw)).toEqual(keys(codex, other));
  });
  it("does not accept an invalid session pair", () => {
    const data = memory(), store = data.create();
    expect(() => store.toggle({ providerId: "codex", sessionId: "" })).toThrow();
    expect(data.storage.setItem).not.toHaveBeenCalled();
  });
});
