import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowRight, ArrowUpRight, Cable, RefreshCw, Search } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { Badge, Button, EmptyState, Input, InlineError, LoadingState } from "../../ui";
import type { AppView } from "../../lib/workspace-navigation";
import type { ProxyStatus } from "../../types/proxy";
import { healthDisplay } from "../../lib/health-format";
import { errorText } from "../../lib/error";
import { loadAppTakeovers } from "../../lib/takeover-service";
import { protocolLabels } from "../providers/ProviderRow";
import type { ProvidersController } from "../providers/useProviders";
import "./overview.css";

interface OverviewPageProps {
  providers: ProvidersController;
  proxyStatus: ProxyStatus | null;
  onNavigate: (view: AppView, providerId?: string) => void;
}

export function OverviewPage({ providers, proxyStatus, onNavigate }: OverviewPageProps) {
  const language = useLanguage();
  const [search, setSearch] = useState("");
  const takeovers = useQuery({ queryKey: ["takeovers"], queryFn: loadAppTakeovers });
  const current = providers.providers.find(provider => provider.id === providers.currentId);
  const affected = takeovers.data?.filter(status => status.active) ?? [];
  const scopeKnown = takeovers.data !== undefined && !takeovers.isError;
  const locked = providers.loading || Boolean(providers.readError) || providers.busy || Boolean(providers.testing) || !scopeKnown || takeovers.isFetching;
  const visible = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    return providers.providers.filter(provider => !query || [provider.name, provider.group, provider.model, provider.endpoint].some(value => value.toLocaleLowerCase().includes(query)));
  }, [providers.providers, search]);
  const routeTone = proxyStatus?.lastError ? "danger" : proxyStatus?.running ? "success" : "neutral";
  const routeLabel = !proxyStatus ? t("状态未读取") : proxyStatus.lastError ? t("运行异常") : proxyStatus.running ? t("运行中") : t("已停止");
  const checkedTime = (value: number | null | undefined) => value ? new Intl.DateTimeFormat(language === "en" ? "en-US" : "zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(value < 1e12 ? value * 1000 : value) : t("暂无记录");

  return <div className="workspace-page overview-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("当前工作环境")}</h1><p className="page-description">{t("核对当前选择，比较下一项配置。")}</p></div><Button variant="secondary" icon={<ArrowUpRight size={15} />} iconPosition="end" onClick={() => onNavigate("providers")}>{t("管理供应商")}</Button></header>
    {(providers.error || providers.readError) && <InlineError message={providers.error || providers.readError} onRetry={() => void providers.refresh()} />}
    {providers.message && <p className="overview-operation-result" role="status">{providers.message}</p>}
    {providers.loading && <LoadingState label={t("读取供应商配置…")} />}

    {providers.readError ? <EmptyState icon={<Cable size={24} />} title={t("当前配置状态未知")} description={t("重新读取供应商配置后再切换。")} /> : current ? <section className="overview-current" aria-labelledby="overview-current-title">
      <div className="overview-current-identity"><span className="overview-label" id="overview-current-title">{t("当前配置")}</span><div className="overview-current-title"><h2>{current.name}</h2><Badge tone={current.enabled ? "accent" : "warning"}>{current.enabled ? t("已选择") : t("已禁用")}</Badge></div><span className="overview-current-protocol">{current.group} · {protocolLabels[current.advanced.upstreamFormat]}</span></div>
      <dl className="overview-current-values"><div><dt>{t("默认模型")}</dt><dd><code>{current.model || t("未配置模型")}</code></dd></div><div><dt>{t("服务地址")}</dt><dd><code>{current.endpoint}</code></dd></div></dl>
      <Button variant="quiet" size="sm" icon={<ArrowRight size={14} />} iconPosition="end" onClick={() => onNavigate("provider-detail", current.id)}>{t("查看配置")}</Button>
    </section> : !providers.loading && <EmptyState icon={<Cable size={24} />} title={t("尚未选择当前配置")} description={t("保存供应商的端点与模型，再为客户端配置接入。")} action={<Button onClick={() => onNavigate("providers")}>{t("添加供应商")}</Button>} />}

    {proxyStatus?.settings.showInHome && <section className="overview-runtime" aria-label={t("本地路由")}><div><strong>{t("本地路由")}</strong><Badge tone={routeTone} dot>{routeLabel}</Badge></div><code>{proxyStatus.address || proxyStatus.settings.host + ":" + proxyStatus.settings.port}</code><Button size="sm" variant="quiet" onClick={() => onNavigate("settings")}>{t("路由设置")}</Button>{proxyStatus.lastError && <p role="alert">{proxyStatus.lastError}</p>}</section>}

    <section className="overview-candidates" aria-labelledby="overview-candidates-title">
      <div className="overview-section-heading"><h2 id="overview-candidates-title">{t("选择配置")}<span className="overview-count">{visible.length} / {providers.providers.length}</span></h2><div className="overview-search"><Search size={15} aria-hidden="true" /><Input value={search} onChange={event => setSearch(event.target.value)} aria-label={t("搜索供应商、模型或地址")} placeholder={t("搜索供应商、模型或地址")} /></div></div>
      <div className="overview-switch-scope" data-scope={scopeKnown ? affected.length ? "takeover" : "configuration" : "unknown"} aria-live="polite"><div><strong>{t("切换影响")}</strong>{!scopeKnown ? <p>{takeovers.isError ? t("无法确认接管范围，请刷新后再切换。") : t("正在确认实际接管范围…")}</p> : affected.length ? <p>{t("将同步所有已接管配置：")}<strong>{affected.map(status => status.label).join("、")}</strong></p> : <p>{t("仅更改当前配置；没有客户端被接管。")}</p>}</div><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} loading={takeovers.isFetching} disabled={providers.busy} onClick={() => void takeovers.refetch()}>{t("刷新接管状态")}</Button>{takeovers.isError && <span className="overview-scope-error">{errorText(takeovers.error)}</span>}</div>
      {visible.length ? <div className="overview-candidate-list"><div className="overview-candidate-columns" aria-hidden="true"><span>{t("供应商 / 协议")}</span><span>{t("模型 / 地址")}</span><span>{t("上次测试记录")}</span><span>{t("配置选择")}</span></div>{visible.map(provider => {
        const health = providers.health[provider.id];
        const display = healthDisplay(provider, health);
        const selected = provider.id === providers.currentId;
        return <article className="overview-candidate" key={provider.id} data-current={selected}>
          <div className="overview-candidate-identity"><button type="button" className="overview-configuration-link" onClick={() => onNavigate("provider-detail", provider.id)}>{provider.name}</button><span>{provider.group} · {protocolLabels[provider.advanced.upstreamFormat]}</span></div>
          <div className="overview-candidate-endpoint"><code title={provider.model}>{provider.model || t("未配置模型")}</code><code title={provider.endpoint}>{provider.endpoint}</code></div>
          <div className="overview-candidate-health"><span data-tone={health ? display.tone : "muted"}>{health ? display.label : t("未测试")}{health?.latencyMs != null && <span className="overview-health-latency"> · {health.latencyMs} ms</span>}</span><time>{checkedTime(health?.lastCheckedAt)}</time></div>
          <Button variant={selected ? "quiet" : "secondary"} size="sm" disabled={locked || selected || !provider.enabled} onClick={() => void providers.activate(provider.id)} aria-label={selected ? t("当前配置：{name}", { name: provider.name }) : t("设为当前配置：{name}", { name: provider.name })}>{selected ? t("当前") : provider.enabled ? t("设为当前") : t("已禁用")}</Button>
        </article>;
      })}</div> : !providers.loading && <p className="overview-no-results">{search ? t("没有匹配的供应商") : t("保存的连接将在这里显示。")}</p>}
      <div className="overview-candidate-footnote"><p>{t("历史记录不代表当前连通状态。需要时重新测试。")}</p><Button variant="quiet" size="sm" onClick={() => onNavigate("diagnostics", current?.id)}>{t("打开连接诊断")}</Button></div>
    </section>
    <nav className="overview-next" aria-label={t("继续工作")}><Button variant="quiet" onClick={() => onNavigate("clients")}>{t("查看接入状态")}<ArrowUpRight size={14} aria-hidden="true" /></Button><Button variant="quiet" onClick={() => onNavigate("sessions")}>{t("继续会话")}<ArrowUpRight size={14} aria-hidden="true" /></Button><Button variant="quiet" onClick={() => onNavigate("usage")}>{t("查看请求用量")}<ArrowUpRight size={14} aria-hidden="true" /></Button></nav>
  </div>;
}
