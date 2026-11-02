import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { cancelProxyTask, listProxyTasks, loadProxyTask } from "../../lib/governance-service";
import type { ProxyTask, ProxyTaskDetail } from "../../types/governance";
import { Badge, Button, Card, InlineError } from "../../ui";
import { OperationMessage, stamp, usd, useGovernanceOperation } from "./shared";
import { budgetReasonLabel, taskAttemptLabel, taskCostLabel, taskIdentityLabel, taskUsageLabel } from "./task-format";

export function TaskSummaryView({ task }: { task: ProxyTask }) {
  return <dl className="key-values">
    <dt>{t("任务标识")}</dt><dd><code>{task.id}</code></dd>
    <dt>{t("任务身份来源")}</dt><dd>{taskIdentityLabel(task.identitySource)}</dd>
    <dt>{t("任务状态")}</dt><dd>{task.cancelled ? t("已取消") : task.activeAttempts > 0 ? t("执行中") : t("暂无进行中的尝试")}</dd>
    <dt>{t("任务累计尝试 / 上限")}</dt><dd>{task.attempts} / {task.maxAttempts ?? t("未设置")}</dd>
    <dt>{t("任务累计占用 / 上限")}</dt><dd>{usd(task.committedMicrousd)} / {task.maxMicrousd === null ? t("未设置") : usd(task.maxMicrousd)}</dd>
    <dt>{t("累计预留费用")}</dt><dd>{usd(task.reservedMicrousd)}</dd>
    <dt>{t("已知实际费用")}</dt><dd>{usd(task.knownMicrousd)}</dd>
    <dt>{t("费用未结清的尝试")}</dt><dd>{task.unknownCostAttempts}</dd>
    <dt>{t("历史无预留尝试")}</dt><dd>{task.unreservedAttempts}</dd>
    <dt>{t("进行中的尝试")}</dt><dd>{task.activeAttempts}</dd>
  </dl>;
}

export function TaskDetailView({ detail }: { detail: ProxyTaskDetail }) {
  return <section className="gov-task-detail" aria-label={t("任务详情")}>
    <h3>{t("任务详情")}</h3><TaskSummaryView task={detail.task} />
    <p className="page-description">{t("未知费用继续占用预留；已知超额增加累计占用。取消后停止新发送，已发送请求的最终账单仍由供应商结算。")}</p>
    <h3>{t("任务发送记录")}</h3>
    <ol className="gov-task-attempts">{detail.attempts.map(attempt => <li key={attempt.id}>
      <strong>{t("尝试")} {attempt.ordinal} · {taskAttemptLabel(attempt.status)}</strong>
      <dl className="key-values">
        <dt>{t("发送标识")}</dt><dd><code>{attempt.id}</code></dd>
        <dt>{t("客户端请求标识")}</dt><dd><code>{attempt.requestId}</code></dd>
        <dt>{t("供应商")}</dt><dd>{attempt.providerId}</dd><dt>{t("模型")}</dt><dd>{attempt.model}</dd>
        <dt>{t("路由")}</dt><dd>{attempt.route}</dd><dt>{t("HTTP 状态")}</dt><dd>{attempt.statusCode ?? t("未取得 HTTP")}</dd>
        <dt>{t("预留费用")}</dt><dd>{attempt.reservedMicrousd === null ? t("未预留") : usd(attempt.reservedMicrousd)}</dd>
        <dt>{t("实际费用")}</dt><dd>{attempt.actualMicrousd === null ? t("未知") : usd(attempt.actualMicrousd)}</dd>
        <dt>{t("用量状态")}</dt><dd>{taskUsageLabel(attempt.usageState)} · {attempt.inputTokens ?? "—"} / {attempt.outputTokens ?? "—"}</dd>
        <dt>{t("费用状态")}</dt><dd>{taskCostLabel(attempt.costState)}</dd>
        <dt>{t("用量记录标识")}</dt><dd>{attempt.eventId ?? "—"}</dd>
        <dt>{t("开始时间")}</dt><dd>{stamp(attempt.startedAt / 1000)}</dd>
        <dt>{t("结束时间")}</dt><dd>{attempt.finishedAt === null ? "—" : stamp(attempt.finishedAt / 1000)}</dd>
        {attempt.errorKind && <><dt>{t("失败原因")}</dt><dd>{budgetReasonLabel(attempt.errorKind)}</dd></>}
      </dl>
    </li>)}</ol>
    {detail.attempts.length === 0 && <p className="page-description">{t("此任务尚未发送上游请求。")}</p>}
  </section>;
}

export function TaskPanel() {
  const query = useQuery({ queryKey: ["governance", "tasks"], queryFn: listProxyTasks, refetchInterval: 5000 });
  const [selected, setSelected] = useState<string | null>(null), [confirmed, setConfirmed] = useState(false);
  const detail = useQuery({ queryKey: ["governance", "task", selected], queryFn: () => loadProxyTask(selected!), enabled: selected !== null, refetchInterval: 5000 });
  const op = useGovernanceOperation();
  useEffect(() => { setConfirmed(false); }, [selected]);
  return <Card className="gov-card"><OperationMessage op={op} />
    <div className="section-heading"><h2>{t("最近任务")}</h2><Button variant="quiet" disabled={query.isFetching} onClick={() => void op.refresh()}>{t("刷新")}</Button></div>
    <p className="page-description">{t("显示最近 50 个任务。选择任务可查看完整发送记录，并取消同一任务的全部并发请求和后续重试。")}</p>
    {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
    <ul className="gov-list gov-task-list">{query.data?.map(task => <li key={task.id}>
      <div><strong><code>{task.id.slice(0, 16)}</code></strong><p>{taskIdentityLabel(task.identitySource)} · {t("尝试")} {task.attempts} / {task.maxAttempts ?? t("未设置")} · {usd(task.committedMicrousd)}</p><time>{stamp(task.updatedAt)}</time></div>
      <Badge tone={task.cancelled ? "neutral" : task.activeAttempts > 0 ? "warning" : "neutral"}>{task.cancelled ? t("已取消") : task.activeAttempts > 0 ? t("执行中") : t("暂无进行中的尝试")}</Badge>
      <Button variant="quiet" aria-pressed={selected === task.id} onClick={() => setSelected(task.id)}>{t("查看任务")}</Button>
    </li>)}</ul>
    {query.isPending && <p>{t("读取中")}</p>}{query.data?.length === 0 && <p>{t("暂无任务记录")}</p>}
    {detail.error && <InlineError message={String(detail.error)} onRetry={() => void detail.refetch()} />}
    {selected && detail.isPending && <p>{t("读取中")}</p>}
    {detail.data && <><TaskDetailView detail={detail.data} /><fieldset className="gov-fieldset" disabled={op.busy || detail.data.task.cancelled}>
      <label className="gov-check"><input type="checkbox" checked={confirmed} onChange={event => setConfirmed(event.target.checked)} />{t("确认取消此任务的全部请求与后续重试")}</label>
      <div className="gov-actions"><Button variant="danger" disabled={!confirmed} onClick={() => void op.run(async () => { await cancelProxyTask(detail.data!.task.id, confirmed); setConfirmed(false); await op.refresh(); }, t("任务已取消，后续重试已阻断。"))}>{t("取消任务")}</Button></div>
    </fieldset></>}
  </Card>;
}
