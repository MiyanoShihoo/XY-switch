import type { ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import type { ProviderAdvancedConfig } from "../../types/provider";

export type ProviderAdvancedSection = "models" | "request" | "headers" | "body" | "codex" | "claude";
export type ProviderSection = "connection" | "advanced" | ProviderAdvancedSection;
export const providerSections: { id: ProviderAdvancedSection; label: string }[] = [
  { id: "models", label: "模型映射" },
  { id: "request", label: "请求选项" },
  { id: "headers", label: "请求头 Headers" },
  { id: "body", label: "请求体 Body" },
  { id: "codex", label: "Codex 配置" },
  { id: "claude", label: "Claude 配置" },
];

export function ProviderSectionNavigation({ section, onSelect }: { section: ProviderSection; onSelect: (section: ProviderSection) => void }) {
  useLanguage();
  return <nav className="provider-inline-jump" aria-label={t("配置章节")}>{([{ id: "connection", label: "连接与凭据" }, ...providerSections] as { id: ProviderSection; label: string }[]).map(item => <button type="button" key={item.id} aria-current={section === item.id ? "location" : undefined} onClick={() => onSelect(item.id)}>{t(item.label)}</button>)}</nav>;
}

export function initialProviderSections(section: ProviderSection): Partial<Record<ProviderAdvancedSection, boolean>> {
  return section === "connection" ? {} : { [section === "advanced" ? "request" : section]: true };
}

export function providerSectionSummary(value: ProviderAdvancedConfig, section: ProviderAdvancedSection): string {
  if (section === "models") return value.modelMappings.length ? t("{count} 个映射", { count: value.modelMappings.length }) : t("使用默认模型");
  if (section === "headers" || section === "body") {
    const count = Object.keys(section === "headers" ? value.headerOverrides : value.bodyOverrides).length;
    return count ? t("{count} 项覆盖", { count }) : t("未设置");
  }
  if (section === "request") return value.customUserAgent ? t("自定义 User-Agent") : t("由代理管理");
  if (section === "claude") return t(value.claude.disableArtifactTool ? "禁用 Artifact 工具" : "默认行为");
  const changed = value.codex.remoteCompression || value.codex.applyCommonConfig || value.codex.context1m || value.codex.configToml.trim() || value.codex.compactionThreshold !== 900000;
  return t(changed ? "已配置" : "默认行为");
}

export function ProviderSectionDisclosure({ id, title, summary, open, onOpenChange, children }: {
  id: string; title: string; summary: string; open: boolean; onOpenChange: (open: boolean) => void; children: ReactNode;
}) {
  useLanguage();
  return <section className="provider-task-section" data-provider-section={id}>
    <h2><button id={id} type="button" aria-expanded={open} aria-controls={id + "-content"} onClick={() => onOpenChange(!open)}>
      <ChevronDown size={15} aria-hidden="true" /><span>{title}</span><small>{summary}</small>
    </button></h2>
    <div id={id + "-content"} className="provider-task-section__body" hidden={!open} aria-labelledby={id}>{children}</div>
  </section>;
}
