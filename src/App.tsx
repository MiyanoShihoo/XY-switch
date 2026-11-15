import { useLanguage, t } from "./i18n/language";
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AppShell } from "./shell/AppShell";
import { AgentWorkspace } from "./shell/AgentWorkspace";
import { useWorkspaceNavigation, type AppView } from "./lib/workspace-navigation";
import { InlineError } from "./ui";
import { OverviewPage } from "./features/overview/OverviewPage";
import { useProviders } from "./features/providers/useProviders";
import { DEFAULT_PROVIDER_DIRECTORY_STATE, ProvidersPage, type ProviderDirectoryState } from "./features/providers/ProvidersPage";
import { ProviderConfigurationPage } from "./features/providers/ProviderConfigurationPage";
import { ProviderDiagnosticsPage } from "./features/providers/ProviderDiagnosticsPage";
import { ConfigurationSwitcher } from "./features/providers/ConfigurationSwitcher";
import type { ProviderNavigationGuard } from "./features/providers/ProviderEditor";
import { McpPage } from "./features/mcp/McpPage";
import { ClientConnectionsPage } from "./features/clients/ClientConnectionsPage";
import { AccountsPage } from "./features/accounts/AccountsPage";
import { PluginsPage } from "./features/plugins/PluginsPage";
import { MarketplacePage } from "./features/marketplace/MarketplacePage";
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
  const { route, navigate: navigateRoute } = useWorkspaceNavigation();
  const providerNavigationGuard = useRef<ProviderNavigationGuard | null>(null);
  const setProviderNavigationGuard = useCallback((guard: ProviderNavigationGuard | null) => { providerNavigationGuard.current = guard; }, []);
  const navigate = useCallback((view: AppView, providerId?: string) => {
    const proceed = () => navigateRoute(view, providerId);
    if (providerNavigationGuard.current) providerNavigationGuard.current(proceed); else proceed();
  }, [navigateRoute]);
  const activeView = route.view;
  const configurationRoute = useRef(route.view === "agents" ? { view: "clients" as const } : route);
  useEffect(() => { if (route.view !== "agents") configurationRoute.current = route; }, [route]);
  const previousView = useRef(activeView);
  const [providerDirectoryState, setProviderDirectoryState] = useState<ProviderDirectoryState>(DEFAULT_PROVIDER_DIRECTORY_STATE);
  const [themePreference, setThemePreference] = useState<ThemePreference>(() => readThemePreference(localStorage.getItem("xy-switch:theme")));
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const theme = resolveTheme(themePreference, systemDark);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [proxyBusy, setProxyBusy] = useState(false);
  const [runtimeError, setRuntimeError] = useState("");
  const [runtimeStale, setRuntimeStale] = useState(false);
  const proxyLock = useRef(false), runtimeGeneration = useRef(0), mainRef = useRef<HTMLElement>(null);
  const refreshRuntime = useCallback(async () => {
    const generation = ++runtimeGeneration.current;
    try {
      const next = await loadProxyStatus();
      if (generation === runtimeGeneration.current) { setProxyStatus(next); setRuntimeStale(false); setRuntimeError(""); }
    } catch (cause) {
      if (generation === runtimeGeneration.current) { setProxyStatus(null); setRuntimeStale(true); setRuntimeError(t("路由读取失败：") + errorText(cause)); }
      throw cause;
    }
  }, []);
  const providers = useProviders(refreshRuntime);
  useEffect(() => {
    const generation = ++runtimeGeneration.current;
    void loadInitialProxyStatus().then(next => { if (generation === runtimeGeneration.current) { setProxyStatus(next); setRuntimeStale(false); } }).catch(cause => { if (generation === runtimeGeneration.current) { setProxyStatus(null); setRuntimeStale(true); setRuntimeError(t("路由读取失败：") + errorText(cause)); } });
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
  useEffect(() => { if (previousView.current === activeView) return; previousView.current = activeView; if (activeView === "providers" && providerDirectoryState.focusTarget) return; mainRef.current?.scrollTo({ top: 0 }); mainRef.current?.focus({ preventScroll: true }); }, [activeView, providerDirectoryState.focusTarget]);
  const mutateProxy = useCallback(async (work: () => Promise<ProxyStatus>): Promise<ProxyStatus> => {
    if (proxyLock.current) throw new Error(t("另一项路由操作正在进行，请稍候。"));
    proxyLock.current = true; runtimeGeneration.current += 1; setProxyBusy(true); setRuntimeError("");
    try {
      const next = await work(); runtimeGeneration.current += 1; setProxyStatus(next); setRuntimeStale(false); return next;
    } catch (cause) {
      // A failed write may have partially changed the backend. Do not keep presenting an assumed runtime state.
      try { setProxyStatus(await loadProxyStatus()); setRuntimeStale(false); } catch { setProxyStatus(null); setRuntimeStale(true); }
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
  const currentName = providers.loading ? t("读取中") : providers.readError ? t("状态未知") : providers.providers.find(provider => provider.id === providers.currentId)?.name;
  const returnToConfiguration = () => navigate(configurationRoute.current.view, configurationRoute.current.providerId);
  return <AppShell activeView={activeView} onNavigate={view => activeView === "agents" && view === configurationRoute.current.view ? returnToConfiguration() : navigate(view)} providerCount={providers.providers.length} currentProviderName={currentName} proxyStatus={proxyStatus} proxyBusy={proxyBusy} runtimeStale={runtimeStale} onToggleProxy={() => void toggleProxy()} theme={theme} onToggleTheme={() => setThemePreference(nextExplicitTheme(theme))} mainRef={mainRef}>
    {platform.data?.mobile && <p className="workspace-route-notice">{platform.data.os === "android" ? "Android" : "iOS"} {t("移动端：凭据保存在")} {platform.data.credentialStore}{t("；不能接管其他应用的 CLI，也不能保证后台持续运行本地代理。")}</p>}
    {runtimeError && <div className="workspace-runtime-error"><InlineError message={runtimeError} onRetry={() => { void refreshRuntime().catch(() => undefined); }} /></div>}
    {activeView === "overview" && <OverviewPage providers={providers} proxyStatus={proxyStatus} onNavigate={navigate} />}
    {activeView === "providers" && <ProvidersPage controller={providers} directoryState={providerDirectoryState} onDirectoryStateChange={setProviderDirectoryState} onRuntimeChanged={refreshRuntime} onNavigationGuardChange={setProviderNavigationGuard} onConfigure={id => navigate("provider-detail", id)} onDiagnostics={id => navigate("diagnostics", id)} />}
    {activeView === "provider-detail" && <ProviderConfigurationPage key={route.providerId} controller={providers} providerId={route.providerId ?? ""} onBack={() => navigate("providers")} onDiagnostics={id => navigate("diagnostics", id)} onRuntimeChanged={refreshRuntime} onNavigationGuardChange={setProviderNavigationGuard} />}
    {activeView === "diagnostics" && <ProviderDiagnosticsPage controller={providers} providerId={route.providerId} onSelect={id => navigate("diagnostics", id)} onConfigure={id => navigate("provider-detail", id)} onProviders={() => navigate("providers")} />}
    {activeView === "mcp" && <McpPage />}
    {activeView === "clients" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("请在桌面端管理原生客户端登录和配置。")}</p> : <ClientConnectionsPage onNavigate={navigate} configurationControl={<ConfigurationSwitcher controller={providers} onManage={() => navigate("providers")} />} />)}
    {activeView === "accounts" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("请在桌面端管理授权账号池。")}</p> : <AccountsPage />)}
    {activeView === "plugins" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("请在桌面端安装和执行本地插件。")}</p> : <PluginsPage />)}
    {activeView === "marketplace" && <MarketplacePage />}
    {activeView === "prompts" && <PromptsPage />}
    {activeView === "skills" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("移动端不支持扫描桌面 Skills 目录，请在 Windows、macOS 或 Linux 端导入。")}</p> : <SkillsPage />)}
    {activeView === "sessions" && (platform.data?.mobile ? <p className="workspace-route-notice">{t("移动端无法读取其他应用的本机会话目录，请在对应桌面端查看。")}</p> : <SessionsPage />)}
    {activeView === "usage" && <UsagePage providers={providers.providers} />}
    {activeView === "governance" && <GovernancePage providers={providers.providers} proxyStatus={proxyStatus} onRestored={imported} />}
    {activeView === "settings" && <SettingsPage themePreference={themePreference} onThemeChange={setThemePreference} providers={providers} proxyStatus={proxyStatus} proxyBusy={proxyBusy} runtimeError={runtimeError} mutateProxy={mutateProxy} onRefreshRuntime={refreshRuntime} onImported={imported} />}
    {activeView === "agents" && <AgentWorkspace onReturn={returnToConfiguration} />}
  </AppShell>;
}
