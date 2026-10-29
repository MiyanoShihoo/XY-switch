import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadInitialProxyStatus } from "./lib/proxy-service";
import type { ProxyStatus } from "./types/proxy";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
const status = (enabled: boolean, running = false): ProxyStatus => ({
  settings: { enabled, showInHome: false, host: "127.0.0.1", port: 15721, routeTargets: ["codex"] },
  running, address: "http://127.0.0.1:15721", lastError: null,
});

describe("native routing startup", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  });
  afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  it("does not enable routing when the backend reports it disabled", async () => {
    const disabled = status(false);
    invokeMock.mockResolvedValue(disabled);
    expect(await loadInitialProxyStatus()).toBe(disabled);
    expect(invokeMock.mock.calls).toEqual([["get_proxy_status"]]);
  });
  it("starts a stopped proxy only when its settings explicitly enable it", async () => {
    const stopped = status(true);
    const running = status(true, true);
    invokeMock.mockResolvedValueOnce(stopped).mockResolvedValueOnce(running);
    expect(await loadInitialProxyStatus()).toBe(running);
    expect(invokeMock.mock.calls).toEqual([["get_proxy_status"], ["set_proxy_enabled", { enabled: true }]]);
  });
  it("does not restart an already running listener", async () => {
    const running = status(true, true);
    invokeMock.mockResolvedValue(running);
    expect(await loadInitialProxyStatus()).toBe(running);
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });
  it("retains the reported state and warns if an enabled listener cannot start", async () => {
    const stopped = status(true);
    const error = new Error("port already in use");
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    invokeMock.mockResolvedValueOnce(stopped).mockRejectedValueOnce(error);
    expect(await loadInitialProxyStatus()).toBe(stopped);
    expect(warning).toHaveBeenCalledWith("Auto-start proxy on launch failed:", error);
  });
  it("does not try to start a listener in browser preview", async () => {
    vi.stubGlobal("window", {});
    expect((await loadInitialProxyStatus()).running).toBe(false);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
