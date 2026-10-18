import type { AuditRecord } from "../../types/model-audit";
import { responseModelEvidence } from "../../lib/response-model";
import { Badge } from "../../ui";
import { VerificationDetails } from "./VerificationDetails";
import "./response-model.css";

export function ResponseModelEvidence({ audit, local = false, compact = false }: { audit?: AuditRecord | null; local?: boolean; compact?: boolean }) {
  const evidence = responseModelEvidence(audit, local);
  return <div className="response-model-evidence" data-compact={compact || undefined}>
    <div className="response-model-evidence__line" data-judgment={evidence.judgment.kind}><span className="response-model-evidence__label">检测结论</span><Badge tone={evidence.judgment.tone}>{evidence.judgment.label}</Badge></div>
    <div className="response-model-evidence__line"><span className="response-model-evidence__label">上游声明</span><code title={evidence.declaredModels.join(" → ")}>{evidence.declaredModels.join(" → ") || (local ? "未采集" : "未返回 / 未记录")}</code></div>
    <div className="response-model-evidence__line"><span className="response-model-evidence__label">真实响应模型</span><Badge tone="warning">{evidence.verifiedModel ?? evidence.identityLabel}</Badge></div>
    <small className="response-model-evidence__comparison" data-warning={evidence.status === "mismatched" || evidence.status === "conflicting" || undefined}>{evidence.comparison}{evidence.partial ? " · 响应未完成" : ""}</small>
    {!compact && <><p className="page-description">{evidence.judgment.summary}</p><p className="page-description">下一步：{evidence.judgment.nextStep}</p><p className="page-description">{evidence.identityReason}本次结果不代表同供应商的其他请求。</p></>}
    {!compact && !local && audit?.verification && <VerificationDetails evidence={audit.verification} />}
  </div>;
}
