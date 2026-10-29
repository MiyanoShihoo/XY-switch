import { useLanguage, t } from "./i18n/language";
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AppShell, type AppView } from "./shell/AppShell";
import { Badge, InlineError } from "./ui";
import { useProviders } from "./features/providers/useProviders";
import { ProvidersPage } from "./features/providers/ProvidersPage";
import { McpPage } from "./features/mcp/McpPage";
import { PromptsPage } from "./features/prompts/PromptsPage";
import { SkillsPage } from "./features/skills/SkillsPage";
import { SessionsPage } from "./features/sessions/SessionsPage";
import { UsagePage } from "./features/usage/UsagePage";
import { GovernancePage } from "./features/governance/GovernancePage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { nextExplicitTheme, readThemePreference, resolveTheme, type ThemePreference } from "./lib/theme";
import { loadInitialProxyStatus, loadProxyStatus, setProxyEnabled } from "./lib/proxy-service";
import { errorText } from "./lib/error";
import { usePlatformCapabilities } from "./lib/platform-service";
import type { ProxyStatus } from "./types/proxy";

export default function App() {
  useLanguage();
  const client = useQueryClient();
  const platform = usePlatformCapabilities();
  const [activeView, setActiveView] = useState<AppView>("providers");
  const [themePreference, setThemePreference] = useState<ThemePreference>(() => readThemePreference(localStorage.getItem("xy-switch:theme")));
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const theme = resolveTheme(themePreference, systemDark);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [proxyBusy, setProxyBusy] = useState(false);
  const [runtimeError, setRuntimeError] = useState("");
  const proxyLock = useRef(false), runtimeGeneration = useRef(0), mainRef = useRef<HTMLElement>(null);
  const refreshRuntime = useCallback(async () => {
    const generation = ++runtimeGeneration.current;
    const next = await loadProxyStatus();
    if (generation === runtimeGeneration.current) { setProxyStatus(next); setRuntimeError(""); }
  }, []);
  const providers = useProviders(refreshRuntime);
  useEffect(() => {
    const generation = ++runtimeGeneration.current;
    void loadInitialProxyStatus().then(next => { if (generation === runtimeGeneration.current) setProxyStatus(next); }).catch(cause => { if (generation === runtimeGeneration.current) setRuntimeError(t("路由读取失败：") + errorText(cause)); });
    return () => { runtimeGeneration.current += 1; };
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    if (themePreference === "system") localStorage.removeItem("xy-switch:theme"); else localStorage.setItem("xy-switch:theme", themePreference);
  }, [theme, themePreference]);
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setSystemDark(media.matches); update(); media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => { mainRef.current?.scrollTo({ top: 0 }); }, [activeView]);
  const mutateProxy = useCallback(async (work: () => Promise<ProxyStatus>): Promise<ProxyStatus> => {
    if (proxyLock.current) throw new Error(t("另一项路由操作正在进行，请稍候。"));
    proxyLock.current = true; runtimeGeneration.current += 1; setProxyBusy(true); setRuntimeError("");
    try {
      const next = await work(); runtimeGeneration.current += 1; setProxyStatus(next); return next;
    } catch (cause) {
      // A failed write may have partially changed the backend. Do not keep presenting an assumed runtime state.
      try { setProxyStatus(await loadProxyStatus()); } catch { setProxyStatus(null); }
      runtimeGeneration.current += 1; setRuntimeError(t("路由操作失败：") + errorText(cause)); throw cause;
    } finally { proxyLock.current = false; setProxyBusy(false); }
  }, []);
  const toggleProxy = async () => {
    if (!proxyStatus || proxyLock.current) return;
    const requested = !proxyStatus.running;
    try { const next = await mutateProxy(() => setProxyEnabled(requested)); if (next.running !== requested) setRuntimeError(next.lastError || t("路由实际状态与请求不一致。")); }
    catch { /* mutateProxy records the error and reloads actual state. */ }
  };
  const imported = async () => {
    const [refreshed] = await Promise.all([providers.refresh(), refreshRuntime(), client.invalidateQueries({ refetchType: "all" }, { throwOnError: true })]);
    if (!refreshed) throw new Error(t("供应商状态无法刷新，请在供应商页重试。"));
  };
  const currentName = providers.loading ? t("读取中") : providers.error ? t("状态未知") : providers.providers.find(provider => provider.id === providers.currentId)?.name;
  return <AppShell activeView={activeView} onNavigate={setActiveView} providerCount={providers.providers.length} currentProviderName={currentName} proxyStatus={proxyStatus} proxyBusy={proxyBusy} onToggleProxy={() => void toggleProxy()} theme={theme} onToggleTheme={() => setThemePreference(nextExplicitTheme(theme))} mainRef={mainRef}>
    {platform.data?.mobile && <p className="workspace-route-notice">{platform.data.os === "android" ? "Android" : "iOS"} {t("移动端：凭据保存在")} {platform.data.credentialStore}{t("；不能接管其他应用的 CLI，也不能保证后台持续运行本地代理。")}</p>}
    {runtimeError && <div className="workspace-runtime-error"><InlineError message={runtimeError} onRetry={() => { void refreshRuntime().catch(cause => setRuntimeError(errorText(cause))); }} /></div>}
    {activeView === "providers" && <>{proxyStatus?.settings.showInHome && <div className="workspace-route-notice"><Badge dot tone={proxyStatus.lastError ? "danger" : proxyStatus.running ? "success" : "neutral"}>{proxyStatus.lastError ? t("路由异常") : proxyStatus.running ? t("本地路由运行中") : t("本地路由已停止")}</Badge><code>{proxyStatus.address}</code></div>}<ProvidersPage controller={providers} onRuntimeChanged={refreshRuntime} /></>}
    {activeView === "mcp" && <McpPage />}
    {activeView === "prompts" && <PromptsPage />}
    {activeView === "skills" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("移动端不支持扫描桌面 Skills 目录，请在 Windows、macOS 或 Linux 端导入。")}</p> : <SkillsPage />)}
    {activeView === "sessions" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("移动端无法读取其他应用的本机会话目录，请在对应桌面端查看。")}</p> : <SessionsPage />)}
    {activeView === "usage" && <UsagePage providers={providers.providers} />}
    {activeView === "governance" && <GovernancePage providers={providers.providers} proxyStatus={proxyStatus} onRestored={imported} />}
    {activeView === "settings" && <SettingsPage themePreference={themePreference} onThemeChange={setThemePreference} providers={providers} proxyStatus={proxyStatus} proxyBusy={proxyBusy} runtimeError={runtimeError} mutateProxy={mutateProxy} onRefreshRuntime={refreshRuntime} onImported={imported} />}
  </AppShell>;
}
