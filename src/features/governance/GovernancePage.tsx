import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ArrowRight, ShieldCheck } from "lucide-react";
import { useLanguage, t } from "../../i18n/language";
import { governanceNative, listProxyTasks, loadAccessPolicy, loadGovernanceEvents, loadGovernanceOverview, loadMonitorPolicy } from "../../lib/governance-service";
import { usePlatformCapabilities } from "../../lib/platform-service";
import type { Provider } from "../../types/provider";
import type { ProxyStatus } from "../../types/proxy";
import { Badge, Button, Field, InlineError, Input, Select } from "../../ui";
import { TaskNavigation, openRoutingSettings } from "../settings/TaskNavigation";
import { AccessPanel } from "./AccessPanel";
import { BudgetPanel } from "./BudgetPanel";
import { MonitorPanel } from "./MonitorPanel";
import { VersionsPanel } from "./VersionsPanel";
import { MountsPanel } from "./MountsPanel";
import { TaskPanel, taskNeedsReview } from "./TaskPanel";
import { checkStatus, stamp, usd } from "./shared";
import "./governance.css";

type GovernanceTask = "access" | "tokens" | "budget" | "tasks" | "monitor" | "versions" | "mounts" | "audit";
type TaskEntry = { value: GovernanceTask; label: string; description: string };
type NavigateTask = (task: GovernanceTask, targetId?: string) => void;

export function GovernancePage({ providers, proxyStatus, onRestored }: { providers: Provider[]; proxyStatus: ProxyStatus | null; onRestored: () => Promise<void> }) {
  useLanguage();
  const [selection, setSelection] = useState<{ task: GovernanceTask; targetId?: string } | null>(null);
  const platform = usePlatformCapabilities(), readOnly = !platform.data || platform.data.mobile;
  const heading = useRef<HTMLHeadingElement>(null), returnContext = useRef({ id: "", scroll: 0 });
  const entries: TaskEntry[] = [
    ...(!readOnly ? [
      { value: "access" as const, label: t("访问策略"), description: t("检查前置条件，修改认证、速率与并发") },
      { value: "tokens" as const, label: t("本地访问令牌"), description: t("按用途与期限检查、创建或撤销令牌") },
      { value: "budget" as const, label: t("硬预算"), description: t("查看当前账本，再进入策略修改") },
      { value: "tasks" as const, label: t("最近任务"), description: t("核对活动与未知费用，查看尝试或取消任务") },
    ] : []),
    { value: "monitor", label: t("验收监控"), description: readOnly ? t("查看已有验收收据") : t("检查前提、选择目标、授权收费并核对结果") },
    ...(!readOnly ? [
      { value: "versions" as const, label: t("配置版本"), description: t("选择恢复版本，比较后确认回滚") },
      { value: "mounts" as const, label: t("显式挂载"), description: t("授权目录、指定文件、选择来源并审核差异") },
    ] : []),
    { value: "audit", label: t("治理审计"), description: t("按类型和文字筛选已加载事件") },
  ];
  const active = entries.find(entry => entry.value === selection?.task);
  const openTask: NavigateTask = (task, targetId) => {
    if (!active) returnContext.current = { id: document.activeElement?.id ?? "", scroll: document.querySelector<HTMLElement>(".xy-shell-main")?.scrollTop ?? 0 };
    setSelection({ task, targetId });
  };
  useEffect(() => {
    const scroller = document.querySelector<HTMLElement>(".xy-shell-main");
    if (active) { if (scroller) scroller.scrollTop = 0; heading.current?.focus({ preventScroll: true }); }
    else { if (scroller) scroller.scrollTop = returnContext.current.scroll; (document.getElementById(returnContext.current.id) ?? heading.current)?.focus({ preventScroll: true }); }
  }, [active?.value]);
  return <TaskNavigation>{({ leave, busy }) => <div className="workspace-page governance-page" aria-labelledby="governance-title">
    <header className="page-heading"><div className="page-heading__copy">
      {active && <Button variant="quiet" icon={<ArrowLeft size={16} />} disabled={busy} onClick={() => leave(() => setSelection(null))}>{t("返回安全与验收")}</Button>}
      <h1 id="governance-title" ref={heading} tabIndex={-1}>{!active && <ShieldCheck size={23} aria-hidden="true" />}{active?.label ?? t("安全与验收")}</h1>
      <p className="page-description">{active?.description ?? t("从当前状态和待处理记录进入具体任务；选择入口不会执行写入或收费验收。")}</p>
    </div></header>
    {!governanceNative() && <p className="gov-warning">{t("浏览器中仅展示预览；没有真实监控数据，也不会保存设置或发送收费请求。")}</p>}
    {platform.data?.mobile && <p className="gov-warning">{t("移动端为只读视图，仅显示本机数据；尚未连接其他设备。不会接管桌面客户端，也不保证后台代理常驻。")}</p>}
    {platform.error && <InlineError message={String(platform.error)} onRetry={() => void platform.refetch()} />}
    {active && <Field className="gov-task-switcher" label={t("治理任务")}><Select value={active.value} disabled={busy} onChange={event => { const task = event.target.value as GovernanceTask; leave(() => openTask(task)); }}>{entries.map(entry => <option key={entry.value} value={entry.value}>{entry.label}</option>)}</Select></Field>}
    <div className="gov-panel" key={active?.value ?? "overview"} role="region" aria-label={active?.label ?? t("当前状态与待处理")}>
      {!active && <Overview providers={providers} proxyStatus={proxyStatus} entries={entries} onNavigate={openTask} readOnly={readOnly} />}
      {active?.value === "access" && <AccessPanel view="policy" proxyStopped={proxyStatus?.running === false} />}
      {active?.value === "tokens" && <AccessPanel view="tokens" proxyStopped={proxyStatus?.running === false} />}
      {active?.value === "budget" && <BudgetPanel />}
      {active?.value === "tasks" && <TaskPanel initialTaskId={selection?.targetId} />}
      {active?.value === "monitor" && <MonitorPanel providers={providers} proxyRunning={proxyStatus?.running === true} readOnly={readOnly} initialRunId={selection?.targetId} onBudget={readOnly ? undefined : () => openTask("budget")} />}
      {active?.value === "versions" && <VersionsPanel proxyStopped={proxyStatus?.running === false} onRestored={onRestored} />}
      {active?.value === "mounts" && <MountsPanel />}
      {active?.value === "audit" && <AuditPanel />}
    </div>
  </div>}</TaskNavigation>;
}

function Overview({ providers, proxyStatus, entries, onNavigate, readOnly }: { providers: Provider[]; proxyStatus: ProxyStatus | null; entries: TaskEntry[]; onNavigate: NavigateTask; readOnly: boolean }) {
  const query = useQuery({ queryKey: ["governance", "overview"], queryFn: loadGovernanceOverview, refetchInterval: 15000 });
  const access = useQuery({ queryKey: ["governance", "access"], queryFn: loadAccessPolicy });
  const monitor = useQuery({ queryKey: ["governance", "monitor"], queryFn: loadMonitorPolicy });
  const tasks = useQuery({ queryKey: ["governance", "tasks"], queryFn: listProxyTasks, refetchInterval: 5000, enabled: !readOnly });
  const data = query.data, attention = tasks.data?.filter(taskNeedsReview) ?? [], failedChecks = data?.checks.filter(run => run.status === "failed") ?? [];
  const refresh = () => { void query.refetch(); void access.refetch(); void monitor.refetch(); if (!readOnly) void tasks.refetch(); };
  return <div className="gov-stack">
    <div className="gov-home-layout"><div className="gov-stack">
      <section className="gov-card"><div className="section-heading"><h2>{t("运行与保护状态")}</h2><Button variant="quiet" disabled={query.isFetching || access.isFetching || monitor.isFetching} onClick={refresh}>{t("刷新")}</Button></div>
        <dl className="gov-state-actions">
          <div><dt>{t("代理状态")}</dt><dd>{proxyStatus ? proxyStatus.running ? t("运行中") : t("已停止") : t("状态未知")}</dd>{!readOnly && <Button variant="quiet" onClick={openRoutingSettings}>{t("检查运行与接管")}</Button>}</div>
          <div><dt>{t("访问认证")}</dt><dd>{access.data ? access.data.requireToken ? t("要求令牌") : t("兼容模式") : t("状态未知")}</dd>{!readOnly && <Button id="gov-status-access" variant="quiet" onClick={() => onNavigate("access")}>{t("检查访问策略")}</Button>}</div>
          <div><dt>{t("硬预算")}</dt><dd>{data ? <>{data.budget.policy.enabled ? t("已启用") : t("已停用")}<small>{t("今日费用预留")} {usd(data.budget.reservedMicrousd)}</small></> : t("状态未知")}</dd>{!readOnly && <Button id="gov-status-budget" variant="quiet" onClick={() => onNavigate("budget")}>{t("查看当前账本")}</Button>}</div>
          <div><dt>{t("合成监控")}</dt><dd>{monitor.data ? monitor.data.enabled ? t("已启用") : t("已停用") : t("状态未知")}</dd><Button id="gov-status-monitor" variant="quiet" onClick={() => onNavigate("monitor")}>{t("检查验收与计划")}</Button></div>
        </dl>
        {access.data && !access.data.requireToken && <p className="gov-warning">{t("兼容模式：本地模型接口不要求认证。只读状态接口始终要求令牌。")}</p>}
        <p className="page-description">{t("只读接口 GET /xy/status 使用 monitor 令牌，不返回供应商端点、密钥、响应正文或上游原始错误。代理仍仅监听回环地址。")}</p>
        {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}{access.error && <InlineError message={String(access.error)} onRetry={() => void access.refetch()} />}{monitor.error && <InlineError message={String(monitor.error)} onRetry={() => void monitor.refetch()} />}
      </section>
      <section className="gov-card"><h2>{t("活动与待核对记录")}</h2><p className="page-description">{t("来自最近任务与验收记录；未知费用不是零，历史失败也不代表当前故障。")}</p>
        {tasks.error && <InlineError message={String(tasks.error)} onRetry={() => void tasks.refetch()} />}
        <ul className="gov-handoff-list">
          {attention.map(task => <li key={task.id}><div><code className="gov-id">{task.id}</code><p>{task.activeAttempts > 0 ? t("执行中") : task.cancelled ? t("已取消") : t("暂无进行中的尝试")} · {t("未知费用尝试")} {task.unknownCostAttempts} · {t("未预留尝试")} {task.unreservedAttempts}</p></div><Button id={"gov-open-task-" + task.id} variant="secondary" onClick={() => onNavigate("tasks", task.id)}>{t("查看任务")}</Button></li>)}
          {failedChecks.map(run => <li key={run.id}><div><strong>{providers.find(provider => provider.id === run.providerId)?.name ?? run.providerId}</strong><p>{checkStatus(run.status)} · {run.mode} · {stamp(run.createdAt)}</p></div><Button id={"gov-open-run-" + run.id} variant="secondary" onClick={() => onNavigate("monitor", run.id)}>{t("核对失败收据")}</Button></li>)}
        </ul>
        {data && (readOnly || tasks.data) && attention.length === 0 && failedChecks.length === 0 && <p>{t("已读取的记录中没有待核对项。")}</p>}
        {query.isPending && <p>{t("读取中")}</p>}
      </section>
    </div><section className="gov-task-index"><h2>{t("治理任务")}</h2><nav aria-label={t("治理任务")}>{entries.map(entry => <button type="button" key={entry.value} id={"gov-task-" + entry.value} onClick={() => onNavigate(entry.value)}><span><strong>{entry.label}</strong><small>{entry.description}</small></span><ArrowRight size={16} aria-hidden="true" /></button>)}</nav></section></div>
    {data && <>
      <dl className="gov-request-total"><dt>{t("累计代理请求")}</dt><dd>{data.requests.totalRequests.toLocaleString()}</dd></dl>
      <details className="workspace-disclosure"><summary>{t("供应商状态")}</summary><div className="workspace-disclosure__body"><ul className="gov-list">{data.providers.map(provider => <li key={provider.providerId}><div><strong>{providers.find(item => item.id === provider.providerId)?.name ?? provider.providerId}</strong><p>{provider.latencyMs ?? "—"} ms · {t("成功率")} {provider.successRate.toFixed(1)}% · {stamp(provider.lastCheckedAt / 1000)}</p></div><Badge tone={provider.status === "healthy" ? "success" : provider.status === "unknown" ? "neutral" : "danger"}>{provider.status === "healthy" ? t("健康") : provider.status === "unhealthy" ? t("异常") : provider.status === "circuit_open" ? t("熔断中") : t("状态未知")}</Badge></li>)}</ul>{data.providers.length === 0 && <p>{t("暂无供应商状态数据")}</p>}</div></details>
      <details className="workspace-disclosure"><summary>{t("最近验收状态")}</summary><div className="workspace-disclosure__body"><ul className="gov-list">{data.checks.map(run => <li key={run.id}><div><strong>{providers.find(provider => provider.id === run.providerId)?.name ?? run.providerId}</strong><p>{run.mode} · {stamp(run.createdAt)} · {run.latencyMs} ms · {checkStatus(run.status)}</p></div><Button id={"gov-receipt-" + run.id} variant="quiet" onClick={() => onNavigate("monitor", run.id)}>{t("查看验收证据")}</Button></li>)}</ul>{data.checks.length === 0 && <p>{t("尚未执行验收；不会自动发送测试请求。")}</p>}</div></details>
      <p className="page-description">{t("数据更新时间")} {stamp(data.time)}</p>
    </>}
  </div>;
}

function AuditPanel() {
  const query = useQuery({ queryKey: ["governance", "events"], queryFn: loadGovernanceEvents, refetchInterval: 15000 });
  const [kind, setKind] = useState(""), [search, setSearch] = useState("");
  const kinds = [...new Set(query.data?.map(event => event.kind) ?? [])];
  const term = search.trim().toLocaleLowerCase();
  const events = query.data?.filter(event => (!kind || event.kind === kind) && (!term || (event.kind + " " + event.detail).toLocaleLowerCase().includes(term)));
  return <section className="gov-card"><div className="section-heading"><h2>{t("治理审计")}</h2><Button variant="quiet" disabled={query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button></div><p className="page-description">{t("记录策略变更、令牌撤销、预算拒绝、监控失败 / 恢复和显式文件操作。保留最近 2000 条，显示最近 100 条；不记录密钥或请求正文。")}</p>
    <div className="gov-grid"><Field label={t("已加载事件类型")}><Select value={kind} onChange={event => setKind(event.target.value)}><option value="">{t("全部类型")}</option>{kinds.map(value => <option key={value} value={value}>{value}</option>)}</Select></Field><Field label={t("筛选已加载记录")}><Input type="search" value={search} onChange={event => setSearch(event.target.value)} /></Field></div>
    <p className="page-description" role="status">{t("显示 {visible} / {loaded} 条已加载事件；此筛选不搜索未加载的日志。", { visible: events?.length ?? 0, loaded: query.data?.length ?? 0 })}</p>
    {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
    <ol className="gov-event-log">{events?.map((event, index) => <li key={event.createdAt + ":" + index}><time>{stamp(event.createdAt)}</time><div><strong><code>{event.kind}</code></strong><p className="gov-detail">{event.detail}</p></div></li>)}</ol>
    {query.data?.length === 0 && <p className="page-description">{t("暂无治理审计事件")}</p>}{query.data && query.data.length > 0 && events?.length === 0 && <p>{t("已加载记录中没有匹配项")}</p>}
  </section>;
}
