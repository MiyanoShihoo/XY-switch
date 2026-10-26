import { useLanguage, t, localeTag, translateMessage } from "../../i18n/language";
import { Copy, Gauge, Pencil, Trash2 } from "lucide-react";
import { Badge, Button, EmptyState } from "../../ui";
import { ProviderResponseProbe } from "./ProviderResponseProbe";
import { healthDisplay } from "../../lib/health-format";
import type { Provider, ProviderHealth } from "../../types/provider";
export function ProviderInspector({ provider, health, current, testing, busy, onEdit, onCopy, onDelete, onTest }: { provider?: Provider; health?: ProviderHealth; current: boolean; testing: boolean; busy: boolean; onEdit: (provider: Provider) => void; onCopy: (provider: Provider) => void; onDelete: (provider: Provider) => void; onTest: (id: string) => void }) {
  useLanguage();
  if (!provider) return <EmptyState title={t("选择一个供应商")} description={t("查看连接、模型和最近测试状态。点击供应商卡片切换；详情图标只查看。")} />;
  const display = healthDisplay(provider, health);
  return <div className="form-stack">
    <div className="provider-inspector-title"><h3>{provider.name}</h3><div className="inline-actions">{current && <Badge tone="accent">{t("当前配置")}</Badge>}<Badge tone={provider.enabled ? "neutral" : "warning"}>{provider.enabled ? t("已启用") : t("已禁用")}</Badge></div></div>
    <dl className="key-values"><dt>ID</dt><dd className="mono">{provider.id}</dd><dt>{t("端点")}</dt><dd className="mono">{provider.endpoint}</dd><dt>{t("模型")}</dt><dd className="mono">{provider.model || t("未配置")}</dd><dt>{t("协议")}</dt><dd className="mono">{provider.advanced.upstreamFormat}</dd><dt>{t("URL 模式")}</dt><dd>{provider.advanced.isFullUrl ? t("完整 URL") : t("自动追加路径")}</dd><dt>{t("模型映射")}</dt><dd>{provider.advanced.modelMappings.length} {t("项")}</dd></dl>
    <section className="section"><h3>{t("最近连接测试")}</h3><Badge tone={display.tone === "healthy" ? "success" : display.tone === "danger" ? "danger" : display.tone === "warning" ? "warning" : "neutral"}>{display.tone === "healthy" ? t("测试通过") : provider.enabled && !health ? t("尚未测试") : display.label}</Badge>{health?.lastCheckedAt ? <dl className="key-values"><dt>{t("测试时间")}</dt><dd>{new Date(health.lastCheckedAt < 1e12 ? health.lastCheckedAt * 1000 : health.lastCheckedAt).toLocaleString(localeTag(), { hour12: false })}</dd><dt>{t("延迟")}</dt><dd className="mono">{health.latencyMs != null ? health.latencyMs + " ms" : t("无数据")}</dd><dt>{t("成功率")}</dt><dd className="mono">{health.status === "unknown" ? t("无数据") : health.successRate.toFixed(1) + "%"}</dd></dl> : <p className="page-description">{t("没有连接测试记录。启用或切换配置不代表请求可用。")}</p>}{health?.lastError && <p className="provider-inspector-error">{translateMessage(health.lastError)}</p>}</section>
    <ProviderResponseProbe key={JSON.stringify([provider.id, provider.endpoint, provider.model, provider.enabled, provider.advanced])} provider={provider} busy={busy} />
    <div className="provider-inspector-actions"><Button variant="secondary" icon={<Gauge size={14} />} loading={testing} disabled={busy || !provider.enabled} onClick={() => onTest(provider.id)}>{t("测试连接")}</Button><Button variant="secondary" icon={<Pencil size={14} />} disabled={busy} onClick={() => onEdit(provider)}>{t("编辑配置")}</Button><Button variant="quiet" icon={<Copy size={14} />} disabled={busy} onClick={() => onCopy(provider)}>{t("复制（不含密钥）")}</Button><Button variant="danger" icon={<Trash2 size={14} />} disabled={busy || provider.builtIn} title={provider.builtIn ? t("内置供应商不能删除") : undefined} onClick={() => onDelete(provider)}>{t("删除供应商")}</Button></div>
  </div>;
}
