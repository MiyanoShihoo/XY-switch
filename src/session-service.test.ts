import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { deleteSessions, getSessionCatalog, getSessionDetail, getSessionMessages, listSessions } from "./lib/session-service";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
beforeEach(() => { invoke.mockReset(); });
afterEach(() => vi.unstubAllGlobals());

const ids = [{ providerId: "codex", sessionId: "session-1" }];
const calls = [
  ["catalog", () => getSessionCatalog()],
  ["list", () => listSessions()],
  ["detail", () => getSessionDetail("codex", "session-1")],
  ["messages", () => getSessionMessages("codex", "session-1")],
  ["delete", () => deleteSessions(ids)],
] as const;

describe("native session IPC", () => {
  it.each(calls)("rejects browser-only %s instead of returning fake empty data", async (_name, call) => {
    vi.stubGlobal("window", {});
    await expect(call()).rejects.toThrow("本地会话需要原生桌面程序");
    expect(invoke).not.toHaveBeenCalled();
  });
  it("is safe without a window and detects the native bridge at call time", async () => {
    vi.stubGlobal("window", undefined);
    await expect(getSessionCatalog()).rejects.toThrow("本地会话需要原生桌面程序");
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const catalog = { sessions: [], warnings: ["Skipped an incomplete log"] };
    invoke.mockResolvedValue(catalog);
    expect(await getSessionCatalog()).toEqual(catalog);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("get_session_catalog");
  });
  it("preserves messages and warnings and uses camelCase native arguments", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const detail = { messages: [{ role: "tool", content: "tool result", ts: null }], warnings: ["Trailing log entry is incomplete"] };
    invoke.mockResolvedValue(detail);
    expect(await getSessionDetail("claude", "session-2")).toEqual(detail);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("get_session_detail", { providerId: "claude", sessionId: "session-2" });
  });
  it("keeps legacy read commands compatible", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockResolvedValue([]);
    expect(await listSessions()).toEqual([]);
    expect(invoke).toHaveBeenNthCalledWith(1, "list_sessions");
    expect(await getSessionMessages("gemini", "session-3")).toEqual([]);
    expect(invoke).toHaveBeenNthCalledWith(2, "get_session_messages", { providerId: "gemini", sessionId: "session-3" });
  });
  it("passes only identifiers for deletion and preserves the backend count", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockResolvedValue(0);
    expect(await deleteSessions(ids)).toBe(0);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("delete_sessions", { ids });
  });
  it.each(calls)("propagates native %s errors instead of hiding them", async (_name, call) => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockRejectedValue(new Error("Session file changed; refresh first"));
    await expect(call()).rejects.toThrow("Session file changed; refresh first");
  });
});
