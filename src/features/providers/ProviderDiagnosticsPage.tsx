import { useLanguage, t } from "../../i18n/language";
import { useId, useRef, useState, type KeyboardEvent } from "react";
import { Gauge } from "lucide-react";
import { Badge, Button, EmptyState, Field, LoadingState, Select } from "../../ui";
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
    {c.loading ? <LoadingState label={t("读取供应商配置…")} /> : !provider ? <EmptyState title={c.providers.length ? t("所选供应商已不存在") : t("尚未配置供应商")} description={t("返回供应商列表，选择其他配置。")} action={<Button variant="secondary" onClick={onProviders}>{t("返回供应商")}</Button>} /> : <>
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
