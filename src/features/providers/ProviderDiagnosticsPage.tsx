import { useLanguage, t, localeTag } from "../../i18n/language";
import { useId, useRef, useState, type KeyboardEvent } from "react";
import { Gauge } from "lucide-react";
import { Badge, Button, EmptyState, Field, LoadingState, Select } from "../../ui";
import { healthDisplay } from "../../lib/health-format";
import { ProviderFeedback } from "./ProviderFeedback";
import { ProviderHealthSummary } from "./ProviderHealthSummary";
import { ProviderResponseProbe } from "./ProviderResponseProbe";
import type { ProvidersController } from "./useProviders";
import "./providers.css";
export const diagnosticTabs = [{ id: "connection", label: "连接测试" }, { id: "model", label: "模型响应与验证" }] as const;
export function ProviderDiagnosticsPage({ controller: c, providerId, onSelect, onConfigure, onProviders }: { controller: ProvidersController; providerId?: string; onSelect: (id: string) => void; onConfigure: (id: string) => void; onProviders: () => void }) {
  useLanguage();
  const [tab, setTab] = useState<(typeof diagnosticTabs)[number]["id"]>("connection");
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const tabId = useId();
  const provider = providerId ? c.providers.find(item => item.id === providerId) : c.providers.find(item => item.id === c.currentId) ?? c.providers[0];
  const busy = c.busy || Boolean(c.testing);
  const selectTab = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = event.key === "ArrowRight" ? (index + 1) % diagnosticTabs.length : event.key === "ArrowLeft" ? (index + diagnosticTabs.length - 1) % diagnosticTabs.length : event.key === "Home" ? 0 : event.key === "End" ? diagnosticTabs.length - 1 : null;
    if (next === null) return;
    event.preventDefault(); setTab(diagnosticTabs[next].id); tabRefs.current[next]?.focus();
  };
  return <div className="workspace-page provider-diagnostics-page">
    <header className="page-heading"><div className="page-heading__copy"><span className="page-eyebrow">{t("观测工作台")}</span><h1>{t("连接诊断")}</h1><p className="page-description">{t("连接测试、模型响应与证据集中在这里。查看不切换当前配置，也不会自动发起上游请求。")}</p></div><Button variant="secondary" icon={<Gauge size={16} />} loading={c.testing === "all"} disabled={busy || c.loading || !c.providers.some(item => item.enabled)} onClick={() => void c.test()}>{t("测试全部")}</Button></header>
    <ProviderFeedback controller={c} />
    {!c.loading && c.providers.length > 0 && <section className="provider-diagnostic-matrix" aria-labelledby={tabId + "-comparison"}>
      <div className="section-heading"><h2 id={tabId + "-comparison"}>{t("连接测试比较")}</h2><span className="muted">{t("共 {count} 个配置", { count: c.providers.length })}</span></div>
      <p className="page-description">{t("配置状态与测试证据分列；选择一行查看记录，不会切换当前配置。")}</p>
      <table><thead><tr><th scope="col">{t("供应商 / 模型")}</th><th scope="col">{t("配置状态")}</th><th scope="col">{t("最近测试")}</th><th scope="col">{t("测试时间")}</th><th scope="col">{t("延迟")}</th><th scope="col">{t("成功率")}</th></tr></thead><tbody>
        {c.providers.map(item => {
          const health = c.health[item.id];
          const display = healthDisplay({ ...item, enabled: true }, health);
          const testing = c.testing === "all" ? item.enabled : c.testing === item.id;
          const tone = testing ? "info" : display.tone === "healthy" ? "success" : display.tone === "danger" ? "danger" : display.tone === "warning" ? "warning" : "neutral";
          const checked = health?.lastCheckedAt ? new Date(health.lastCheckedAt < 1e12 ? health.lastCheckedAt * 1000 : health.lastCheckedAt).toLocaleString(localeTag(), { hour12: false }) : t("无记录");
          return <tr key={item.id} data-provider-id={item.id} data-selected={provider?.id === item.id}>
            <td><button type="button" className="provider-diagnostic-choice" aria-label={t("查看 {provider} 的证据", { provider: item.name })} aria-pressed={provider?.id === item.id} disabled={busy} onClick={() => onSelect(item.id)}><strong>{item.name}</strong><code>{item.model || t("未配置")}</code></button></td>
            <td data-label={t("配置状态")}><div className="provider-diagnostic-state"><span>{item.enabled ? t("已启用") : t("已禁用")}</span>{c.currentId === item.id && <Badge tone="accent">{t("当前配置")}</Badge>}</div></td>
            <td data-label={t("最近测试")}><Badge tone={tone}>{testing ? t("测试中") : display.tone === "healthy" ? t("测试通过") : !health ? t("尚未测试") : display.label}</Badge></td>
            <td data-label={t("测试时间")} className="provider-diagnostic-time">{checked}</td>
            <td data-label={t("延迟")} className="mono">{health?.latencyMs != null ? health.latencyMs + " ms" : "—"}</td>
            <td data-label={t("成功率")} className="mono">{health && health.status !== "unknown" ? health.successRate.toFixed(1) + "%" : "—"}</td>
          </tr>;
        })}
      </tbody></table>
    </section>}
    {c.loading ? <LoadingState label={t("读取供应商配置…")} /> : !provider ? <EmptyState title={c.providers.length ? t("所选供应商已不存在") : t("尚未配置供应商")} description={t("返回供应商列表，选择其他配置。")} action={<Button variant="secondary" onClick={onProviders}>{t("返回供应商")}</Button>} /> : <>
      <div className="provider-evidence-heading"><h2>{t("选中证据")}</h2><span className="muted">{t("选择查看对象")}</span></div>
      <section className="provider-diagnostic-context" aria-label={t("诊断对象")}><Field label={t("诊断供应商")} description={t("只切换查看对象，不修改客户端配置。")}><Select aria-label={t("诊断供应商")} value={provider.id} disabled={busy} onChange={event => onSelect(event.target.value)}>{c.providers.map(item => <option key={item.id} value={item.id}>{item.name}{item.enabled ? "" : " · " + t("已禁用")}</option>)}</Select></Field><div className="provider-diagnostic-context__summary"><div className="inline-actions"><strong>{provider.name}</strong>{provider.id === c.currentId && <Badge tone="accent">{t("当前配置")}</Badge>}<Badge tone={provider.enabled ? "neutral" : "warning"}>{provider.enabled ? t("已启用") : t("已禁用")}</Badge></div><code>{provider.endpoint}</code></div></section>
      <div className="provider-diagnostic-tabs" role="tablist" aria-label={t("诊断板块")}>{diagnosticTabs.map((item, index) => <button key={item.id} type="button" role="tab" id={tabId + "-" + item.id} aria-selected={tab === item.id} aria-controls={tabId + "-panel-" + item.id} tabIndex={tab === item.id ? 0 : -1} ref={node => { tabRefs.current[index] = node; }} onClick={() => setTab(item.id)} onKeyDown={event => selectTab(event, index)}>{t(item.label)}</button>)}</div>
      <div role="tabpanel" id={tabId + "-panel-connection"} aria-labelledby={tabId + "-connection"} tabIndex={0} className="provider-diagnostic-panel" hidden={tab !== "connection"}>
        <ProviderHealthSummary provider={provider} health={c.health[provider.id]} busy={busy} testing={c.testing === "all" || c.testing === provider.id} onTest={() => void c.test(provider.id)} onConfigure={() => onConfigure(provider.id)} />
      </div>
      <div role="tabpanel" id={tabId + "-panel-model"} aria-labelledby={tabId + "-model"} tabIndex={0} className="provider-diagnostic-panel" hidden={tab !== "model"}>
        <h2 className="sr-only">{t("模型响应与验证")}</h2>
        {tab === "model" && <div className="provider-detail-card provider-diagnostics-module"><ProviderResponseProbe key={JSON.stringify([provider.id, provider.endpoint, provider.model, provider.enabled, provider.advanced])} provider={provider} busy={busy} /></div>}
      </div>
    </>}
  </div>;
}
