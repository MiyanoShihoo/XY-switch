import { useLanguage, t } from "../i18n/language";
import { ChevronRight, Moon, Sun } from "lucide-react";
import { IconButton } from "../ui";
import type { AppView } from "./AppShell";
import { navigation } from "./SideNav";
export function ContextBar({ activeView, currentProviderName, theme, onToggleTheme }: { activeView: AppView; currentProviderName?: string; theme: "light" | "dark"; onToggleTheme: () => void }) {
  useLanguage();
  const label = activeView === "provider-detail" ? "配置详情" : navigation.find(item => item.view === activeView)?.label ?? "工作台";
  return <header className="xy-shell-contextbar">
    <div className="xy-shell-context-page"><span>XY Switch</span><ChevronRight size={14} aria-hidden="true" /><strong>{t(label)}</strong></div>
    <div className="xy-shell-context-current"><span className="xy-shell-context-label">{t("当前配置")}</span><strong title={currentProviderName}>{currentProviderName || t("未设置供应商")}</strong></div>
    <IconButton aria-label={theme === "dark" ? t("切换到浅色") : t("切换到深色")} onClick={onToggleTheme}>{theme === "dark" ? <Sun size={18} aria-hidden="true" /> : <Moon size={18} aria-hidden="true" />}</IconButton>
  </header>;
}
