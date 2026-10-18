import type { VerificationEvidence } from "../../types/model-verification";
import { CLAIM_LABELS, CODE_LABELS, PROBE_LABELS, isCompleteVerificationResponse } from "../../lib/model-verification";
import { Badge } from "../../ui";
import { judgeVerificationEvidence } from "../../lib/model-judgment";
import "./response-model.css";
const number = (n: number | null) => n == null ? "未知" : n.toLocaleString();
export function VerificationDetails({ evidence: e }: { evidence: VerificationEvidence }) {
  const judgment = judgeVerificationEvidence(e);
  const complete = isCompleteVerificationResponse(e);
  const claim = !complete ? "UNKNOWN" : judgment.kind === "claim_conflict" || judgment.kind === "claim_mismatch" ? "MISMATCH"
    : judgment.declaredModels.length === 0 ? "MISSING" : e.claimConsistency;
  return <div className="verification-evidence" data-observation-id={e.observationId}>
    <div className="inline-actions"><Badge tone={e.riskLevel === "warning" ? "warning" : "neutral"}>{CLAIM_LABELS[claim] ?? "证据不足"}</Badge>
      <Badge tone="warning">{e.identityAssurance === "INCONCLUSIVE" ? "身份证据不足" : "真实身份未验证"}</Badge><small>响应 #{e.observationId}</small></div>
    <div className="inline-actions" data-judgment={judgment.kind}><strong>检测结论</strong><Badge tone={judgment.tone}>{judgment.label}</Badge></div>
    <p className="page-description">{judgment.summary}</p>
    <p className="page-description">下一步：{judgment.nextStep}</p>
    {complete && <p className="page-description">{e.explanation}</p>}
    <dl className="key-values verification-evidence__values">
      <dt>选择 → 转发</dt><dd><code>{e.requestedModel} → {e.forwardedModel ?? "未知"}</code></dd>
      <dt>本条上游声明</dt><dd><code>{e.reportedModels.join(" → ") || "未返回"}</code></dd>
      <dt>观察时间 / 来源</dt><dd>{new Date(e.observedAt).toLocaleString("zh-CN", { hour12: false })} · {e.source === "passive" ? "业务响应（无额外请求）" : "可选收费复核"}</dd>
      <dt>协议 / 形态</dt><dd>{e.protocol} / {e.protocolShape ?? "未知"} · {e.isStreaming ? "SSE" : "JSON"}</dd>
      <dt>HTTP / 完整性</dt><dd>{e.httpStatus ?? "未知"} / {e.streamCompleted ? "完整" : "不完整"} · {e.finishReason ?? "结束原因未知"}</dd>
      <dt>用量完整度</dt><dd>{{ complete: "最终统计", contradictory: "统计不自洽，费用未知", partial: "仅部分统计", missing: "未提供", unknown: "未知" }[e.usageStatus] ?? e.usageStatus}</dd>
      <dt>输入 / 输出 / 推理</dt><dd>{number(e.inputTokens)} / {number(e.outputTokens)} / {number(e.reasoningTokens)}</dd>
      <dt>总计 / 缓存读 / 写</dt><dd>{number(e.totalTokens)} / {number(e.cacheReadTokens)} / {number(e.cacheCreationTokens)}</dd>
      <dt>近期窗口</dt><dd>{e.rollingObservedCount} 条 · {e.routeChangeCount} 次未确认路由变化{e.repeatedAnomaly ? " · 重复异常" : ""}</dd>
      <dt>收费复核</dt><dd>{PROBE_LABELS[e.activeProbeState] ?? e.activeProbeState} · 用量 {number(e.activeProbeTokens)}</dd>
      {e.responseId && <><dt>响应 ID</dt><dd><code>{e.responseId}</code></dd></>}
      {e.providerRequestId && <><dt>请求 ID</dt><dd><code>{e.providerRequestId}</code></dd></>}
    </dl>
    <ul className="verification-codes">{e.evidenceCodes.map(code => <li key={code}><code>{code}</code> · {code === "CLAIM_MATCH" && !complete ? "历史匹配标记已失效，以当前检测结论为准" : CODE_LABELS[code] ?? "保留的诊断线索"}</li>)}</ul>
    <small className="muted">证据来源：不可信上游响应元数据。仅对本条响应有效；声明匹配不是模型身份认证。</small>
  </div>;
}
