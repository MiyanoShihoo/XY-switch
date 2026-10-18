import { Moon, Sun } from "lucide-react";
import { IconButton } from "../ui";
import type { AppView } from "./AppShell";
import { navigation } from "./SideNav";
export function ContextBar({ activeView, currentProviderName, theme, onToggleTheme }: { activeView: AppView; currentProviderName?: string; theme: "light" | "dark"; onToggleTheme: () => void }) {
  return <header className="xy-shell-contextbar"><div className="xy-shell-context-page">工作台<span aria-hidden="true">/</span><strong>{navigation.find(item => item.view === activeView)?.label}</strong></div><div className="xy-shell-context-current"><span>当前配置</span><strong title={currentProviderName}>{currentProviderName || "未设置供应商"}</strong></div><IconButton aria-label={theme === "dark" ? "切换到浅色" : "切换到深色"} onClick={onToggleTheme}>{theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}</IconButton></header>;
}
