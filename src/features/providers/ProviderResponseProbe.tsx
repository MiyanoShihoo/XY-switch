import { useEffect, useRef, useState } from "react";
import { RefreshCw, ScanSearch } from "lucide-react";
import { Badge, Button, Field, InlineError, Input, Toggle } from "../../ui";
import { errorText } from "../../lib/error";
import { probeProviderResponseModel } from "../../lib/response-model-service";
import { confirmVerificationCost, getVerificationStatus, saveVerificationSettings } from "../../lib/model-verification-service";
import { eligibleEvidence } from "../../lib/model-verification";
import { DEFAULT_VERIFICATION_SETTINGS, VERIFICATION_TOKEN_BUDGET_RANGE, type VerificationSettings, type VerificationStatus } from "../../types/model-verification";
import type { Provider } from "../../types/provider";
import type { ResponseModelProbe } from "../../types/response-model";
import { ResponseModelEvidence } from "../usage/ResponseModelEvidence";
import { VerificationDetails } from "../usage/VerificationDetails";
import { VerificationRouteSummary } from "../usage/VerificationRouteSummary";

export function ProviderResponseProbe({ provider, busy }: { provider: Provider; busy: boolean }) {
  const [model, setModel] = useState(provider.model);
  const [status, setStatus] = useState<VerificationStatus | null>(null);
  const [settings, setSettings] = useState<VerificationSettings>(DEFAULT_VERIFICATION_SETTINGS);
  const [draft, setDraft] = useState<VerificationSettings>({ ...DEFAULT_VERIFICATION_SETTINGS, aliases: [] });
  const [alias, setAlias] = useState("");
  const [allowRoute, setAllowRoute] = useState(false);
  const [result, setResult] = useState<ResponseModelProbe | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const revision = useRef(0);
  const mutating = useRef(false);
  useEffect(() => {
    let alive = true; let first = true;
    revision.current += 1; setStatus(null); setResult(null); setError("");
    const refresh = async () => {
      if (mutating.current) return;
      const current = revision.current;
      try {
        const next = await getVerificationStatus(provider.id, model.trim());
        if (alive && current === revision.current && !mutating.current) { setStatus(next); setSettings(next.settings); if (first) { setDraft(next.settings); first = false; } }
      } catch (reason) { if (alive && current === revision.current) setError(errorText(reason)); }
    };
    void refresh();
    // Local IPC only; never polls an upstream API or model catalogue.
    const timer = setInterval(() => void refresh(), 4000);
    return () => { alive = false; revision.current += 1; clearInterval(timer); };
  }, [provider.id, model]);
  const action = async (operation: () => Promise<unknown>) => {
    const current = ++revision.current;
    mutating.current = true; setSaving(true); setError("");
    try {
      await operation();
      const next = await getVerificationStatus(provider.id, model.trim());
      if (current === revision.current) { setStatus(next); setSettings(next.settings); setDraft(next.settings); }
    } catch (reason) { if (current === revision.current) setError(errorText(reason)); }
    finally { mutating.current = false; if (current === revision.current) setSaving(false); }
  };
  const set = (change: Partial<VerificationSettings>) => action(() => saveVerificationSettings({ ...settings, ...change }));
  const anomaly = status?.recent.find(eligibleEvidence);
  const probe = async () => {
    if (!anomaly) return;
    const current = revision.current; setRunning(true); setResult(null); setError("");
    try {
      const next = await probeProviderResponseModel(provider.id, model.trim(), anomaly.observationId);
      if (current === revision.current) setResult(next);
    } catch (reason) { if (current === revision.current) setError(errorText(reason)); }
    finally { setRunning(false); }
  };
  const controlsDisabled = busy || saving || !status;
  const budgetFields = [
    ["maxRequestsPerDay", "每日最多复核次数", 1, 20], ["softTokenBudgetPerDay", "每日软 Token 预算", VERIFICATION_TOKEN_BUDGET_RANGE.min, VERIFICATION_TOKEN_BUDGET_RANGE.max],
    ["maxOutputTokens", "单次输出上限", 1, 64], ["timeoutSeconds", "超时（秒）", 1, 30],
    ["cooldownHours", "冷却（小时）", 1, 168], ["rollingWindowSize", "近期观察窗口", 2, 100],
    ["repeatedAnomalyThreshold", "重复异常阈值", 1, draft.rollingWindowSize],
  ] as const;
  return <section className="section provider-response-probe" aria-label="低 Token 模型验证">
    <div className="verification-toggle-line"><h3>低 Token 模型验证</h3><Toggle aria-label="启用模型验证（全局）" checked={settings.enabled} disabled={controlsDisabled} onCheckedChange={enabled => void set({ enabled })} /></div>
    <p className="page-description">整个模块默认关闭。开启后只观察正常请求的同一份响应，额外 API 请求 / 推理 Token 均为 0；不插入提示词，不改业务参数。此开关全局生效。</p>
    {!settings.enabled && <p className="page-description">当前不新增验证元数据、不发送复核请求；普通用量日志和已有记录保留。开关不会启动代理或切换客户端。</p>}
    {error && <InlineError message={error} />}
    {status?.contextError && <InlineError message={status.contextError} />}
    {settings.enabled && <div className="form-stack">
      <Field label="观察的逻辑模型" description="与业务请求选择的模型 ID 精确对应；模型映射后的实际转发名称单独记录。"><Input value={model} maxLength={256} disabled={saving || running} onChange={event => setModel(event.target.value)} /></Field>
      <div className="inline-actions"><Badge tone="neutral">被动观察 {status?.observedResponses ?? 0} 条</Badge><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} disabled={controlsDisabled} onClick={() => void action(() => Promise.resolve())}>刷新本地证据</Button></div>
      {status && <VerificationRouteSummary recent={status.recent} providerId={provider.id} model={model.trim()} />}
      {status?.storageError && <InlineError message="验证存储存在异常，未影响业务请求；请检查存储后重新保存设置。收费复核不会绕过存储失败。" />}
      {!!status?.droppedObservations && <p className="page-description">观察队列繁忙，已跳过 {status.droppedObservations} 条元数据；未阻塞原请求。</p>}
      <div className="verification-toggle-line"><strong>允许收费异常复核（全局）</strong><Toggle aria-label="允许收费异常复核（全局）" checked={settings.activeProbeEnabled} disabled={controlsDisabled} onCheckedChange={activeProbeEnabled => void set({ activeProbeEnabled })} /></div>
      <p className="page-description">默认关闭。只有有效异常、此路由的费用边界确认、预算和冷却检查全部通过，才会自动发送一次固定输入「Reply OK.」。不重试；失败也占次数，用量未知不当免费。</p>
      <details className="verification-settings"><summary>预算、隐私与别名设置</summary><div className="form-stack">
        <div className="verification-budget-grid">{budgetFields.map(([key, label, min, max]) => <Field key={key} label={label}><Input aria-label={label} type="number" min={min} max={max} step={1} value={draft[key]} disabled={saving || running} onChange={event => setDraft({ ...draft, [key]: Number(event.target.value) })} /></Field>)}</div>
        <p className="page-description">每日软 Token 预算默认 100，可调整为 0–10000；0 表示不允许收费复核，不影响被动观察。次数和软预算按上游 + 逻辑模型计算，UTC 00:00 重置；跨午夜仍受冷却限制。每次先预留 32 输入 + 输出上限；实际统计可能超过软预算，不是金额硬上限。</p>
        <div className="verification-toggle-line"><span>保存白名单诊断 ID（默认不保存）</span><Toggle aria-label="保存诊断 ID" checked={draft.storeDiagnosticIds} onCheckedChange={storeDiagnosticIds => setDraft({ ...draft, storeDiagnosticIds })} disabled={saving || running} /></div>
        <Button variant="secondary" disabled={controlsDisabled || running} onClick={() => void action(() => saveVerificationSettings({ ...draft, enabled: settings.enabled, activeProbeEnabled: settings.activeProbeEnabled }))}>保存预算与隐私设置</Button>
        <Field label="明确确认的上游别名" description="不自动删除日期、版本或后缀。确认仅改变名称比较，不等于身份证明。"><Input value={alias} maxLength={256} placeholder="精确填写允许的 reported_model" disabled={saving} onChange={event => setAlias(event.target.value)} /></Field>
        <div className="verification-toggle-line"><span>同时允许该别名参与已知回退 / 路由变化</span><Toggle aria-label="允许此别名参与路由变化" checked={allowRoute} onCheckedChange={setAllowRoute} disabled={saving} /></div>
        <Button variant="secondary" disabled={controlsDisabled || !alias.trim() || !model.trim()} onClick={() => void set({ aliases: [...settings.aliases.filter(a => !(a.providerId === provider.id && a.requestedModel === model.trim() && a.reportedModel === alias.trim())), { providerId: provider.id, requestedModel: model.trim(), reportedModel: alias.trim(), userConfirmed: true, allowRouteChange: allowRoute }] })}>确认并添加此精确别名</Button>
        {settings.aliases.filter(a => a.providerId === provider.id).map(a => <div className="verification-alias" key={a.requestedModel + ":" + a.reportedModel}><code>{a.requestedModel} → {a.reportedModel}{a.allowRouteChange ? "（允许路由变化）" : ""}</code><Button variant="quiet" size="sm" disabled={saving} onClick={() => void set({ aliases: settings.aliases.filter(item => item !== a) })}>移除</Button></div>)}
      </div></details>
      {settings.activeProbeEnabled && <>
        <div className="verification-toggle-line"><strong>确认当前路由费用边界</strong><Toggle aria-label="确认当前路由费用边界" checked={status?.capabilityConfirmed ?? false} disabled={controlsDisabled || !provider.enabled || !model.trim()} onCheckedChange={confirmed => void action(() => confirmVerificationCost(provider.id, model.trim(), confirmed))} /></div>
        <p className="page-description">仅在已确认该路由支持输出上限、短输入连同包装不超过 32 token，且无额外未受限的推理 / 工具用量时开启。不确定就保持关闭。凭据、协议、端点、映射或别名变化后需重新确认；确认本身不发请求。Chat 使用包含推理的 max_completion_tokens；未知或不支持的上游协议在发送前拒绝。</p>
        <dl className="key-values"><dt>今日复核次数</dt><dd>{status?.probeRequestsToday ?? 0} / {settings.maxRequestsPerDay}</dd><dt>已占软预算</dt><dd>{status?.budgetUsedToday ?? 0} / {settings.softTokenBudgetPerDay} token</dd><dt>上报实际用量</dt><dd>{status?.probeTokensToday == null ? "未知（保留预留）" : status.probeTokensToday + " token"}</dd></dl>
        <Button variant="secondary" icon={<ScanSearch size={15} />} loading={running} disabled={controlsDisabled || running || !anomaly || !status?.capabilityConfirmed || !provider.enabled || settings.softTokenBudgetPerDay === 0} onClick={() => void probe()}>复核最近一条异常（共用预算）</Button>
        {settings.softTokenBudgetPerDay === 0 && <small className="muted">每日软 Token 预算为 0，收费复核已暂停；被动观察不受影响。</small>}
        {!anomaly && <small className="muted">没有当前路由的完整被动异常；不会为取得结果而主动请求。</small>}
      </>}
      {result && <div aria-live="polite"><ResponseModelEvidence audit={result.audit} />{result.errorMessage && <InlineError message={result.errorMessage} />}</div>}
      {!status?.recent.length ? <p className="page-description">尚无本次路由的响应证据。正常业务请求完成后在此显示；不会继承其他凭据或配置的结论。</p> : <div className="form-stack">{status.recent.map((e, index) => <details key={e.observationId} open={index === 0}><summary>响应 #{e.observationId} · {e.source === "passive" ? "被动观察" : "收费复核"} · {e.reportedModels.join(" / ") || "声明缺失"}</summary><VerificationDetails evidence={e} /></details>)}</div>}
    </div>}
  </section>;
}
