import { useLanguage, t } from "../../i18n/language";
import { ArrowUpRight, GripVertical, Pencil } from "lucide-react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Badge, Button, IconButton } from "../../ui";
import { healthDisplay } from "../../lib/health-format";
import type { Provider, ProviderHealth } from "../../types/provider";

export interface ProviderRowProps {
  provider: Provider; health?: ProviderHealth; current: boolean; testing: boolean; busy: boolean; activationDisabled?: boolean;
  onInspect: (provider: Provider) => void; onActivate: (id: string) => void; onEditModel?: (id: string) => void;
}
export const protocolLabels = { openai_responses: "Responses", openai_chat: "Chat Completions", anthropic: "Messages" } as const;
export function ProviderRow({ provider, health, current, testing, busy, activationDisabled = false, onInspect, onActivate, onEditModel }: ProviderRowProps) {
  useLanguage();
  const sortable = useSortable({ id: provider.id, disabled: busy || activationDisabled });
  const display = healthDisplay(provider, health);
  const tone = testing ? "info" : display.tone === "healthy" ? "success" : display.tone === "danger" ? "danger" : display.tone === "warning" ? "warning" : "neutral";
  return <div ref={sortable.setNodeRef} className="provider-row" data-provider-id={provider.id} data-current={current} data-disabled={!provider.enabled} data-dragging={sortable.isDragging}
    style={{ transform: CSS.Transform.toString(sortable.transform), transition: sortable.transition }} role="listitem">
    <span className="provider-row__handle" data-row-action="drag"><IconButton size="sm" aria-label={t("拖动排序 ") + provider.name} disabled={busy || activationDisabled} {...sortable.attributes} {...sortable.listeners}><GripVertical size={16} aria-hidden="true" /></IconButton></span>
    <button type="button" className="provider-row__identity" disabled={busy} aria-label={t("查看 ") + provider.name + t(" 详情")} title={t("查看配置，不切换供应商")} onClick={() => { if (!sortable.isDragging) onInspect(provider); }}>
      <span className="provider-row__identity-copy"><span className="provider-row__name"><strong>{provider.name}</strong>{current && <Badge tone="accent">{t("当前配置")}</Badge>}{!provider.enabled && <Badge tone="warning">{t("已禁用")}</Badge>}<ArrowUpRight size={13} aria-hidden="true" /></span><code className="provider-row__endpoint" title={provider.endpoint}>{provider.endpoint}</code></span>
    </button>
    <div className="provider-row__model"><span className="provider-row__target">{protocolLabels[provider.advanced.upstreamFormat]}</span><button type="button" data-row-action="model" className="provider-model-action" aria-label={t("修改 {name} 的默认模型", { name: provider.name })} disabled={busy || activationDisabled || !onEditModel} onClick={() => onEditModel?.(provider.id)}><code>{provider.model || t("未配置模型")}</code><Pencil size={12} aria-hidden="true" /></button></div>
    <div className="provider-row__health"><Badge tone={tone} dot>{testing ? t("测试中") : display.tone === "healthy" ? t("测试通过") : provider.enabled && !health ? t("未测试") : display.label}</Badge>{health?.latencyMs != null && !testing && <span className="mono">{health.latencyMs} ms</span>}</div>
    <div className="provider-row__actions"><Button data-row-action="activate" variant="quiet" size="sm" aria-label={t("切换到 ") + provider.name} disabled={busy || activationDisabled || current || !provider.enabled} onClick={() => onActivate(provider.id)}>{current ? t("正在使用") : t("设为当前")}</Button></div>
  </div>;
}
