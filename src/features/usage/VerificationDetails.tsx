import { translateMessage, t, localeTag, useLanguage } from "../../i18n/language";
import type { VerificationEvidence } from "../../types/model-verification";
import { CLAIM_LABELS, CODE_LABELS, PROBE_LABELS, isCompleteVerificationResponse } from "../../lib/model-verification";
import { Badge } from "../../ui";
import { judgeVerificationEvidence } from "../../lib/model-judgment";
import "./response-model.css";
const number = (n: number | null) => n == null ? t("未知") : n.toLocaleString(localeTag());
export function VerificationDetails({ evidence: e }: { evidence: VerificationEvidence }) {
  useLanguage();
  const judgment = judgeVerificationEvidence(e);
  const complete = isCompleteVerificationResponse(e);
  const claim = !complete ? "UNKNOWN" : judgment.kind === "claim_conflict" || judgment.kind === "claim_mismatch" ? "MISMATCH"
    : judgment.declaredModels.length === 0 ? "MISSING" : e.claimConsistency;
  return <div className="verification-evidence" data-observation-id={e.observationId}>
    <div className="inline-actions"><Badge tone={e.riskLevel === "warning" ? "warning" : "neutral"}>{CLAIM_LABELS[claim] ?? t("证据不足")}</Badge>
      <Badge tone="warning">{e.identityAssurance === "INCONCLUSIVE" ? t("身份证据不足") : t("真实身份未验证")}</Badge><small>{t("响应 #")}{e.observationId}</small></div>
    <div className="inline-actions" data-judgment={judgment.kind}><strong>{t("检测结论")}</strong><Badge tone={judgment.tone}>{judgment.label}</Badge></div>
    <p className="page-description">{judgment.summary}</p>
    <p className="page-description">{t("下一步：")}{judgment.nextStep}</p>
    {complete && <p className="page-description">{translateMessage(e.explanation)}</p>}
    <dl className="key-values verification-evidence__values">
      <dt>{t("选择 → 转发")}</dt><dd><code>{e.requestedModel} → {e.forwardedModel ?? t("未知")}</code></dd>
      <dt>{t("本条上游声明")}</dt><dd><code>{e.reportedModels.join(" → ") || t("未返回")}</code></dd>
      <dt>{t("观察时间 / 来源")}</dt><dd>{new Date(e.observedAt).toLocaleString("zh-CN", { hour12: false })} · {e.source === "passive" ? t("业务响应（无额外请求）") : t("可选收费复核")}</dd>
      <dt>{t("协议 / 形态")}</dt><dd>{e.protocol} / {e.protocolShape ?? t("未知")} · {e.isStreaming ? "SSE" : "JSON"}</dd>
      <dt>{t("HTTP / 完整性")}</dt><dd>{e.httpStatus ?? t("未知")} / {e.streamCompleted ? t("完整") : t("不完整")} · {e.finishReason ?? t("结束原因未知")}</dd>
      <dt>{t("用量完整度")}</dt><dd>{{ complete: t("最终统计"), contradictory: t("统计不自洽，费用未知"), partial: t("仅部分统计"), missing: t("未提供"), unknown: t("未知") }[e.usageStatus] ?? e.usageStatus}</dd>
      <dt>{t("输入 / 输出 / 推理")}</dt><dd>{number(e.inputTokens)} / {number(e.outputTokens)} / {number(e.reasoningTokens)}</dd>
      <dt>{t("总计 / 缓存读 / 写")}</dt><dd>{number(e.totalTokens)} / {number(e.cacheReadTokens)} / {number(e.cacheCreationTokens)}</dd>
      <dt>{t("近期窗口")}</dt><dd>{e.rollingObservedCount} {t("条 ·")} {e.routeChangeCount} {t("次未确认路由变化")}{e.repeatedAnomaly ? t(" · 重复异常") : ""}</dd>
      <dt>{t("收费复核")}</dt><dd>{PROBE_LABELS[e.activeProbeState] ?? e.activeProbeState} {t("· 用量")} {number(e.activeProbeTokens)}</dd>
      {e.responseId && <><dt>{t("响应 ID")}</dt><dd><code>{e.responseId}</code></dd></>}
      {e.providerRequestId && <><dt>{t("请求 ID")}</dt><dd><code>{e.providerRequestId}</code></dd></>}
    </dl>
    <ul className="verification-codes">{e.evidenceCodes.map(code => <li key={code}><code>{code}</code> · {code === "CLAIM_MATCH" && !complete ? t("历史匹配标记已失效，以当前检测结论为准") : CODE_LABELS[code] ?? t("保留的诊断线索")}</li>)}</ul>
    <small className="muted">{t("证据来源：不可信上游响应元数据。仅对本条响应有效；声明匹配不是模型身份认证。")}</small>
  </div>;
}
