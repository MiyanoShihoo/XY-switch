import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { listSyntheticRuns, loadHardBudget, loadMonitorPolicy, runSyntheticCheck, saveMonitorPolicy } from "../../lib/governance-service";
import { validInteger } from "../../lib/governance-diff";
import type { CheckMode, MonitorPolicy, SyntheticRun } from "../../types/governance";
import type { Provider } from "../../types/provider";
import { Badge, Button, Field, InlineError, Input, Select, Tabs, Toggle } from "../../ui";
import { openRoutingSettings, useTaskDraft, useTaskNavigation } from "../settings/TaskNavigation";
import { checkStatus, Confirmation, OperationMessage, stamp, useGovernanceOperation } from "./shared";

export function MonitorPanel({ providers, proxyRunning, readOnly, initialRunId, onBudget }: { providers: Provider[]; proxyRunning: boolean; readOnly: boolean; initialRunId?: string; onBudget?: () => void }) {
  const [view, setView] = useState(initialRunId ? "receipts" : "configuration");
  const [lastRun, setLastRun] = useState<SyntheticRun | null>(null);
  const { leave } = useTaskNavigation();
  const activeView = readOnly ? "receipts" : view;
  const policy = useQuery({ queryKey: ["governance", "monitor"], queryFn: loadMonitorPolicy });
  const budget = useQuery({ queryKey: ["governance", "budget"], queryFn: loadHardBudget });
  const runs = useQuery({ queryKey: ["governance", "runs"], queryFn: listSyntheticRuns, refetchInterval: 15000 });
  const [draft, setDraft] = useState<MonitorPolicy | null>(null), [confirmation, setConfirmation] = useState("");
  const op = useGovernanceOperation();
  useEffect(() => { if (!draft && policy.data) setDraft({ ...policy.data }); }, [policy.data, draft]);
  const change = (v: Partial<MonitorPolicy>) => { setDraft(d => d ? { ...d, ...v } : d); setConfirmation(""); };
  useTaskDraft(Boolean(draft && policy.data && JSON.stringify(draft) !== JSON.stringify(policy.data)) || Boolean(confirmation), op.busy);
  const valid = draft && validInteger(draft.intervalMinutes, 5, 1440) && validInteger(draft.maxRunsPerDay, 1, 100);
  const targetValid = draft && draft.model.trim() && providers.some(p => p.enabled && p.id === draft.providerId) && (draft.mode !== "gemini" || /^[a-zA-Z0-9._-]+$/.test(draft.model));
  const consent = confirmation === "ALLOW PAID CHECKS", budgetEnabled = budget.data?.policy.enabled === true;
  const modes: { value: CheckMode; label: string }[] = [{ value: "chat", label: "Chat JSON" }, { value: "stream", label: "Chat SSE" }, { value: "tools", label: t("函数工具调用") }, { value: "responses", label: "Responses" }, { value: "anthropic", label: "Anthropic" }, { value: "gemini", label: "Gemini" }];
  return <div className="gov-stack">
    {!readOnly && <Tabs id="monitor" label={t("验收工作区")} value={activeView} onChange={next => leave(() => { if (policy.data) setDraft({ ...policy.data }); setConfirmation(""); setView(next); })} items={[{ value: "receipts", label: t("验收收据"), disabled: op.busy }, { value: "configuration", label: t("验收配置"), disabled: op.busy }]} />}
    <OperationMessage op={op} />
    {!readOnly && activeView === "configuration" && <div role="tabpanel" id="monitor-panel-configuration" aria-labelledby="monitor-tab-configuration"><section className="gov-card"><h2>{t("真实端到端验收与合成监控")}</h2>
      <p className="page-description">{t("通过真实回环 socket → 本地代理 → 指定供应商；固定模型、不重试、不故障转移、不触发额外模型复核。工具只验证返回结构，不执行。")}</p>
      <p className="gov-warning">{t("每次验收会产生真实上游请求。文本最多 16 输出 Token，工具最多 48；超时 60 秒，响应上限 1 MiB。必须先开启预算闸并配置精确价格。")}</p>
      {policy.error && <InlineError message={String(policy.error)} onRetry={() => void policy.refetch()} />}
      {budget.error && <InlineError message={String(budget.error)} onRetry={() => void budget.refetch()} />}
      <section className="gov-prerequisites"><h3>{t("执行前提")}</h3><dl className="gov-prerequisite-list"><div><dt>{t("代理状态")}</dt><dd>{proxyRunning ? t("运行中") : t("尚未确认运行")}</dd></div><div><dt>{t("硬预算")}</dt><dd>{budget.data ? budgetEnabled ? t("已启用") : t("已停用") : t("状态未知")}</dd></div><div><dt>{t("精确价格")}</dt><dd>{t("实际派发模型须已有精确自定义价格；发送前再次校验。")}</dd></div></dl><div className="gov-actions"><Button variant="quiet" disabled={op.busy} onClick={() => leave(openRoutingSettings)}>{t("检查本地路由")}</Button>{onBudget && <Button variant="quiet" disabled={op.busy} onClick={() => leave(onBudget)}>{t("检查预算策略")}</Button>}</div></section>
      {draft ? <fieldset className="gov-fieldset gov-monitor-editor" disabled={op.busy}><div className="gov-monitor-editor__fields">
        <section className="gov-group"><h3>{t("验收目标")}</h3><div className="gov-form-rows"><Field label={t("验收供应商")}><Select value={draft.providerId} onChange={e => { const provider = providers.find(p => p.id === e.target.value); change({ providerId: e.target.value, model: provider?.model ?? "" }); }}><option value="">{t("选择供应商")}</option>{providers.filter(p => p.enabled).map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></Field><Field label={t("明确的上游模型 ID")}><Input value={draft.model} maxLength={256} autoComplete="off" spellCheck={false} onChange={e => change({ model: e.target.value })} /></Field><Field label={t("验收模式")}><Select value={draft.mode} onChange={e => change({ mode: e.target.value as CheckMode })}>{modes.map(mode => <option key={mode.value} value={mode.value}>{mode.label}</option>)}</Select></Field></div>
        </section>
        </div><div className="gov-approval gov-monitor-editor__approval"><h3>{t("收费授权")}</h3>
        {(!proxyRunning || !budgetEnabled) && <p className="gov-warning">{t("单次验收需要正在运行的代理和已启用的预算闸。")}</p>}
        <Confirmation word="ALLOW PAID CHECKS" value={confirmation} onChange={setConfirmation} />
        <div className="gov-actions"><Button variant="secondary" disabled={!valid || !targetValid || !consent || !proxyRunning || !budgetEnabled} onClick={() => void op.run(async () => { const result = await runSyntheticCheck(draft, confirmation); setConfirmation(""); setLastRun(result); await op.refresh(); if (result.status === "failed") op.setError(checkStatus(result.status) + " · " + result.detail); else op.setMessage(checkStatus(result.status) + " · " + result.detail); })}>{t("执行一次收费验收")}</Button><Button variant="quiet" onClick={() => { if (policy.data) setDraft({ ...policy.data }); setConfirmation(""); }}>{t("放弃草稿")}</Button></div>
        </div>
        {lastRun && <section className="gov-run-result"><h3>{t("本次验收结果")}</h3><ReceiptEvidence run={lastRun} providers={providers} expanded /></section>}
        <details className="workspace-disclosure"><summary>{t("可选：保存验收计划")} · {policy.data?.enabled ? t("已启用") : t("已停用")}</summary><div className="workspace-disclosure__body gov-stack">
        <section className="gov-group"><h3>{t("计划频率与限额")}</h3>
        <label className="gov-toggle"><span>{t("应用运行期间定时验收")}</span><Toggle aria-label={t("应用运行期间定时验收")} checked={draft.enabled} onCheckedChange={enabled => change({ enabled })} /></label>
        <div className="gov-form-rows"><Field label={t("间隔（分钟）")}><Input type="number" min={5} max={1440} value={draft.intervalMinutes} onChange={e => change({ intervalMinutes: Number(e.target.value) })} /></Field><Field label={t("每日最多验收次数（UTC）")}><Input type="number" min={1} max={100} value={draft.maxRunsPerDay} onChange={e => change({ maxRunsPerDay: Number(e.target.value) })} /></Field></div>
        <p className="page-description">{t("手动与计划共享每日次数。单次使用草稿与已保存上限中的较小值；修改上限须先保存。重启按已记录时间等待，不补跑历史任务。")}</p>
        <p className="page-description">{t("仅在失败或恢复时写入告警，不重复提醒相同状态。应用关闭或系统挂起期间不保证执行。")}</p>
        </section>
        <div className="gov-actions"><Button disabled={!valid || (!!draft.enabled && (!targetValid || !consent || !budgetEnabled))} onClick={() => void op.run(async () => { await saveMonitorPolicy(draft, confirmation); setConfirmation(""); await op.refresh(); }, t("验收计划已保存"))}>{draft.enabled ? t("保存并启用计划") : t("保存停用的计划")}</Button></div></div></details>
      </fieldset> : <p>{t("读取中")}</p>}
    </section></div>}
    {activeView === "receipts" && <div role={readOnly ? "region" : "tabpanel"} id="monitor-panel-receipts" aria-label={readOnly ? t("验收收据") : undefined} aria-labelledby={readOnly ? undefined : "monitor-tab-receipts"}><section className="gov-card"><div className="section-heading"><h2>{t("验收收据")}</h2><Button variant="quiet" disabled={runs.isFetching} onClick={() => void runs.refetch()}>{t("刷新")}</Button></div><p className="page-description">{t("HTTP 200 不代表通过：必须验证精确文本、完整流结束或指定工具参数。只保存状态和允许的用量证据，不保存响应正文。保留 30 天，显示最近 100 条。")}</p>
      {runs.error && <InlineError message={String(runs.error)} onRetry={() => void runs.refetch()} />}
      {runs.isPending && <p role="status">{t("读取中")}</p>}
      {initialRunId && runs.data && !runs.data.some(run => run.id === initialRunId) && <p role="status">{t("所选验收收据不在最近记录中，请刷新或查看其余记录。")}</p>}
      <ol className="gov-receipts">{runs.data?.map(run => <li key={run.id} data-selected={initialRunId === run.id}>
        <ReceiptEvidence run={run} providers={providers} expanded={initialRunId === run.id} />
      </li>)}</ol>
      {runs.data?.length === 0 && <p className="page-description">{t("尚未执行验收；不会自动发送测试请求。")}</p>}
    </section></div>}
  </div>;
}

function ReceiptEvidence({ run, providers, expanded = false }: { run: SyntheticRun; providers: Provider[]; expanded?: boolean }) {
  return <div className="gov-receipt-content">
        <div className="gov-receipt__stamp"><time>{stamp(run.createdAt)}</time><Badge tone={run.status === "passed" ? "success" : run.status === "failed" ? "danger" : "neutral"}>{checkStatus(run.status)}</Badge><span className="gov-number">{run.latencyMs} ms</span></div>
        <div className="gov-receipt__evidence"><header><strong>{providers.find(p => p.id === run.providerId)?.name ?? run.providerId}</strong><code>{run.mode}</code></header><code className="gov-id" tabIndex={0}>{run.model}</code>
          <details className="workspace-disclosure" open={expanded || undefined}><summary>{t("查看验收证据")}</summary><div className="workspace-disclosure__body"><dl className="gov-receipt__facts"><div className="gov-receipt__model"><dt>{t("返回模型")}</dt><dd><code className="gov-id" tabIndex={0}>{run.evidence.reportedModel ?? "—"}</code></dd></div><div><dt>HTTP</dt><dd>{run.evidence.httpStatus ?? "—"}</dd></div><div><dt>Token</dt><dd>{run.evidence.inputTokens ?? "?"} / {run.evidence.outputTokens ?? "?"}</dd></div></dl><code className="gov-detail">{run.detail}</code></div></details>
        </div>
  </div>;
}
