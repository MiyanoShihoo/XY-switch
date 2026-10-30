import { useLanguage, t } from "../../i18n/language";
import { ArrowUpRight, GripVertical } from "lucide-react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Badge, Button, IconButton } from "../../ui";
import { healthDisplay } from "../../lib/health-format";
import type { Provider, ProviderHealth } from "../../types/provider";

export interface ProviderRowProps {
  provider: Provider; health?: ProviderHealth; current: boolean; testing: boolean; busy: boolean;
  onInspect: (provider: Provider) => void; onActivate: (id: string) => void;
}
export const protocolLabels = { openai_responses: "Responses", openai_chat: "Chat Completions", anthropic: "Messages" } as const;
export function ProviderRow({ provider, health, current, testing, busy, onInspect, onActivate }: ProviderRowProps) {
  useLanguage();
  const sortable = useSortable({ id: provider.id, disabled: busy });
  const display = healthDisplay(provider, health);
  const tone = testing ? "info" : display.tone === "healthy" ? "success" : display.tone === "danger" ? "danger" : display.tone === "warning" ? "warning" : "neutral";
  const choose = () => { if (busy || sortable.isDragging) return; if (!provider.enabled) onInspect(provider); else if (!current) onActivate(provider.id); };
  return <div ref={sortable.setNodeRef} className="provider-row" data-provider-id={provider.id} data-current={current} data-disabled={!provider.enabled} data-dragging={sortable.isDragging}
    style={{ transform: CSS.Transform.toString(sortable.transform), transition: sortable.transition }} role="listitem"
    onClick={event => { if (!(event.target as Element).closest("[data-row-action]")) choose(); }}>
    <span className="provider-row__handle" data-row-action="drag" onClick={event => event.stopPropagation()}><IconButton size="sm" aria-label={t("拖动排序 ") + provider.name} disabled={busy} {...sortable.attributes} {...sortable.listeners}><GripVertical size={16} aria-hidden="true" /></IconButton></span>
    <button type="button" className="provider-row__identity" disabled={busy} aria-label={(provider.enabled ? t("切换到 ") : t("查看已禁用配置 ")) + provider.name} aria-pressed={current}>
      <span className="provider-mark" aria-hidden="true">{provider.name.slice(0, 1).toUpperCase()}</span><span className="provider-row__identity-copy"><span className="provider-row__name"><strong>{provider.name}</strong>{current && <Badge tone="accent">{t("当前")}</Badge>}</span><code className="provider-row__endpoint" title={provider.endpoint}>{provider.endpoint}</code></span>
    </button>
    <div className="provider-row__model"><code title={provider.model}>{provider.model || t("未配置")}</code><span>{provider.group} · {protocolLabels[provider.advanced.upstreamFormat]}</span></div>
    <div className="provider-row__health"><Badge tone={tone} dot>{testing ? t("测试中") : display.tone === "healthy" ? t("测试通过") : provider.enabled && !health ? t("未测试") : display.label}</Badge>{health?.latencyMs != null && !testing && <span className="mono">{health.latencyMs} ms</span>}</div>
    <Button data-row-action="inspect" variant="quiet" size="sm" className="provider-row__configure" aria-label={t("查看 ") + provider.name + t(" 详情")} title={t("查看配置，不切换供应商")} disabled={busy} icon={<ArrowUpRight size={16} />} iconPosition="end" onClick={event => { event.stopPropagation(); onInspect(provider); }}>{t("配置")}</Button>
  </div>;
}
