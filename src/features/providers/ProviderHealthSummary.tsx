import { useLanguage, t, localeTag, translateMessage } from "../../i18n/language";
import { ArrowUpRight, Gauge } from "lucide-react";
import { Badge, Button, InlineError } from "../../ui";
import { healthDisplay } from "../../lib/health-format";
import type { Provider, ProviderHealth } from "../../types/provider";
export function ProviderHealthSummary({ provider, health, busy, testing, onTest, onConfigure }: { provider: Provider; health?: ProviderHealth; busy: boolean; testing: boolean; onTest: () => void; onConfigure: () => void }) {
  useLanguage();
  const display = healthDisplay(provider, health);
  const tone = testing ? "info" : display.tone === "healthy" ? "success" : display.tone === "danger" ? "danger" : display.tone === "warning" ? "warning" : "neutral";
  const checked = health?.lastCheckedAt ? new Date(health.lastCheckedAt < 1e12 ? health.lastCheckedAt * 1000 : health.lastCheckedAt).toLocaleString(localeTag(), { hour12: false }) : t("无记录");
  return <section className="provider-detail-card provider-health-summary" aria-labelledby="provider-health-title">
    <div className="section-heading"><div className="provider-health-heading"><h2 id="provider-health-title">{t("最近连接测试")}</h2><Badge dot tone={tone}>{testing ? t("测试中") : display.tone === "healthy" ? t("测试通过") : provider.enabled && !health ? t("尚未测试") : display.label}</Badge></div><Button icon={<Gauge size={16} />} loading={testing} disabled={busy || !provider.enabled} onClick={onTest}>{t("测试连接")}</Button></div>
    <dl className="provider-health-metrics"><div><dt>{t("延迟")}</dt><dd>{health?.latencyMs != null ? <>{health.latencyMs}<span> ms</span></> : "—"}</dd></div><div><dt>{t("成功率")}</dt><dd>{health && health.status !== "unknown" ? health.successRate.toFixed(1) + "%" : "—"}</dd></div><div><dt>{t("测试时间")}</dt><dd className="provider-health-date">{checked}</dd></div></dl>
    {health?.lastError && <InlineError message={translateMessage(health.lastError)} />}
    {!health?.lastCheckedAt && <p className="page-description">{t("没有连接测试记录。启用或切换配置不代表请求可用。")}</p>}
    {!provider.enabled && <p className="notice">{t("该供应商已禁用，可查看历史结果；启用后才能发起连接测试。")}</p>}
    <div className="provider-health-footer"><p className="page-description">{t("仅测试选中的供应商，不改变当前配置。")}</p><Button variant="quiet" size="sm" icon={<ArrowUpRight size={16} />} onClick={onConfigure}>{t("查看配置")}</Button></div>
  </section>;
}
