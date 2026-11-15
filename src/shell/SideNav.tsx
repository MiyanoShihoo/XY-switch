import { useEffect, useRef } from "react";
import { Activity, Blocks, Cable, ChartNoAxesCombined, History, KeyRound, Laptop, LayoutGrid, MessageSquareText, Server, Settings2, ShieldCheck, Sparkles, Store } from "lucide-react";
import { t, useLanguage } from "../i18n/language";
import type { AppView } from "../lib/workspace-navigation";

export const navigationSections = ["总览", "连接", "工具", "记录", "设置"] as const;
export const navigation = [
  { view: "overview", label: "总览", section: "总览" },
  { view: "providers", label: "供应商", section: "连接" },
  { view: "clients", label: "客户端接入", section: "连接" },
  { view: "accounts", label: "账号池", section: "连接" },
  { view: "diagnostics", label: "连接诊断", section: "连接" },
  { view: "plugins", label: "本地插件", section: "工具" },
  { view: "marketplace", label: "插件市场", section: "工具" },
  { view: "mcp", label: "MCP 服务器", section: "工具" },
  { view: "prompts", label: "提示词", section: "工具" },
  { view: "skills", label: "Skills", section: "工具" },
  { view: "sessions", label: "会话", section: "记录" },
  { view: "usage", label: "用量与审计", section: "记录" },
  { view: "settings", label: "设置", section: "设置" },
  { view: "governance", label: "安全与验收", section: "设置" },
] as const;

type Section = typeof navigationSections[number];
export function workspaceSection(view: AppView) {
  return navigation.find(item => item.view === (view === "provider-detail" ? "providers" : view))?.section ?? "总览";
}
const sectionIcons = { 总览: LayoutGrid, 连接: Cable, 工具: Blocks, 记录: History, 设置: Settings2 };
const viewIcons = { overview: LayoutGrid, providers: Cable, clients: Laptop, accounts: KeyRound, diagnostics: Activity, plugins: Blocks, marketplace: Store, mcp: Server, prompts: MessageSquareText, skills: Sparkles, sessions: History, usage: ChartNoAxesCombined, settings: Settings2, governance: ShieldCheck };
interface NavigationProps { activeView: AppView; onNavigate: (view: AppView) => void; providerCount?: number; }

export function ActivityNav({ activeView, onNavigate }: NavigationProps) {
  useLanguage();
  const section = workspaceSection(activeView);
  const recent = useRef<Partial<Record<Section, AppView>>>({});
  useEffect(() => { recent.current[section] = activeView === "provider-detail" ? "providers" : activeView; }, [activeView, section]);
  return <nav className="xy-shell-activity-nav" aria-label={t("工作区分组")}>
    {navigationSections.map(item => {
      const Icon = sectionIcons[item];
      const destination = navigation.find(entry => entry.section === item)!.view;
      return <button type="button" key={item} className="xy-shell-activity" data-section={destination} data-default-view={destination} data-active={section === item} aria-current={section === item ? "true" : undefined} title={t(item)} onClick={() => onNavigate(recent.current[item] ?? destination)}><Icon size={20} strokeWidth={1.55} aria-hidden="true" /><span>{t(item)}</span></button>;
    })}
  </nav>;
}

export function SectionNav({ activeView, onNavigate, providerCount }: NavigationProps) {
  const language = useLanguage();
  const navigationRef = useRef<HTMLElement>(null);
  const selected = activeView === "provider-detail" ? "providers" : activeView;
  useEffect(() => { navigationRef.current?.querySelector<HTMLElement>('[aria-current="page"]')?.scrollIntoView?.({ block: "nearest", inline: "nearest" }); }, [activeView, language]);
  if (workspaceSection(activeView) === "总览") return <span className="xy-shell-location">{t("总览")}</span>;
  return <nav ref={navigationRef} className="xy-shell-section-nav" aria-label={t("主导航")}>
    {navigation.filter(item => item.section === workspaceSection(activeView)).map(item => <button key={item.view} type="button" className="xy-shell-nav-item" data-view={item.view} data-active={selected === item.view} aria-current={selected === item.view ? "page" : undefined} onClick={() => onNavigate(item.view)}><span>{t(item.label)}</span>{item.view === "providers" && providerCount !== undefined && <span className="xy-shell-nav-count" aria-hidden="true">{providerCount}</span>}</button>)}
  </nav>;
}

export function SideNav({ activeView, onNavigate, providerCount }: NavigationProps) {
  const language = useLanguage();
  const navigationRef = useRef<HTMLElement>(null);
  const selected = activeView === "provider-detail" ? "providers" : activeView;
  useEffect(() => { navigationRef.current?.querySelector<HTMLElement>('[aria-current="page"]')?.scrollIntoView?.({ block: "nearest", inline: "nearest" }); }, [activeView, language]);
  return <nav ref={navigationRef} className="xy-shell-nav" aria-label={t("主导航")}>
    {navigationSections.map(section => <div className="xy-shell-nav-group" key={section}>
      {section !== "总览" && <span className="xy-shell-nav-heading">{t(section)}</span>}
      {navigation.filter(item => item.section === section).map(item => {
        const Icon = viewIcons[item.view];
        return <button key={item.view} type="button" className="xy-shell-nav-item" data-view={item.view} data-active={selected === item.view} aria-current={selected === item.view ? "page" : undefined} title={t(item.label)} onClick={() => onNavigate(item.view)}><Icon size={17} strokeWidth={1.6} aria-hidden="true" /><span>{t(item.label)}</span>{item.view === "providers" && providerCount !== undefined && <span className="xy-shell-nav-count" aria-hidden="true">{providerCount}</span>}</button>;
      })}
    </div>)}
  </nav>;
}
