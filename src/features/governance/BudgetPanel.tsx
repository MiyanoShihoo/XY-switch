import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { loadHardBudget, saveHardBudget } from "../../lib/governance-service";
import { validInteger } from "../../lib/governance-diff";
import type { BudgetPolicy } from "../../types/governance";
import { Button, Card, Field, InlineError, Input, Stat, Toggle } from "../../ui";
import { OperationMessage, usd, useGovernanceOperation } from "./shared";

export function BudgetPanel() {
  const query = useQuery({ queryKey: ["governance", "budget"], queryFn: loadHardBudget });
  const [draft, setDraft] = useState<BudgetPolicy | null>(null), [confirmed, setConfirmed] = useState(false);
  const op = useGovernanceOperation();
  useEffect(() => { if (!draft && query.data) setDraft({ ...query.data.policy }); }, [query.data, draft]);
  const change = (v: Partial<BudgetPolicy>) => { setDraft(d => d ? { ...d, ...v } : d); setConfirmed(false); };
  const valid = draft && validInteger(draft.dailyTokens, 0, 1000000000) && validInteger(draft.dailyMicrousd, 0, 1000000000000) && validInteger(draft.inputReservation, 256, 1000000) && validInteger(draft.maxOutputTokens, 1, 128000);
  return <div className="gov-stack"><OperationMessage op={op} />{query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
    <Card className="gov-card"><h2>{t("费用 / Token 硬预算闸")}</h2>
      <p className="gov-warning">{t("这是本地预留准入额度，不是供应商最终账单保证。输入按 UTF-8 大小做保守检查，不是任意模型的精确 Token 计数。")}</p>
      <p className="page-description">{t("每次上游尝试（包括重试、故障转移和模型复核）先扣预留，再发送。失败、取消及未知用量不退款。重启或修改限额不会清零。每日按 UTC 划分。")}</p>
      {query.data && <div className="gov-stats"><Stat label={t("今日预留 Token")} value={query.data.reservedTokens.toLocaleString()} /><Stat label={t("今日预留费用")} value={usd(query.data.reservedMicrousd)} /><Stat label={t("今日准入尝试")} value={query.data.attempts} /></div>}
      {draft ? <fieldset disabled={op.busy} className="gov-fieldset">
        <label className="gov-toggle"><span>{t("启用硬预算准入")}</span><Toggle aria-label={t("启用硬预算准入")} checked={draft.enabled} onCheckedChange={enabled => change({ enabled })} /></label>
        {!draft.enabled && <p className="gov-warning">{t("预算闸关闭时不限制普通代理请求；收费验收和计划监控将被阻断。")}</p>}
        <div className="gov-grid"><Field label={t("每日 Token 预留上限")}><Input type="number" min={0} max={1000000000} value={draft.dailyTokens} onChange={e => change({ dailyTokens: Number(e.target.value) })} /></Field><Field label={t("每日费用预留上限（USD）")}><Input type="number" min={0} max={1000000} step="0.000001" value={draft.dailyMicrousd / 1000000} onChange={e => change({ dailyMicrousd: Math.round(Number(e.target.value) * 1000000) })} /></Field><Field label={t("每次输入预留 Token")}><Input type="number" min={256} max={1000000} value={draft.inputReservation} onChange={e => change({ inputReservation: Number(e.target.value) })} /></Field><Field label={t("每次最大输出 Token")}><Input type="number" min={1} max={128000} value={draft.maxOutputTokens} onChange={e => change({ maxOutputTokens: Number(e.target.value) })} /></Field></div>
        <p className="page-description">{t("必须先在用量与审计中，为实际派发模型 ID 保存精确自定义价格。参考价和模糊匹配不能授权请求。缓存按最高输入价预留；费用按微美元向上取整。")}</p>
        <p className="page-description">{t("启用后仅准入有界纯文本和本地函数工具；多模态、远端历史、内建联网工具、未知参数与多候选会被拒绝。Chat 强制 max_completion_tokens，旧网关可能不兼容。")}</p>
        <label className="gov-check"><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />{t("我已检查策略，确认应用此变更")}</label>
        <div className="gov-actions"><Button disabled={!valid || !confirmed} onClick={() => void op.run(async () => { await saveHardBudget(draft, confirmed); setConfirmed(false); await op.refresh(); }, t("预算策略已保存，历史预留保持不变"))}>{t("保存预算策略")}</Button><Button variant="quiet" onClick={() => { if (query.data) setDraft({ ...query.data.policy }); setConfirmed(false); }}>{t("放弃草稿")}</Button></div>
      </fieldset> : <p>{t("读取中")}</p>}
    </Card>
  </div>;
}
