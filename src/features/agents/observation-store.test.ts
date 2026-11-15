import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentSnapshot } from "../../types/agents";
import { createObservationStore, type ObservationService } from "./observation-store";
import { snapshot } from "./test-fixtures";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const flush = () => vi.advanceTimersByTimeAsync(0);
const stores: ReturnType<typeof createObservationStore>[] = [];
function observe(service: ObservationService) {
  const store = createObservationStore(service); stores.push(store); store.start({}); return store;
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => { stores.splice(0).forEach(store => store.stop()); vi.useRealTimers(); });

describe("observation request lifecycle", () => {
  it("rejects an older query response and preserves the last successful projection on failure", async () => {
    const first = deferred<AgentSnapshot>(), second = deferred<AgentSnapshot>(), failed = deferred<AgentSnapshot>();
    const read = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockReturnValueOnce(failed.promise);
    const dispose = vi.fn();
    const store = observe({ available: () => true, snapshot: read, listen: async () => dispose });
    await flush();
    store.setQuery({ teamId: "selected", at: 900 });
    await flush();
    const projected = snapshot({ revision: 2, replayAt: 900 });
    second.resolve(projected); await flush();
    first.resolve(snapshot({ revision: 99 })); await flush();
    expect(store.getSnapshot().snapshot).toBe(projected);
    expect(store.getSnapshot().snapshotQuery).toEqual({ teamId: "selected", at: 900 });
    store.refresh(); await flush(); failed.reject(new Error("source temporarily unavailable")); await flush();
    expect(store.getSnapshot()).toMatchObject({ snapshot: projected, connection: "disconnected", loading: false });
    expect(projected.agents[0]).toMatchObject({ status: "running", connection: "disconnected", endedAt: null });
    store.stop(); expect(dispose).toHaveBeenCalledOnce();
  });

  it("times out pending reads, ignores their late results, and disposes a late subscription after unmount", async () => {
    const read = deferred<AgentSnapshot>(), subscription = deferred<() => void>(), dispose = vi.fn();
    let notify: (() => void) | undefined;
    const store = observe({ available: () => true, snapshot: () => read.promise, listen: callback => { notify = callback; return subscription.promise; } });
    await vi.advanceTimersByTimeAsync(15000);
    expect(store.getSnapshot()).toMatchObject({ snapshot: null, connection: "disconnected", notifications: "disconnected", loading: false, error: "AGENT_READ_TIMEOUT" });
    const version = store.getSnapshot().changeVersion;
    store.stop(); subscription.resolve(dispose); read.resolve(snapshot()); await flush(); notify?.();
    expect(dispose).toHaveBeenCalledOnce();
    expect(store.getSnapshot().snapshot).toBeNull();
    expect(store.getSnapshot().changeVersion).toBe(version);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("recovers missed notifications through reconciliation without inferring a terminal lifecycle", async () => {
    const read = vi.fn().mockResolvedValue(snapshot());
    const store = observe({ available: () => true, snapshot: read, listen: async () => () => {} });
    await vi.advanceTimersByTimeAsync(200);
    const version = store.getSnapshot().changeVersion;
    read.mockResolvedValue(snapshot({ revision: 2 }));
    await vi.advanceTimersByTimeAsync(30000);
    expect(store.getSnapshot().changeVersion).toBeGreaterThan(version);
    expect(store.getSnapshot().snapshot?.agents[0].status).toBe("running");
    expect(store.getSnapshot().snapshot?.agents[0].endedAt).toBeNull();
  });

  it("never invokes native services in a browser-only environment", async () => {
    const read = vi.fn(), listen = vi.fn();
    const store = observe({ available: () => false, snapshot: read, listen });
    store.refresh(); store.setQuery({ at: 1000 }); await vi.advanceTimersByTimeAsync(60000);
    expect(store.getSnapshot()).toMatchObject({ connection: "native-required", notifications: "unavailable", snapshot: null });
    expect(read).not.toHaveBeenCalled(); expect(listen).not.toHaveBeenCalled();
  });
});
