import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ShieldCheck } from "lucide-react";
import { useLanguage, t } from "../../i18n/language";
import { governanceNative, loadAccessPolicy, loadGovernanceEvents, loadGovernanceOverview, loadMonitorPolicy } from "../../lib/governance-service";
import { usePlatformCapabilities } from "../../lib/platform-service";
import type { Provider } from "../../types/provider";
import type { ProxyStatus } from "../../types/proxy";
import { Badge, Button, Card, InlineError, Stat, Tabs } from "../../ui";
import { AccessPanel } from "./AccessPanel";
import { BudgetPanel } from "./BudgetPanel";
import { MonitorPanel } from "./MonitorPanel";
import { VersionsPanel } from "./VersionsPanel";
import { MountsPanel } from "./MountsPanel";
import { checkStatus, stamp, usd } from "./shared";
import "./governance.css";

export function GovernancePage({ providers, proxyStatus, onRestored }: { providers: Provider[]; proxyStatus: ProxyStatus | null; onRestored: () => Promise<void> }) {
  useLanguage();
  const [tab, setTab] = useState("overview"), platform = usePlatformCapabilities();
  const readOnly = !platform.data || platform.data.mobile;
  const tabs = [{ value: "overview", label: t("只读概览") }, ...(!readOnly ? [{ value: "access", label: t("访问控制") }, { value: "budget", label: t("硬预算") }] : []), { value: "monitor", label: t("验收监控") }, ...(!readOnly ? [{ value: "versions", label: t("配置版本") }, { value: "mounts", label: t("显式挂载") }] : []), { value: "audit", label: t("治理审计") }];
  const active = tabs.some(item => item.value === tab) ? tab : "overview";
  return <div className="workspace-page governance-page"><header className="page-header"><div><h1><ShieldCheck size={23} aria-hidden="true" /> {t("安全与验收")}</h1><p className="page-description">{t("先预览、再确认；让访问、花费与外部文件变更都有明确边界。")}</p></div></header>
    {!governanceNative() && <p className="gov-warning">{t("浏览器中仅展示预览；没有真实监控数据，也不会保存设置或发送收费请求。")}</p>}
    {platform.data?.mobile && <p className="gov-warning">{t("移动端为只读视图，仅显示本机数据；尚未连接其他设备。不会接管桌面客户端，也不保证后台代理常驻。")}</p>}
    {platform.error && <InlineError message={String(platform.error)} onRetry={() => void platform.refetch()} />}
    <Tabs id="governance" label={t("安全与验收页面")} value={active} onChange={setTab} items={tabs} />
    <div role="tabpanel" id={"governance-panel-" + active} aria-labelledby={"governance-tab-" + active} className="gov-panel">
      {active === "overview" && <Overview providers={providers} proxyStatus={proxyStatus} />}
      {active === "access" && !readOnly && <AccessPanel proxyStopped={proxyStatus?.running === false} />}
      {active === "budget" && !readOnly && <BudgetPanel />}
      {active === "monitor" && <MonitorPanel providers={providers} proxyRunning={proxyStatus?.running === true} readOnly={readOnly} />}
      {active === "versions" && !readOnly && <VersionsPanel proxyStopped={proxyStatus?.running === false} onRestored={onRestored} />}
      {active === "mounts" && !readOnly && <MountsPanel />}
      {active === "audit" && <AuditPanel />}
    </div>
  </div>;
}
function Overview({ providers, proxyStatus }: { providers: Provider[]; proxyStatus: ProxyStatus | null }) {
  const query = useQuery({ queryKey: ["governance", "overview"], queryFn: loadGovernanceOverview, refetchInterval: 15000 });
  const access = useQuery({ queryKey: ["governance", "access"], queryFn: loadAccessPolicy });
  const monitor = useQuery({ queryKey: ["governance", "monitor"], queryFn: loadMonitorPolicy });
  const data = query.data;
  return <div className="gov-stack">
    <Card className="gov-card"><div className="section-heading"><h2>{t("运行与保护状态")}</h2><Button variant="quiet" disabled={query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button></div>
      {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
      <div className="gov-stats"><Stat label={t("代理状态")} value={proxyStatus ? proxyStatus.running ? t("运行中") : t("已停止") : t("状态未知")} /><Stat label={t("访问认证")} value={access.data ? access.data.requireToken ? t("要求令牌") : t("兼容模式") : "—"} /><Stat label={t("硬预算")} value={data ? data.budget.policy.enabled ? t("已启用") : t("已停用") : "—"} /><Stat label={t("合成监控")} value={monitor.data ? monitor.data.enabled ? t("已启用") : t("已停用") : "—"} /></div>
      {!access.data?.requireToken && access.data && <p className="gov-warning">{t("兼容模式：本地模型接口不要求认证。只读状态接口始终要求令牌。")}</p>}
      <p className="page-description">{t("只读接口 GET /xy/status 使用 monitor 令牌，不返回供应商端点、密钥、响应正文或上游原始错误。代理仍仅监听回环地址。")}</p>
      {access.error && <InlineError message={String(access.error)} onRetry={() => void access.refetch()} />}{monitor.error && <InlineError message={String(monitor.error)} onRetry={() => void monitor.refetch()} />}
    </Card>
    {data && <><Card className="gov-card"><h2>{t("用量与预留")}</h2><div className="gov-stats"><Stat label={t("累计代理请求")} value={data.requests.totalRequests} /><Stat label={t("今日预留 Token")} value={data.budget.reservedTokens.toLocaleString()} detail={t("上限") + " " + data.budget.policy.dailyTokens.toLocaleString()} /><Stat label={t("今日预留费用")} value={usd(data.budget.reservedMicrousd)} detail={t("上限") + " " + usd(data.budget.policy.dailyMicrousd)} /></div><p className="page-description">{t("预留额度不等同实际账单；实际用量与未知费用请查看用量与审计。")}</p></Card>
      <Card className="gov-card"><h2>{t("供应商状态")}</h2><ul className="gov-list">{data.providers.map(p => <li key={p.providerId}><div><strong>{providers.find(item => item.id === p.providerId)?.name ?? p.providerId}</strong><p>{p.latencyMs ?? "—"} ms · {t("成功率")} {p.successRate.toFixed(1)}% · {stamp(p.lastCheckedAt / 1000)}</p></div><Badge tone={p.status === "healthy" ? "success" : p.status === "unknown" ? "neutral" : "danger"}>{p.status === "healthy" ? t("健康") : p.status === "unhealthy" ? t("异常") : p.status === "circuit_open" ? t("熔断中") : t("状态未知")}</Badge></li>)}</ul>{data.providers.length === 0 && <p>{t("暂无供应商状态数据")}</p>}</Card>
      <Card className="gov-card"><h2>{t("最近验收状态")}</h2><ul className="gov-list">{data.checks.map(run => <li key={run.id}><div><strong>{providers.find(p => p.id === run.providerId)?.name ?? run.providerId}</strong><p>{run.mode} · {stamp(run.createdAt)} · {run.latencyMs} ms</p></div><Badge tone={run.status === "passed" ? "success" : run.status === "failed" ? "danger" : "neutral"}>{checkStatus(run.status)}</Badge></li>)}</ul>{data.checks.length === 0 && <p>{t("尚未执行验收；不会自动发送测试请求。")}</p>}</Card>
      <p className="page-description">{t("数据更新时间")} {stamp(data.time)}</p></>}
  </div>;
}
function AuditPanel() {
  const query = useQuery({ queryKey: ["governance", "events"], queryFn: loadGovernanceEvents, refetchInterval: 15000 });
  return <Card className="gov-card"><div className="section-heading"><h2>{t("治理审计")}</h2><Button variant="quiet" disabled={query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button></div><p className="page-description">{t("记录策略变更、令牌撤销、预算拒绝、监控失败 / 恢复和显式文件操作。保留最近 2000 条，显示最近 100 条；不记录密钥或请求正文。")}</p>
    {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
    <ul className="gov-list">{query.data?.map((event, index) => <li key={event.createdAt + ":" + index}><div><strong><code>{event.kind}</code></strong><p className="gov-detail">{event.detail}</p></div><time>{stamp(event.createdAt)}</time></li>)}</ul>
    {query.data?.length === 0 && <p className="page-description">{t("暂无治理审计事件")}</p>}
  </Card>;
}
