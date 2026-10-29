import { describe, expect, it, vi } from "vitest";
import { createNativeSessionFavoritesStore, type NativeFavoritesDriver } from "./lib/native-session-favorites";
import { sessionKey } from "./lib/session-format";
import type { SessionIdPair } from "./types/session";
const first = { providerId: "codex", sessionId: "one" }, second = { providerId: "claude", sessionId: "two" };
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail; }); return { promise, resolve, reject }; }
function fixture(initial: SessionIdPair[] = []) {
  const saved = new Map(initial.map(item => [sessionKey(item), item]));
  const driver = { load: vi.fn(async () => [...saved.values()]), set: vi.fn(async (favorite: SessionIdPair, enabled: boolean) => { if (enabled) saved.set(sessionKey(favorite), favorite); else saved.delete(sessionKey(favorite)); }) };
  return { saved, driver, store: createNativeSessionFavoritesStore(driver) };
}
const keys = (...values: SessionIdPair[]) => new Set(values.map(sessionKey));
describe("native favorites queue", () => {
  it("loads without rewriting and only exposes identifiers", async () => {
    const {store,driver}=fixture([first]); await store.refresh(); expect(store.getSnapshot()).toEqual({keys:keys(first),issue:null,unsaved:false}); expect(driver.set).not.toHaveBeenCalled();
  });
  it("merges optimistic edits into a late initial load", async () => {
    const pending=deferred<SessionIdPair[]>(), data=fixture(); data.driver.load.mockImplementationOnce(()=>pending.promise);
    const loading=data.store.refresh(); await Promise.resolve(); data.store.toggle(first); data.store.toggle(first); data.store.toggle(second);
    expect(data.store.getSnapshot().unsaved).toBe(true); pending.resolve([first]); await loading;
    expect(data.store.getSnapshot()).toEqual({keys:keys(second),issue:null,unsaved:false});
  });
  it("does not erase a newer toggle when an older write finishes", async () => {
    const data=fixture(), write=deferred<void>(); await data.store.refresh();
    data.driver.set.mockImplementationOnce(async (favorite,enabled)=>{ await write.promise; if(enabled)data.saved.set(sessionKey(favorite),favorite); });
    data.store.toggle(first); await Promise.resolve(); await Promise.resolve(); expect(data.driver.set).toHaveBeenCalledTimes(1);
    data.store.toggle(first); data.store.toggle(second); const flushed=data.store.refresh(); write.resolve(); await flushed;
    expect(data.store.getSnapshot()).toEqual({keys:keys(second),issue:null,unsaved:false}); expect(data.saved.has(sessionKey(first))).toBe(false);
    expect(data.driver.set.mock.calls.map(call=>call[1])).toEqual([true,false,true]);
  });
  it("retains failed edits and merges newer persisted data on explicit retry", async () => {
    const data=fixture(); await data.store.refresh(); data.driver.set.mockRejectedValueOnce(new Error("database unavailable")); data.store.toggle(first); await data.store.refresh();
    expect(data.store.getSnapshot()).toEqual({keys:keys(first),issue:"unavailable",unsaved:true}); data.saved.set(sessionKey(second),second);
    await data.store.refresh(); expect(data.store.getSnapshot()).toEqual({keys:keys(first,second),issue:null,unsaved:false});
  });
  it("does not overwrite invalid legacy data or write pending edits until a valid load", async () => {
    const driver: NativeFavoritesDriver = { load: vi.fn(async()=>{ throw new Error("Invalid legacy favorites data"); }), set: vi.fn() };
    const store=createNativeSessionFavoritesStore(driver); store.toggle(first); await store.refresh(); expect(store.getSnapshot().issue).toBe("invalid"); expect(store.getSnapshot().unsaved).toBe(true); expect(driver.set).not.toHaveBeenCalled();
  });
  it("refreshes imported favorites without rewriting the collection", async () => {
    const data=fixture(); await data.store.refresh(); data.saved.set(sessionKey(second),second); await data.store.refresh(); expect(data.store.getSnapshot().keys).toEqual(keys(second)); expect(data.driver.set).not.toHaveBeenCalled();
  });
  it("never passes a title, path or body to native persistence", async () => {
    const data=fixture(); await data.store.refresh(); data.store.toggle({...first,title:"private title",sourcePath:"private path"} as SessionIdPair); await data.store.refresh();
    expect(data.driver.set).toHaveBeenCalledWith(first,true); expect(Object.keys(data.driver.set.mock.calls[0][0])).toEqual(["providerId","sessionId"]);
  });
  it("maintains one write at a time and survives subscribers queuing a final edit", async () => {
    const data=fixture(); await data.store.refresh(); let added=false;
    const off=data.store.subscribe(()=>{ if(!added && !data.store.getSnapshot().unsaved && data.store.getSnapshot().keys.has(sessionKey(first))) { added=true; data.store.toggle(second); } });
    data.store.toggle(first); await data.store.refresh(); off(); expect(data.store.getSnapshot().keys).toEqual(keys(first,second)); expect(data.store.getSnapshot().unsaved).toBe(false);
  });
  it("retries reads without a background retry loop", async () => {
    const data=fixture(); data.driver.load.mockRejectedValueOnce(new Error("locked")); await data.store.refresh(); expect(data.driver.load).toHaveBeenCalledTimes(1); expect(data.store.getSnapshot().issue).toBe("unavailable"); await data.store.refresh(); expect(data.store.getSnapshot().issue).toBeNull();
  });
});
