import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { duplicateProvider } from "../../lib/provider-actions";
import { loadProviderHealth, testAllProviderHealth, testProviderHealth } from "../../lib/health-service";
import { deleteProvider, loadCurrentProvider, loadProviders, saveProvider, saveProviderOrder, saveSelectedProvider } from "../../lib/provider-service";
import { normalizeProvider } from "../../lib/providers";
import type { Provider, ProviderHealth } from "../../types/provider";
export function errorText(error: unknown) { return error instanceof Error ? error.message : String(error); }
export function useProviders(onRuntimeChanged: () => Promise<void>) {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [currentId, setCurrentId] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [health, setHealth] = useState<Record<string, ProviderHealth>>({});
  const mounted = useRef(true), operation = useRef(false), testingRef = useRef(false), refreshGeneration = useRef(0);
  const callback = useRef(onRuntimeChanged); callback.current = onRuntimeChanged;
  const refresh = useCallback(async () => {
    const generation = ++refreshGeneration.current;
    setLoading(true); setError("");
    try {
      const [items, selected, checks] = await Promise.all([loadProviders(), loadCurrentProvider(), loadProviderHealth()]);
      if (!mounted.current || generation !== refreshGeneration.current) return false;
      const list = items.map(normalizeProvider);
      setProviders(list);
      // Never report a fallback as activated before the backend confirms it.
      setCurrentId(list.some(p => p.id === selected) ? selected : "");
      setHealth(Object.fromEntries(checks.map(item => [item.providerId, item])));
      return true;
    } catch (e) { if (mounted.current && generation === refreshGeneration.current) setError("供应商读取失败：" + errorText(e)); return false; }
    finally { if (mounted.current && generation === refreshGeneration.current) setLoading(false); }
  }, []);
  useEffect(() => { mounted.current = true; void refresh(); return () => { mounted.current = false; refreshGeneration.current += 1; }; }, [refresh]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const register = (promise: Promise<() => void>) => { void promise.then(stop => { if (disposed) stop(); else cleanups.push(stop); }).catch(e => setError("托盘事件监听失败：" + errorText(e))); };
    register(listen("provider-changed", () => { void refresh(); void callback.current().catch(e => setError(errorText(e))); }));
    register(listen<string>("provider-switch-error", event => setError("托盘供应商切换失败：" + event.payload)));
    return () => { disposed = true; cleanups.forEach(stop => stop()); };
  }, [refresh]);
  const run = async (work: () => Promise<void>) => {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError(""); setMessage("");
    try { await work(); } catch (e) { setError(errorText(e)); } finally { operation.current = false; setBusy(false); }
  };
  const activate = (id: string) => run(async () => {
    const provider = providers.find(p => p.id === id);
    if (!provider?.enabled || id === currentId) return;
    await saveSelectedProvider(id); setCurrentId(id); setMessage("已设为当前：" + provider.name);
    try { await callback.current(); } catch (e) { setError("配置已切换，但运行状态读取失败：" + errorText(e)); }
  });
  const copy = (source: Provider) => run(async () => {
    const duplicate = duplicateProvider(source, providers);
    await saveProvider(duplicate); setProviders(items => [...items, duplicate]); setMessage("已复制为 " + duplicate.name + "，API Key 未复制");
  });
  const remove = async (provider: Provider) => {
    if (provider.builtIn || operation.current) return false;
    let removed = false;
    await run(async () => {
      await deleteProvider(provider.id);
      const remaining = providers.filter(p => p.id !== provider.id);
      setProviders(remaining); removed = true; setMessage("已删除 " + provider.name);
      try { const id = await loadCurrentProvider(); setCurrentId(remaining.some(p => p.id === id) ? id : ""); await callback.current(); }
      catch (e) { if (currentId === provider.id) setCurrentId(""); setError("配置已删除，但当前状态读取失败：" + errorText(e)); }
    });
    return removed;
  };
  const reorder = (ordered: Provider[]) => run(async () => {
    const previous = providers; setProviders(ordered);
    try { await saveProviderOrder(ordered.map(p => p.id)); setMessage("供应商顺序已保存"); }
    catch (e) { setProviders(previous); throw new Error("排序保存失败：" + errorText(e)); }
  });
  const test = async (id?: string) => {
    if (testingRef.current) return;
    testingRef.current = true; setTesting(id ?? "all"); setError(""); setMessage("");
    try {
      const result = id ? [await testProviderHealth(id)] : await testAllProviderHealth();
      setHealth(items => ({ ...items, ...Object.fromEntries(result.map(item => [item.providerId, item])) })); setMessage("连接测试已完成，结果以列表状态为准");
    } catch (e) { setError("连接测试失败：" + errorText(e)); }
    finally { testingRef.current = false; setTesting(null); }
  };
  const saved = (provider: Provider) => { setProviders(items => items.some(p => p.id === provider.id) ? items.map(p => p.id === provider.id ? provider : p) : [...items, provider]); setMessage("已保存 " + provider.name); };
  return { providers, currentId, loading, busy, testing, error, message, health, refresh, activate, copy, remove, reorder, test, saved, dismissMessage: () => setMessage("") };
}
export type ProvidersController = ReturnType<typeof useProviders>;
