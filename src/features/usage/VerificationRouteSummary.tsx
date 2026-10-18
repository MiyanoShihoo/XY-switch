
import type { VerificationEvidence } from "../../types/model-verification";
import { summarizeVerificationRoute } from "../../lib/model-judgment";
import { Badge } from "../../ui";

export function VerificationRouteSummary({ recent, providerId, model }: { recent: VerificationEvidence[]; providerId: string; model: string }) {
  const summary = summarizeVerificationRoute(recent, { providerId, requestedModel: model });
  return <section className="verification-evidence" aria-label="当前路由判定摘要">
    <div className="inline-actions"><strong>当前路由检测结论</strong><Badge tone={summary.tone}>{summary.label}</Badge></div>
    <dl className="key-values verification-evidence__values">
      <dt>当前配置样本</dt><dd>{summary.samples} 条 · 被动 {summary.passiveSamples} / 收费复核 {summary.activeSamples}</dd>
      <dt>成功完整 / 失败 / 不完整</dt><dd>{summary.completed} / {summary.failed} / {summary.incomplete}</dd>
      <dt>声明一致 / 需核对异常</dt><dd>{summary.consistentClaims} / {summary.anomalies}</dd>
      <dt>缺少可比型号声明</dt><dd>{summary.missingClaims} 条</dd>
      <dt>成功响应声明</dt><dd><code>{summary.reportedModels.join(" / ") || "尚无可比响应"}</code></dd>
      <dt>已知报告用量 / 未知用量请求</dt><dd>{summary.knownUsageSamples === 0 ? "暂无完整报告" : summary.knownReportedTokens == null ? "超出安全统计范围" : summary.knownReportedTokens + " Token"} / {summary.unknownUsageSamples} 条</dd>
    </dl>
    <p className="page-description">只汇总当前凭据和配置的响应，去重后计数；错误响应不参与型号变化基线。成功样本不覆盖之前的失败，未知用量不计成 0。</p>
    <small className="muted">这些是接口与声明检测结果，不是权重认证。尚未进行相同输入的重复采样，不能据此声称长期稳定或输出确定。</small>
  </section>;
}
