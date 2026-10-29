import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { issueAccessToken, listAccessTokens, loadAccessPolicy, revokeAccessToken, saveAccessPolicy } from "../../lib/governance-service";
import { validInteger } from "../../lib/governance-diff";
import type { AccessPolicy, IssuedAccessToken } from "../../types/governance";
import { Badge, Button, Card, Field, InlineError, Input, Select, Toggle } from "../../ui";
import { Confirmation, OperationMessage, stamp, useClock, useGovernanceOperation } from "./shared";

export function AccessPanel({ proxyStopped }: { proxyStopped: boolean }) {
  const policy = useQuery({ queryKey: ["governance", "access"], queryFn: loadAccessPolicy });
  const tokens = useQuery({ queryKey: ["governance", "tokens"], queryFn: listAccessTokens });
  const [draft, setDraft] = useState<AccessPolicy | null>(null), [confirmed, setConfirmed] = useState(false);
  const [label, setLabel] = useState(""), [scope, setScope] = useState<"proxy" | "monitor">("proxy"), [days, setDays] = useState(30);
  const [issued, setIssued] = useState<IssuedAccessToken | null>(null), [revokeId, setRevokeId] = useState(""), [revokeWord, setRevokeWord] = useState("");
  const op = useGovernanceOperation(), now = useClock();
  useEffect(() => { if (!draft && policy.data) setDraft({ ...policy.data }); }, [policy.data, draft]);
  const change = (values: Partial<AccessPolicy>) => { setDraft(d => d ? { ...d, ...values } : d); setConfirmed(false); };
  const valid = draft && validInteger(draft.requestsPerMinute, 1, 10000) && validInteger(draft.maxInFlight, 1, 128);
  return <div className="gov-stack"><OperationMessage op={op} />
    {policy.error && <InlineError message={String(policy.error)} onRetry={() => void policy.refetch()} />}
    <Card className="gov-card"><h2>{t("代理访问控制")}</h2>
      <p className="page-description">{t("仅监听回环地址，拒绝跨站 Origin 和错误 Host；访问令牌不等于供应商 API Key。")}</p>
      {draft ? <fieldset disabled={op.busy} className="gov-fieldset">
        <label className="gov-toggle"><span>{t("要求本地访问令牌")}</span><Toggle aria-label={t("要求本地访问令牌")} checked={draft.requireToken} onCheckedChange={requireToken => change({ requireToken })} /></label>
        {!draft.requireToken && <p className="gov-warning">{t("兼容模式：本地模型接口不要求认证。只读状态接口始终要求令牌。")}</p>}
        <div className="gov-grid"><Field label={t("每分钟请求上限")}><Input type="number" min={1} max={10000} value={draft.requestsPerMinute} onChange={e => change({ requestsPerMinute: Number(e.target.value) })} /></Field><Field label={t("最大并发（含流式响应）")}><Input type="number" min={1} max={128} value={draft.maxInFlight} onChange={e => change({ maxInFlight: Number(e.target.value) })} /></Field></div>
        <p className="page-description">{t("修改策略前必须停止代理并恢复客户端接管；重新接管会写入受管令牌。撤销或到期后，需要恢复并重新接管。")}</p>
        <label className="gov-check"><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />{t("我已检查策略，确认应用此变更")}</label>
        <div className="gov-actions"><Button disabled={!valid || !confirmed || !proxyStopped} onClick={() => void op.run(async () => { await saveAccessPolicy(draft, confirmed); setConfirmed(false); await op.refresh(); }, t("访问策略已保存"))}>{t("保存访问策略")}</Button><Button variant="quiet" onClick={() => { if (policy.data) setDraft({ ...policy.data }); setConfirmed(false); }}>{t("放弃草稿")}</Button></div>
        {!proxyStopped && <p className="gov-warning">{t("请先停止代理，并在设置中恢复接管。")}</p>}
      </fieldset> : <p>{t("读取中")}</p>}
    </Card>
    <Card className="gov-card"><h2>{t("本地访问令牌")}</h2><p className="page-description">{t("令牌只显示一次，数据库仅保存哈希。monitor 只能读取 /xy/status，不能请求模型。限流与并发限制由所有令牌共享。")}</p>
      <fieldset className="gov-fieldset" disabled={op.busy || !!issued}>
        <div className="gov-grid"><Field label={t("令牌名称")}><Input maxLength={160} value={label} autoComplete="off" onChange={e => setLabel(e.target.value)} /></Field><Field label={t("权限范围")}><Select value={scope} onChange={e => setScope(e.target.value as typeof scope)}><option value="proxy">{t("代理与只读状态")}</option><option value="monitor">{t("仅只读状态")}</option></Select></Field><Field label={t("有效天数")}><Input type="number" min={1} max={365} value={days} onChange={e => setDays(Number(e.target.value))} /></Field></div>
        <Button disabled={!label.trim() || !validInteger(days, 1, 365)} onClick={() => void op.run(async () => { const result = await issueAccessToken(label, scope, days); setIssued(result); setLabel(""); await op.refresh(); })}>{t("创建令牌")}</Button>
      </fieldset>
      {issued && <div className="gov-secret" role="status"><strong>{t("请现在保存令牌，关闭后无法再次查看。")}</strong><code>{issued.token}</code><div className="gov-actions"><Button disabled={op.busy} onClick={() => void op.run(() => navigator.clipboard.writeText(issued.token), t("令牌已复制"))}>{t("复制令牌")}</Button><Button variant="quiet" onClick={() => setIssued(null)}>{t("我已保存，隐藏令牌")}</Button></div></div>}
      {tokens.error && <InlineError message={String(tokens.error)} onRetry={() => void tokens.refetch()} />}
      <ul className="gov-list">{tokens.data?.map(token => <li key={token.id}><div><strong>{token.label}</strong><p><code>{token.scope}</code> · {t("到期时间")} {stamp(token.expiresAt)}</p><Badge tone={token.revoked || token.expiresAt <= now ? "neutral" : "success"}>{token.revoked ? t("已撤销") : token.expiresAt <= now ? t("已过期") : t("有效")}</Badge></div><Button variant="danger" disabled={op.busy || token.revoked} onClick={() => { setRevokeId(token.id); setRevokeWord(""); }}>{t("撤销")}</Button></li>)}</ul>
      {tokens.data?.length === 0 && <p className="page-description">{t("还没有访问令牌")}</p>}
      {revokeId && <div className="gov-confirm"><p>{t("撤销后使用此令牌的新请求会被拒绝；已经准入的请求不会强制中断。")}</p><code>{tokens.data?.find(token => token.id === revokeId)?.label}</code><Confirmation word="REVOKE" value={revokeWord} onChange={setRevokeWord} disabled={op.busy} /><div className="gov-actions"><Button variant="danger" disabled={op.busy || revokeWord !== "REVOKE"} onClick={() => void op.run(async () => { await revokeAccessToken(revokeId, true); setRevokeId(""); setRevokeWord(""); await op.refresh(); }, t("令牌已撤销"))}>{t("确认撤销")}</Button><Button variant="quiet" disabled={op.busy} onClick={() => setRevokeId("")}>{t("取消")}</Button></div></div>}
    </Card>
  </div>;
}
