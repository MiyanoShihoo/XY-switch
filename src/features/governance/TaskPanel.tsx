import { useEffect, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTaskDraft } from "../settings/TaskNavigation";
import { t } from "../../i18n/language";
import { cancelProxyTask, listProxyTasks, loadProxyTask } from "../../lib/governance-service";
import type { ProxyTask, ProxyTaskDetail } from "../../types/governance";
import { Badge, Button, Drawer, InlineError } from "../../ui";
import { OperationMessage, stamp, usd, useGovernanceOperation } from "./shared";
import { budgetReasonLabel, taskAttemptLabel, taskCostLabel, taskIdentityLabel, taskUsageLabel } from "./task-format";

function TaskFact({ label, children, token = false }: { label: string; children: ReactNode; token?: boolean }) {
  return <div className={token ? "gov-fact gov-fact--token" : "gov-fact"}><dt>{label}</dt><dd>{children}</dd></div>;
}

export function TaskSummaryView({ task }: { task: ProxyTask }) {
  return <div className="gov-task-summary">
    <dl className="gov-facts">
      <TaskFact label={t("任务标识")} token><code className="gov-id" tabIndex={0}>{task.id}</code></TaskFact>
      <TaskFact label={t("任务身份来源")}>{taskIdentityLabel(task.identitySource)}</TaskFact>
      <TaskFact label={t("任务状态")}><Badge tone={task.cancelled ? "neutral" : task.activeAttempts > 0 ? "warning" : "neutral"}>{task.cancelled ? t("已取消") : task.activeAttempts > 0 ? t("执行中") : t("暂无进行中的尝试")}</Badge></TaskFact>
      <TaskFact label={t("任务累计尝试 / 上限")}><span className="gov-number">{task.attempts} / {task.maxAttempts ?? t("未设置")}</span></TaskFact>
      <TaskFact label={t("任务累计占用 / 上限")}><span className="gov-cost-pair"><span className="gov-number">{usd(task.committedMicrousd)}</span><span aria-hidden="true">/</span><span className="gov-number">{task.maxMicrousd === null ? t("未设置") : usd(task.maxMicrousd)}</span></span></TaskFact>
    </dl>
    <details className="workspace-disclosure"><summary>{t("用量与预留")}</summary><div className="workspace-disclosure__body">
      <dl className="gov-facts">
        <TaskFact label={t("累计预留费用")}><span className="gov-number">{usd(task.reservedMicrousd)}</span></TaskFact>
        <TaskFact label={t("已知实际费用")}><span className="gov-number">{usd(task.knownMicrousd)}</span></TaskFact>
        <TaskFact label={t("费用未结清的尝试")}>{task.unknownCostAttempts}</TaskFact>
        <TaskFact label={t("历史无预留尝试")}>{task.unreservedAttempts}</TaskFact>
        <TaskFact label={t("进行中的尝试")}>{task.activeAttempts}</TaskFact>
      </dl>
    </div></details>
  </div>;
}

export function TaskDetailView({ detail }: { detail: ProxyTaskDetail }) {
  return <section className="gov-task-detail" aria-label={t("任务详情")}>
    <TaskSummaryView task={detail.task} />
    {detail.task.cancelled && <p className="page-description" role="status">{t("任务已取消，后续重试已阻断。")}</p>}
    <p className="gov-warning">{t("未知费用继续占用预留；已知超额增加累计占用。取消后停止新发送，已发送请求的最终账单仍由供应商结算。")}</p>
    <div className="section-heading"><h3>{t("任务发送记录")}</h3><Badge>{detail.attempts.length}</Badge></div>
    <ol className="gov-task-attempts">{detail.attempts.map(attempt => <li key={attempt.id}>
      <details className="workspace-disclosure"><summary>{t("尝试")} {attempt.ordinal} · {taskAttemptLabel(attempt.status)}</summary><div className="workspace-disclosure__body">
        <dl className="gov-facts">
          <TaskFact label={t("发送标识")} token><code>{attempt.id}</code></TaskFact>
          <TaskFact label={t("客户端请求标识")} token><code>{attempt.requestId}</code></TaskFact>
          <TaskFact label={t("供应商")} token><code>{attempt.providerId}</code></TaskFact>
          <TaskFact label={t("模型")} token><code>{attempt.model}</code></TaskFact>
          <TaskFact label={t("路由")} token><code>{attempt.route}</code></TaskFact>
          <TaskFact label={t("HTTP 状态")}>{attempt.statusCode ?? t("未取得 HTTP")}</TaskFact>
          <TaskFact label={t("预留费用")}><span className="gov-number">{attempt.reservedMicrousd === null ? t("未预留") : usd(attempt.reservedMicrousd)}</span></TaskFact>
          <TaskFact label={t("实际费用")}><span className="gov-number">{attempt.actualMicrousd === null ? t("未知") : usd(attempt.actualMicrousd)}</span></TaskFact>
          <TaskFact label={t("用量状态")}>{taskUsageLabel(attempt.usageState)} · <span className="gov-number">{attempt.inputTokens ?? "—"} / {attempt.outputTokens ?? "—"}</span></TaskFact>
          <TaskFact label={t("费用状态")}>{taskCostLabel(attempt.costState)}</TaskFact>
          <TaskFact label={t("用量记录标识")}>{attempt.eventId ?? "—"}</TaskFact>
          <TaskFact label={t("开始时间")}>{stamp(attempt.startedAt / 1000)}</TaskFact>
          <TaskFact label={t("结束时间")}>{attempt.finishedAt === null ? "—" : stamp(attempt.finishedAt / 1000)}</TaskFact>
          {attempt.errorKind && <TaskFact label={t("失败原因")}>{budgetReasonLabel(attempt.errorKind)}</TaskFact>}
        </dl>
      </div></details>
    </li>)}</ol>
    {detail.attempts.length === 0 && <p className="page-description">{t("此任务尚未发送上游请求。")}</p>}
  </section>;
}

export function taskNeedsReview(task: ProxyTask) {
  return task.activeAttempts > 0 || task.unknownCostAttempts > 0 || task.unreservedAttempts > 0;
}

export function TaskPanel({ initialTaskId }: { initialTaskId?: string } = {}) {
  const query = useQuery({ queryKey: ["governance", "tasks"], queryFn: listProxyTasks, refetchInterval: 5000 });
  const [selected, setSelected] = useState<string | null>(initialTaskId ?? null), [confirmed, setConfirmed] = useState(false);
  const detail = useQuery({ queryKey: ["governance", "task", selected], queryFn: () => loadProxyTask(selected!), enabled: selected !== null, refetchInterval: 5000 });
  const op = useGovernanceOperation();
  const [filter, setFilter] = useState<"attention" | "all">("attention");
  const visibleTasks = query.data?.filter(task => filter === "all" || taskNeedsReview(task));
  useTaskDraft(confirmed, op.busy);
  useEffect(() => { setConfirmed(false); }, [selected]);
  return <section className="gov-card">{!selected && <OperationMessage op={op} />}
    <div className="section-heading"><h2>{t("最近任务")}</h2><Button variant="quiet" disabled={query.isFetching} onClick={() => void op.refresh()}>{t("刷新")}</Button></div>
    <p className="page-description">{t("显示最近 50 个任务。选择任务可查看完整发送记录，并取消同一任务的全部并发请求和后续重试。")}</p>
    {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
    <div className="gov-actions" role="group" aria-label={t("任务范围")}><Button variant="quiet" aria-pressed={filter === "attention"} onClick={() => setFilter("attention")}>{t("活动与需核对")}</Button><Button variant="quiet" aria-pressed={filter === "all"} onClick={() => setFilter("all")}>{t("全部最近任务")}</Button></div>
    <p className="page-description">{t("需核对包括未知费用和未预留的尝试；未知费用不按零计。")}</p>
    <div className="gov-table-scroll" tabIndex={0} role="region" aria-label={t("最近任务")}><table className="gov-task-table">
      <thead><tr><th scope="col">{t("任务标识")}</th><th scope="col">{t("任务累计尝试 / 上限")}</th><th scope="col">{t("任务累计占用 / 上限")}</th><th scope="col">{t("数据更新时间")}</th><th scope="col">{t("任务状态")}</th><th scope="col">{t("任务详情")}</th></tr></thead>
      <tbody>{visibleTasks?.map(task => <tr key={task.id}>
        <th scope="row"><code title={task.id}>{task.id}</code><small>{taskIdentityLabel(task.identitySource)}</small></th>
        <td><span className="gov-number">{task.attempts} / {task.maxAttempts ?? t("未设置")}</span></td>
        <td><span className="gov-number">{usd(task.committedMicrousd)}</span><span className="gov-task-table__limit">/ {task.maxMicrousd === null ? t("未设置") : usd(task.maxMicrousd)}</span></td>
        <td><time>{stamp(task.updatedAt)}</time></td>
        <td><Badge tone={task.cancelled ? "neutral" : task.activeAttempts > 0 ? "warning" : "neutral"}>{task.cancelled ? t("已取消") : task.activeAttempts > 0 ? t("执行中") : t("暂无进行中的尝试")}</Badge></td>
        <td><Button variant="quiet" aria-label={t("查看任务") + " · " + task.id} aria-pressed={selected === task.id} onClick={() => setSelected(task.id)}>{t("查看任务")}</Button></td>
      </tr>)}</tbody>
    </table></div>
    {query.isPending && <p>{t("读取中")}</p>}{query.data?.length === 0 && <p>{t("暂无任务记录")}</p>}{query.data && query.data.length > 0 && visibleTasks?.length === 0 && <p role="status">{t("最近记录中没有活动或需核对的任务。可切换到全部最近任务。")}</p>}
    {selected && <Drawer open title={t("任务详情")} busy={op.busy} onClose={() => { setSelected(null); setConfirmed(false); }} footer={detail.data && <fieldset className="gov-fieldset gov-task-cancel" disabled={op.busy || detail.data.task.cancelled}>
      <label className="gov-check"><input type="checkbox" checked={confirmed} onChange={event => setConfirmed(event.target.checked)} /><span>{t("确认取消此任务的全部请求与后续重试")}</span></label>
      <div className="gov-actions"><Button variant="danger" disabled={!confirmed} onClick={() => void op.run(async () => { await cancelProxyTask(detail.data!.task.id, confirmed); setConfirmed(false); await op.refresh(); }, t("任务已取消，后续重试已阻断。"))}>{t("取消任务")}</Button></div>
    </fieldset>}><div className="gov-stack"><OperationMessage op={op} />
      {detail.error && <InlineError message={String(detail.error)} onRetry={() => void detail.refetch()} />}
      {detail.isPending && <p>{t("读取中")}</p>}
      {detail.isSuccess && !detail.data && <p role="status">{t("此任务已不可用，请刷新任务列表。")}</p>}
      {detail.data && <TaskDetailView detail={detail.data} />}
    </div></Drawer>}
  </section>;
}
