import { useLanguage, t } from "../i18n/language";
import { Activity, BookOpen, Box, Network, Settings, ShieldCheck, Sparkles, WandSparkles } from "lucide-react";
import type { AppView } from "./AppShell";
export const navigation = [
  { view: "providers", label: "供应商", icon: Box, section: "配置" },
  { view: "mcp", label: "MCP 服务器", icon: Network, section: "配置" },
  { view: "prompts", label: "提示词", icon: Sparkles, section: "配置" },
  { view: "skills", label: "Skills", icon: WandSparkles, section: "配置" },
  { view: "sessions", label: "会话", icon: BookOpen, section: "记录" },
  { view: "usage", label: "用量与审计", icon: Activity, section: "记录" },
  { view: "governance", label: "安全与验收", icon: ShieldCheck, section: "系统" },
  { view: "settings", label: "设置", icon: Settings, section: "系统" },
] as const;
export function SideNav({ activeView, onNavigate, providerCount }: { activeView: AppView; onNavigate: (view: AppView) => void; providerCount: number }) {
  useLanguage();
  return <aside className="xy-shell-sidebar" aria-label={t("主导航")}>
    <div className="xy-shell-brand"><svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true"><path d="M3 5h7v4H3zM14 5h7v4h-7zM3 15h7v4H3zM14 15h7v4h-7zM10 7h4v10h-4z" fill="currentColor" /></svg><strong>XY Switch</strong></div>
    <nav className="xy-shell-nav">{["配置", "记录", "系统"].map(section => <div className="xy-shell-nav-group" key={section}><p className="xy-shell-section-label">{t(section)}</p>{navigation.filter(item => item.section === section).map(item => <button key={item.view} type="button" className="xy-shell-nav-item" data-active={activeView === item.view} aria-current={activeView === item.view ? "page" : undefined} title={t(item.label)} onClick={() => onNavigate(item.view)}><item.icon size={16} strokeWidth={1.8} aria-hidden="true" /><span className="xy-shell-nav-label">{t(item.label)}</span>{item.view === "providers" && <span className="xy-shell-nav-count">{providerCount}</span>}</button>)}</div>)}</nav>
    <div className="xy-shell-edition">{t("本地工作台")}<span>{t("配置 · 对位 · 观察")}</span></div>
  </aside>;
}
