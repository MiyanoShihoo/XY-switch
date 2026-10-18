import { Button, Badge } from "../../ui";
import { useState } from "react";
import type { AuditRecord } from "../../types/model-audit";
import { AUDIT_FLAG_LABELS, HARD_AUDIT_FLAGS, MODEL_STATUS_LABELS, auditEvidenceState, auditGapLabel, byteBoundExplanation, modelAuditStatus, reportedResponseModels } from "../../lib/model-audit";
import { StreamBillingDetails } from "./StreamBillingDetails";

export function ModelAuditCell({ audit, legacyModel }: { audit?: AuditRecord | null; legacyModel: string }) {
  const [showStream, setShowStream] = useState(false);
  const modelStatus = modelAuditStatus(audit);
  const evidenceState = auditEvidenceState(audit);
  const observation = audit?.observation;
  const stream = observation?.stream;
  const flagCount = audit?.flags.filter((flag) => HARD_AUDIT_FLAGS.has(flag)).length ?? 0;
  return (
    <div className="model-audit-cell">
      <div className="model-audit-models">
        <span>请求 <code>{observation?.requestedModel || "未记录"}</code></span>
        {observation?.forwardedModel && observation.forwardedModel !== observation.requestedModel && (
          <span>路由后 <code>{observation.forwardedModel}</code></span>
        )}
        <span>上游自报 <code>{reportedResponseModels(audit).join(" → ") || "未自报 / 未记录"}</code></span>
      </div>
      <Badge tone={modelStatus === "mismatched" || modelStatus === "conflicting" ? "warning" : "neutral"}>{MODEL_STATUS_LABELS[modelStatus]}</Badge>
      <details className="model-audit-details">
        <summary className={`audit-detail-summary audit-${evidenceState}`}>
          {flagCount > 0 ? `${flagCount} 项硬证据 · 查看详情` : evidenceState === "untested" ? "存在未测项 · 查看详情" : evidenceState === "evidence" ? "自报差异 · 查看详情" : "已测项未见异常 · 查看详情"}
        </summary>
        {!audit ? (
          <p>该记录没有审计观测，无法判断；这不等于没问题。历史计费模型「{legacyModel || "未记录"}」不能作为上游自报证据。</p>
        ) : (
          <>
            <p>模型字段可以被上游改写。一致不能证明真实模型身份；不一致仅证明名称差异。优先对照本次实际发出的模型（含映射与覆盖），不套用供应商的历史结论。</p>
            {audit.flags.length > 0 && <ul className="audit-evidence-list">{audit.flags.map((flag) => (
              <li key={flag}>
                <strong>{AUDIT_FLAG_LABELS[flag] ?? flag}</strong> <code>{flag}</code>
                {flag === "inv.byte_bound" && <p>{byteBoundExplanation(audit)}扣除推理后仍超出字节上界。</p>}
                {flag === "inv.usage_selfcontradictory" && <p>输入 {observation?.inputTokens ?? "未知"} + 输出 {observation?.outputTokens ?? "未知"} ≠ 上报总数 {observation?.totalTokens ?? "未知"}。</p>}
                {flag === "inv.max_tokens_violated" && <p>输出 {observation?.outputTokens ?? "未知"} token，超过请求上限 {observation?.maxTokens ?? "未知"} token。</p>}
                {flag === "inv.model_mismatch" && <p>上游自报未匹配本次实际发送的模型；该信号不确认上游模型身份。</p>}
              </li>
            ))}</ul>}
            {audit.gaps.length > 0 ? (
              <div className="audit-untested audit-gap-box">
                <strong>未测到 / 未排除</strong>
                <ul>{audit.gaps.map((gap) => <li key={gap}>{auditGapLabel(gap)} <code>{gap}</code></li>)}</ul>
                <p>这些缺口不等于没问题；已测项的结果只覆盖可观测部分。</p>
              </div>
            ) : audit.flags.length === 0 && <p className="audit-clear">已测项未见异常，不构成模型身份或渠道可靠性的背书。</p>}
            {stream && (
              <div className="audit-stream-detail">
                <p>采集 {stream.textDeltaCount} 个文本增量；首字 {stream.ttfbMs == null ? "未测到" : `${stream.ttfbMs} ms`}。
                  {stream.samplesTruncated && " 到达时刻样本已截断。"}</p>
                <Button type="button" variant="secondary" size="sm" onClick={() => setShowStream(!showStream)} aria-expanded={showStream}>
                  {showStream ? "收起流式分析" : "分析此响应流式统计"}
                </Button>
                {showStream && (stream.batched || !stream.minimumSamplesMet ? (
                  <p className="audit-untested">{stream.batched ? "检测到帧批处理，全部切分结论不可用。" : "帧样本不足，全部切分结论不可用。"}这不等于没问题。</p>
                ) : (
                  <>
                    <dl className="audit-stream-metrics">
                      <div><dt>帧字符数中位数</dt><dd>{stream.medianDeltaChars.toFixed(2)}</dd></div>
                      {(["digit", "han", "space"] as const).map((axis) => (
                        <div key={axis}><dt>{{ digit: "数字轴", han: "汉字轴", space: "空白轴" }[axis]}</dt><dd>{axisLabel(audit.axes?.[axis])}</dd></div>
                      ))}
                      <div><dt>帧内最长串：数字 / 汉字 / 空白</dt><dd>{stream.maxDigitRun} / {stream.maxHanRun} / {stream.maxSpaceRun}</dd></div>
                    </dl>
                    <p>切分和时延只提供统计线索。SSE 边界不保证等同于 token 边界；网络、缓冲、文本构成与计费基线仍可能影响结果。「一致」不是身份确认。</p>
                  </>
                ))}
                {showStream && <StreamBillingDetails billing={audit.streamBilling} />}
              </div>
            )}
          </>
        )}
      </details>
    </div>
  );
}

function axisLabel(status: string | undefined): string {
  if (status === "contradiction") return "存在统计矛盾，需核查";
  if (status === "consistent") return "可比指标一致";
  return "不可比 / 未测到";
}
