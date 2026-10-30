import { useLanguage, t } from "../i18n/language";
import { Activity, BookOpen, Box, Cable, Network, Settings, ShieldCheck, Sparkles, WandSparkles } from "lucide-react";
import type { AppView } from "./AppShell";
export const navigationSections = ["工作区", "观测", "管理"] as const;
export const navigation = [
  { view: "providers", label: "供应商", icon: Box, section: "工作区" },
  { view: "mcp", label: "MCP 服务器", icon: Network, section: "工作区" },
  { view: "prompts", label: "提示词", icon: Sparkles, section: "工作区" },
  { view: "skills", label: "Skills", icon: WandSparkles, section: "工作区" },
  { view: "diagnostics", label: "连接诊断", icon: Cable, section: "观测" },
  { view: "sessions", label: "会话", icon: BookOpen, section: "观测" },
  { view: "usage", label: "用量与审计", icon: Activity, section: "观测" },
  { view: "governance", label: "安全与验收", icon: ShieldCheck, section: "管理" },
  { view: "settings", label: "设置", icon: Settings, section: "管理" },
] as const;
export function SideNav({ activeView, onNavigate, providerCount }: { activeView: AppView; onNavigate: (view: AppView) => void; providerCount: number }) {
  useLanguage();
  const selected = activeView === "provider-detail" ? "providers" : activeView;
  return <aside className="xy-shell-sidebar" aria-label={t("主导航")}>
    <div className="xy-shell-brand"><span className="xy-shell-brand-mark" aria-hidden="true"><svg viewBox="0 0 24 24" width="24" height="24"><path d="M4 7h14m-4-4 4 4-4 4M20 17H6m4-4-4 4 4 4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg></span><div><strong>XY Switch</strong><span>{t("本地 AI 工作台")}</span></div></div>
    <nav className="xy-shell-nav">{navigationSections.map(section => <div className="xy-shell-nav-group" key={section}><p className="xy-shell-section-label">{t(section)}</p>{navigation.filter(item => item.section === section).map(item => <button key={item.view} type="button" className="xy-shell-nav-item" data-view={item.view} data-active={selected === item.view} aria-current={selected === item.view ? "page" : undefined} aria-label={t(item.label)} title={t(item.label)} onClick={() => onNavigate(item.view)}><item.icon size={18} strokeWidth={1.7} aria-hidden="true" /><span className="xy-shell-nav-label">{t(item.label)}</span>{item.view === "providers" && <span className="xy-shell-nav-count" aria-hidden="true">{providerCount}</span>}</button>)}</div>)}</nav>
    <div className="xy-shell-edition"><ShieldCheck size={16} aria-hidden="true" /><div>{t("本地优先")}<span>{t("配置由你掌控")}</span></div></div>
  </aside>;
}
