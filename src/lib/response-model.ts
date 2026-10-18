import type { AuditRecord } from "../types/model-audit";
import { judgeResponseModel } from "./model-judgment";
import { modelAuditStatus, reportedResponseModels } from "./model-audit";

/** No response name, self-description or statistical fingerprint proves the weights used. */
export function responseModelEvidence(audit?: AuditRecord | null, local = false) {
  const status = local ? "unknown" : modelAuditStatus(audit);
  const observation = audit?.observation;
  const declaredModels = local ? [] : reportedResponseModels(audit);
  const mapped = Boolean(observation?.forwardedModel && observation.requestedModel && observation.forwardedModel !== observation.requestedModel);
  const comparison = local ? "仅本地会话记录" : {
    matched: mapped ? "映射后名称一致" : "本次名称一致",
    mismatched: "本次名称不一致",
    conflicting: "单次响应声明冲突",
    unknown: observation?.modelEvidenceTruncated ? "证据已截断，无法判断" : "缺少可比模型证据",
  }[status];
  return {
    status, declaredModels, comparison,
    judgment: judgeResponseModel(audit, local),
    partial: !local && Boolean(audit) && !observation?.complete,
    // Deliberately not copied from any upstream-controlled field. A verifier with
    // independent trusted identity evidence is required before this can be a name.
    verifiedModel: null,
    identityStatus: !local && audit?.verification?.identityAssurance === "INCONCLUSIVE" ? "inconclusive" as const : "unverified" as const,
    identityLabel: !local && audit?.verification?.identityAssurance === "INCONCLUSIVE" ? "证据不足" : "未证实",
    identityReason: local
      ? "本地会话模型是客户端记录，不包含原始上游响应，不能证实真实身份。"
      : "本条响应没有独立、可验证的模型身份证据；名称、模型自述、用量或流式指纹均不能单独证实真实模型。",
  };
}
