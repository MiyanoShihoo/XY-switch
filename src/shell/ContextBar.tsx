import { useLanguage, t } from "../i18n/language";
import { Moon, Sun } from "lucide-react";
import { IconButton } from "../ui";
import type { AppView } from "./AppShell";
import { navigation } from "./SideNav";
export function ContextBar({ activeView, currentProviderName, theme, onToggleTheme }: { activeView: AppView; currentProviderName?: string; theme: "light" | "dark"; onToggleTheme: () => void }) {
  useLanguage();
  return <header className="xy-shell-contextbar"><div className="xy-shell-context-page">{t("工作台")}<span aria-hidden="true">/</span><strong>{t(navigation.find(item => item.view === activeView)?.label ?? "")}</strong></div><div className="xy-shell-context-current"><span>{t("当前配置")}</span><strong title={currentProviderName}>{currentProviderName || t("未设置供应商")}</strong></div><IconButton aria-label={theme === "dark" ? t("切换到浅色") : t("切换到深色")} onClick={onToggleTheme}>{theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}</IconButton></header>;
}
