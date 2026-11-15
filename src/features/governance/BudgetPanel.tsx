import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { loadHardBudget, saveHardBudget } from "../../lib/governance-service";
import { validInteger } from "../../lib/governance-diff";
import type { BudgetPolicy } from "../../types/governance";
import { Button, Field, InlineError, Input, Toggle } from "../../ui";
import { OperationMessage, useGovernanceOperation } from "./shared";
import { useTaskDraft, useTaskNavigation } from "../settings/TaskNavigation";
import { BudgetLedger } from "./BudgetLedger";

export function BudgetPanel() {
  const [editing, setEditing] = useState(false);
  const { leave } = useTaskNavigation();
  const query = useQuery({ queryKey: ["governance", "budget"], queryFn: loadHardBudget });
  const [draft, setDraft] = useState<BudgetPolicy | null>(null), [confirmed, setConfirmed] = useState(false);
  const op = useGovernanceOperation();
  useEffect(() => { if (!draft && query.data) setDraft({ ...query.data.policy }); }, [query.data, draft]);
  const change = (v: Partial<BudgetPolicy>) => { setDraft(d => d ? { ...d, ...v } : d); setConfirmed(false); };
  useTaskDraft(editing && (Boolean(draft && query.data && JSON.stringify(draft) !== JSON.stringify(query.data.policy)) || confirmed), op.busy);
  const valid = draft && validInteger(draft.dailyTokens, 0, 1000000000) && validInteger(draft.dailyMicrousd, 0, 1000000000000) && validInteger(draft.inputReservation, 256, 1000000) && validInteger(draft.maxOutputTokens, 1, 128000) && validInteger(draft.taskMaxAttempts, 0, 10000) && validInteger(draft.taskMaxMicrousd, 0, 1000000000000);
  return <div className="gov-stack">
    <OperationMessage op={op} />{query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
    <section className="gov-card"><div className="section-heading"><h2>{editing ? t("修改预算策略") : t("当前预算与账本")}</h2>{editing ? <Button variant="quiet" disabled={op.busy} onClick={() => leave(() => { if (query.data) setDraft({ ...query.data.policy }); setConfirmed(false); setEditing(false); })}>{t("返回当前账本")}</Button> : <Button variant="secondary" disabled={!query.data} onClick={() => { setDraft({ ...query.data!.policy }); setConfirmed(false); setEditing(true); }}>{t("修改预算策略")}</Button>}</div>
      <p className="gov-warning">{t("这是本地预留准入额度，不是供应商最终账单保证。输入按 UTF-8 大小做保守检查，不是任意模型的精确 Token 计数。")}</p>
      <p className="page-description">{t("每次上游尝试（包括重试、故障转移和模型复核）先扣预留，再发送。失败、取消及未知用量不退款。重启或修改限额不会清零。每日按 UTC 划分。")}</p>
      <div className="gov-budget-workbench">
      {!editing && query.data && <BudgetLedger value={query.data} />}
      {editing && (draft ? <fieldset disabled={op.busy} className="gov-fieldset gov-policy-editor">
        <label className="gov-toggle"><span>{t("启用硬预算准入")}</span><Toggle aria-label={t("启用硬预算准入")} checked={draft.enabled} onCheckedChange={enabled => change({ enabled })} /></label>
        {!draft.enabled && <p className="gov-warning">{t("预算闸关闭时，新任务不设预算上限；已有受保护任务仍执行历史上限与预留。收费验收和计划监控将被阻断。")}</p>}
        <section className="gov-group"><h3>{t("每日额度")}</h3><div className="gov-form-rows"><Field label={t("每日 Token 预留上限")}><Input type="number" min={0} max={1000000000} value={draft.dailyTokens} onChange={e => change({ dailyTokens: Number(e.target.value) })} /></Field><Field label={t("每日费用预留上限（USD）")}><Input type="number" min={0} max={1000000} step="0.000001" value={draft.dailyMicrousd / 1000000} onChange={e => change({ dailyMicrousd: Math.round(Number(e.target.value) * 1000000) })} /></Field></div>
        </section>
        <section className="gov-group"><h3>{t("单次请求预留")}</h3><div className="gov-form-rows"><Field label={t("每次输入预留 Token")}><Input type="number" min={256} max={1000000} value={draft.inputReservation} onChange={e => change({ inputReservation: Number(e.target.value) })} /></Field><Field label={t("每次最大输出 Token")}><Input type="number" min={1} max={128000} value={draft.maxOutputTokens} onChange={e => change({ maxOutputTokens: Number(e.target.value) })} /></Field></div></section>
        <section className="gov-group"><h3>{t("任务上限与重试")}</h3>
        <div className="gov-form-rows"><Field label={t("单任务总尝试上限")}><Input type="number" min={0} max={10000} value={draft.taskMaxAttempts} onChange={event => change({ taskMaxAttempts: Number(event.target.value) })} /></Field><Field label={t("单任务费用占用上限（USD）")}><Input type="number" min={0} max={1000000} step="0.000001" value={draft.taskMaxMicrousd / 1000000} onChange={event => change({ taskMaxMicrousd: Math.round(Number(event.target.value) * 1000000) })} /></Field></div>
        <p className="page-description">{t("同一任务合并应用重试、故障转移、客户端重试和模型复核。Codex 使用会话与根轮次标识；其他客户端需发送稳定的 X-XY-Task-ID 或 Idempotency-Key。身份缺失或冲突会在发送前拒绝。")}</p>
        <p className="page-description">{t("任务累计跨日和重启保留。收紧限额对已有任务生效，放宽或关闭开关不会提高已有任务的上限。零上限会阻断下一次发送。")}</p>
        </section>
        <p className="page-description">{t("必须先在用量与审计中，为实际派发模型 ID 保存精确自定义价格。参考价和模糊匹配不能授权请求。缓存按最高输入价预留；费用按微美元向上取整。")}</p>
        <details className="workspace-disclosure"><summary>{t("支持范围与计费方式")}</summary><div className="workspace-disclosure__body gov-view">
        <p className="page-description">{t("Responses 支持有界文本、本地 function/custom 工具、命名空间、客户端 tool_search 和内联动态工具定义。工具定义、工具结果和完整会话历史均计入输入预留。")}</p>
        <p className="page-description">{t("多模态、远端历史、非空加密推理、服务端工具、未知协议字段与多候选会被拒绝。Chat 强制 max_completion_tokens，旧网关可能不兼容。")}</p>
        </div></details>
        <div className="gov-approval"><label className="gov-check"><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />{t("我已检查策略，确认应用此变更")}</label>
        <div className="gov-actions"><Button disabled={!valid || !confirmed} onClick={() => void op.run(async () => { await saveHardBudget(draft, confirmed); setConfirmed(false); setEditing(false); await op.refresh(); }, t("预算策略已保存，历史预留保持不变"))}>{t("保存预算策略")}</Button><Button variant="quiet" onClick={() => { if (query.data) setDraft({ ...query.data.policy }); setConfirmed(false); }}>{t("放弃草稿")}</Button></div></div>
      </fieldset> : <p>{t("读取中")}</p>)}
      </div>
    </section>
  </div>;
}
