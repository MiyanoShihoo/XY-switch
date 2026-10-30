import { useLanguage, t } from "../i18n/language";
import type { RefObject, ReactNode } from "react";
import type { ProxyStatus } from "../types/proxy";
import type { AppView } from "../lib/workspace-navigation";
import { ContextBar } from "./ContextBar";
import { RuntimeBar } from "./RuntimeBar";
import { SideNav } from "./SideNav";
export type { AppView } from "../lib/workspace-navigation";
interface AppShellProps { activeView: AppView; onNavigate: (view: AppView) => void; providerCount: number; currentProviderName?: string; proxyStatus: ProxyStatus | null; onToggleProxy: () => void; proxyBusy?: boolean; theme: "light" | "dark"; onToggleTheme: () => void; mainRef?: RefObject<HTMLElement | null>; children: ReactNode; }
export function AppShell({ activeView, onNavigate, providerCount, currentProviderName, proxyStatus, onToggleProxy, proxyBusy = false, theme, onToggleTheme, mainRef, children }: AppShellProps) {
  useLanguage();
  return <div className="xy-shell-root" data-theme={theme} data-workspace-view={activeView}>
    <a className="xy-shell-skip" href="#workspace-content" onClick={event => { event.preventDefault(); document.getElementById("workspace-content")?.focus(); }}>{t("跳到内容")}</a>
    <SideNav activeView={activeView} onNavigate={onNavigate} providerCount={providerCount} />
    <div className="xy-shell-content">
      <ContextBar activeView={activeView} currentProviderName={currentProviderName} theme={theme} onToggleTheme={onToggleTheme} />
      <main ref={mainRef} id="workspace-content" className="xy-shell-main" tabIndex={-1}>{children}</main>
      <RuntimeBar status={proxyStatus} busy={proxyBusy} onToggle={onToggleProxy} />
    </div>
  </div>;
}
