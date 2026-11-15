import { useEffect, useRef, useState, type RefObject, type ReactNode } from "react";
import { ArrowLeft, ChevronDown, Grid2X2, Menu, Moon, Settings2, Sun, Workflow } from "lucide-react";
import { t, useLanguage } from "../i18n/language";
import type { ProxyStatus } from "../types/proxy";
import type { AppView } from "../lib/workspace-navigation";
import { Drawer, IconButton } from "../ui";
import { RuntimeBar } from "./RuntimeBar";
import { navigation, SideNav } from "./SideNav";
export type { AppView } from "../lib/workspace-navigation";

interface AppShellProps {
  activeView: AppView; onNavigate: (view: AppView) => void; providerCount: number; currentProviderName?: string;
  proxyStatus: ProxyStatus | null; onToggleProxy: () => void; proxyBusy?: boolean; runtimeStale?: boolean;
  theme: "light" | "dark"; onToggleTheme: () => void; mainRef?: RefObject<HTMLElement | null>; children: ReactNode;
}

function ShellMenu({ label, children, className = "", active = false, iconOnly = false }: { label: string; children: ReactNode; className?: string; active?: boolean; iconOnly?: boolean }) {
  const root = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const close = (event: PointerEvent) => {
      if (root.current?.open && event.target instanceof Node && !root.current.contains(event.target)) root.current.open = false;
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return <details ref={root} className={"xy-shell-dropdown " + className} data-active={active} onKeyDown={event => {
    if (event.key === "Escape" && root.current?.open) { event.stopPropagation(); root.current.open = false; root.current.querySelector("summary")?.focus(); }
  }}>
    <summary aria-label={label} title={label}>{!iconOnly && <span>{label}</span>}<ChevronDown size={14} aria-hidden="true" /></summary>
    <div className="xy-shell-dropdown-panel" onClick={event => { if (event.target instanceof Element && event.target.closest("button[data-view]") && root.current) root.current.open = false; }}>{children}</div>
  </details>;
}

export function AppShell({ activeView, onNavigate, providerCount, currentProviderName, proxyStatus, onToggleProxy, proxyBusy = false, runtimeStale = false, theme, onToggleTheme, mainRef, children }: AppShellProps) {
  useLanguage();
  const [navigationOpen, setNavigationOpen] = useState(false);
  const previousWorkspace = useRef<AppView>("clients");
  const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const selected = activeView === "provider-detail" ? "providers" : activeView;
  const tools = navigation.filter(item => item.section === "工具");
  const toolActive = tools.some(item => item.view === activeView);
  useEffect(() => { setNavigationOpen(false); if (activeView !== "agents") previousWorkspace.current = activeView; }, [activeView]);
  const navigate = (view: AppView) => { setNavigationOpen(false); onNavigate(view); };
  const isConnection = ["clients", "accounts", "diagnostics", "overview"].includes(activeView);
  const themeControl = <IconButton aria-label={theme === "dark" ? t("切换到浅色") : t("切换到深色")} onClick={onToggleTheme}>{theme === "dark" ? <Sun size={17} aria-hidden="true" /> : <Moon size={17} aria-hidden="true" />}</IconButton>;
  return <div className="xy-shell-root" data-theme={theme} data-workspace-view={activeView} data-workspace={activeView === "agents" ? "agents" : "configuration"}>
    <a className="xy-shell-skip" href="#workspace-content" onClick={event => { event.preventDefault(); document.getElementById("workspace-content")?.focus(); }}>{t("跳到内容")}</a>
    <header className="xy-shell-topbar">
      <div className="xy-shell-identity">
        <button type="button" className="xy-shell-brand" data-view="overview" onClick={() => navigate("overview")} aria-label={t("返回总览")} title={t("总览")}><svg viewBox="0 0 32 32" width="24" height="24" aria-hidden="true"><path d="m6 7 9 9-9 9m11-18 9 9-9 9" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="square" /></svg><span>XY <strong>Switch</strong></span></button>
        <ShellMenu label={t("切换工作区")} className="xy-shell-workspace-picker" iconOnly>
          <span className="xy-shell-menu-label">{t("工作区")}</span>
          <button type="button" className="xy-shell-nav-item" data-view="clients" onClick={() => navigate(previousWorkspace.current)} aria-current={activeView !== "agents" ? "true" : undefined}><Grid2X2 size={17} /><span><strong>{t("配置工作区")}</strong><small>{t("客户端、模型与工具")}</small></span></button>
          <button type="button" className="xy-shell-nav-item" data-view="agents" onClick={() => navigate("agents")} aria-current={activeView === "agents" ? "page" : undefined}><Workflow size={17} /><span><strong>{t("Agent 编队")}</strong><small>{t("协作拓扑、任务与用量")}</small></span></button>
        </ShellMenu>
      </div>
      {activeView === "agents" ? <button className="xy-shell-nav-item xy-shell-return" type="button" data-view={previousWorkspace.current} onClick={() => navigate(previousWorkspace.current)}><ArrowLeft size={15} />{t("返回配置工作区")}</button> : <nav className="xy-shell-primary-nav" aria-label={t("主导航")}>
        <button type="button" className="xy-shell-nav-item" data-view="clients" data-active={isConnection} aria-current={isConnection ? "page" : undefined} onClick={() => navigate("clients")}>{t("客户端")}</button>
        <button type="button" className="xy-shell-nav-item" data-view="providers" data-active={selected === "providers"} aria-current={selected === "providers" ? "page" : undefined} onClick={() => navigate("providers")}>{t("供应商")}</button>
        <button type="button" className="xy-shell-nav-item" data-view="sessions" data-active={selected === "sessions"} aria-current={selected === "sessions" ? "page" : undefined} onClick={() => navigate("sessions")}>{t("会话")}</button>
        <ShellMenu key={activeView} label={t("工具")} className="xy-shell-tools" active={toolActive}>
          {tools.map(item => <button key={item.view} type="button" className="xy-shell-nav-item" data-view={item.view} aria-current={item.view === activeView ? "page" : undefined} onClick={() => navigate(item.view)}>{t(item.label)}</button>)}
        </ShellMenu>
        <button type="button" className="xy-shell-nav-item" data-view="usage" data-active={selected === "usage"} aria-current={selected === "usage" ? "page" : undefined} onClick={() => navigate("usage")}>{t("用量")}</button>
      </nav>}
      <span className="xy-shell-mobile-location">{t(navigation.find(item => item.view === selected)?.label ?? "Agent 编队")}</span>
      <div className="xy-shell-utilities">
        <RuntimeBar status={proxyStatus} busy={proxyBusy} stale={runtimeStale} onToggle={onToggleProxy} currentProviderName={currentProviderName} />
        <IconButton className="xy-shell-settings xy-shell-nav-item" data-view="settings" aria-label={t("设置")} aria-current={selected === "settings" || selected === "governance" ? "page" : undefined} onClick={() => navigate("settings")}><Settings2 size={18} aria-hidden="true" /></IconButton>
        <IconButton className="xy-shell-menu" aria-label={t("展开导航")} title={t("全部功能")} aria-expanded={navigationOpen} onClick={() => setNavigationOpen(true)}><Menu size={18} aria-hidden="true" /></IconButton>
      </div>
    </header>
    <main ref={mainRef} id="workspace-content" className="xy-shell-main" tabIndex={-1}>{children}</main>
    <Drawer open={navigationOpen} side="right" title={t("全部功能")} onClose={() => setNavigationOpen(false)} footer={<div className="xy-shell-navigation-footer"><span>{native ? t("本地工作台") : t("浏览器预览")}</span>{themeControl}</div>}><SideNav activeView={activeView} onNavigate={navigate} providerCount={providerCount} /></Drawer>
  </div>;
}
