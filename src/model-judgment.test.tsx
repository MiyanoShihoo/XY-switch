import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { judgeResponseModel, judgeVerificationEvidence, summarizeVerificationRoute } from "./lib/model-judgment";
import { responseModelEvidence } from "./lib/response-model";
import { modelAuditStatus } from "./lib/model-audit";
import { eligibleEvidence } from "./lib/model-verification";
import { VerificationDetails } from "./features/usage/VerificationDetails";
import { VerificationRouteSummary } from "./features/usage/VerificationRouteSummary";
import { ResponseModelEvidence } from "./features/usage/ResponseModelEvidence";
import type { AuditRecord } from "./types/model-audit";
import type { VerificationEvidence } from "./types/model-verification";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
function evidence(change: Partial<VerificationEvidence> = {}): VerificationEvidence {
  return { observationId: "1", upstreamId: "p", requestedModel: "expected", forwardedModel: "expected", reportedModels: ["expected"],
    credentialFingerprint: "PRIVATE_FINGERPRINT", contextKey: "PRIVATE_CONTEXT", protocol: "chat", protocolShape: "chat",
    claimConsistency: "MATCH", identityAssurance: "UNVERIFIED", evidenceCodes: ["CLAIM_MATCH"], explanation: "Fixture", riskLevel: "none",
    observedAt: 1000, usageStatus: "complete", inputTokens: 10, outputTokens: 4, reasoningTokens: 0, totalTokens: 14,
    cacheReadTokens: 0, cacheCreationTokens: 0, isStreaming: false, streamCompleted: true, httpStatus: 200, finishReason: "stop",
    responseId: null, providerRequestId: null, evidenceSource: "untrusted_response_metadata", source: "passive", rollingObservedCount: 1,
    routeChangeCount: 0, repeatedAnomaly: false, newEvidence: true, activeProbeState: "disabled", activeProbeTokens: 0, ...change };
}
function audit(change: Partial<AuditRecord> = {}): AuditRecord {
  return { observation: { requestedModel: "expected", forwardedModel: "expected", upstreamModel: "expected", complete: true, textOnly: true },
    flags: [], gaps: [], modelStatus: "matched", axes: { digit: "not_comparable", han: "not_comparable", space: "not_comparable" }, ...change };
}
const scope = { providerId: "p", requestedModel: "expected" };

describe("响应判定有明确分支，不把所有结果压成未证实", () => {
  it("正常完整响应给出名称一致的可用结论，权重认证仍是另一条轴", () => {
    expect(judgeResponseModel(audit())).toMatchObject({ kind: "claim_consistent", weightIdentity: "unverified", declaredModels: ["expected"] });
    expect(responseModelEvidence(audit()).verifiedModel).toBeNull();
  });
  it.each([400, 401, 429, 500, 503])("旧记录即使写着 MATCH，HTTP %i 也不能显示成型号匹配", code => {
    const e = evidence({ httpStatus: code });
    expect(judgeVerificationEvidence(e).kind).toBe("transport_failure");
    expect(modelAuditStatus(audit({ verification: e }))).toBe("unknown");
  });
  it.each<Partial<VerificationEvidence>>([{ httpStatus: null }, { streamCompleted: false }, { evidenceCodes: ["RESPONSE_INVALID"] }, { evidenceCodes: ["STREAM_INCOMPLETE"] }])("未知和不完整响应不给出一致结论 %j", change => {
    expect(judgeVerificationEvidence(evidence(change)).kind).toBe("incomplete");
  });
  it.each([
    ["MODEL_DECLARATIONS_CONFLICT", "claim_conflict"], ["MODEL_CLAIM_MISMATCH", "claim_mismatch"],
    ["MODEL_ROUTE_CHANGED", "route_changed"], ["PROTOCOL_SHAPE_CHANGED", "protocol_changed"],
    ["USAGE_CONTRADICTORY", "usage_anomaly"], ["OUTPUT_LIMIT_EXCEEDED", "usage_anomaly"], ["USAGE_BOUND_CONTRADICTION", "usage_anomaly"],
  ])("%s 有具体结论而不是伪造真/假权重", (code, kind) => {
    const result = judgeVerificationEvidence(evidence({ evidenceCodes: [code] }));
    expect(result.kind).toBe(kind); expect(result.nextStep.length).toBeGreaterThan(10); expect(result.weightIdentity).toBe("unverified");
  });
  it("同包多个声明即使缓存说 MATCH 也不能通过", () => {
    expect(judgeVerificationEvidence(evidence({ reportedModels: ["expected", "other"] })).kind).toBe("claim_conflict");
  });
  it("明确允许的别名和回退不被误判为替换", () => {
    expect(judgeVerificationEvidence(evidence({ reportedModels: ["confirmed-alias"], evidenceCodes: ["CLAIM_MATCH", "ALLOWED_ROUTE_CHANGE"] })).kind).toBe("claim_consistent");
  });
  it("没有声明、不完整观察和本地会话分别处理", () => {
    expect(judgeVerificationEvidence(evidence({ reportedModels: [], claimConsistency: "MISSING" })).kind).toBe("incomplete");
    expect(judgeResponseModel(audit({ observation: { complete: true, textOnly: true, modelEvidenceTruncated: true } })).kind).toBe("incomplete");
    expect(judgeResponseModel(audit(), true)).toMatchObject({ kind: "no_evidence", declaredModels: [] });
    expect(judgeResponseModel().kind).toBe("no_evidence");
  });
  it("旧审计上限异常也进入判断，推理不重复当成身份依据", () => {
    expect(judgeResponseModel(audit({ flags: ["inv.max_tokens_violated"] })).kind).toBe("usage_anomaly");
    expect(judgeResponseModel(audit({ flags: ["inv.usage_selfcontradictory"] })).kind).toBe("usage_anomaly");
  });
  it("忽略模型自称、任意 actual_model 和 confidence 字段", () => {
    const forged = { ...audit(), actual_model: "different", verifiedModel: "different", confidence: 1, fingerprint: "trusted", selfDescription: "I am official" };
    expect(judgeResponseModel(forged)).toMatchObject({ weightIdentity: "unverified", declaredModels: ["expected"] });
  });
});

describe("当前路由聚合与失败保留", () => {
  it("去重并隔离其他凭据上下文、供应商、模型，输入数组不被排序修改", () => {
    const rows = [evidence(), evidence(), evidence({ observationId: "2", contextKey: "OLD", observedAt: 500 }),
      evidence({ observationId: "3", upstreamId: "other" }), evidence({ observationId: "4", requestedModel: "other" })];
    const before = JSON.stringify(rows); const result = summarizeVerificationRoute(rows, scope);
    expect(result).toMatchObject({ samples: 1, completed: 1, consistentClaims: 1, knownReportedTokens: 14, unknownUsageSamples: 0 });
    expect(JSON.stringify(rows)).toBe(before);
  });
  it("后续成功不抹掉同一配置中的超时及未知用量", () => {
    const result = summarizeVerificationRoute([evidence({ observationId: "ok" }), evidence({ observationId: "timeout", observedAt: 900,
      httpStatus: null, streamCompleted: false, claimConsistency: "UNKNOWN", reportedModels: [], usageStatus: "unknown", totalTokens: null })], scope);
    expect(result).toMatchObject({ samples: 2, completed: 1, incomplete: 1, knownReportedTokens: 14, unknownUsageSamples: 1, determinism: "not_tested" });
    expect(result.label).toBe("可用性仍有缺口");
  });
  it("HTTP 错误中的型号不加入成功声明集合", () => {
    const result = summarizeVerificationRoute([evidence(), evidence({ observationId: "bad", httpStatus: 503, reportedModels: ["other"] })], scope);
    expect(result.failed).toBe(1); expect(result.reportedModels).toEqual(["expected"]); expect(result.consistentClaims).toBe(1);
  });
  it("主动样本与被动样本明确计数，不把样本数当成身份置信度", () => {
    const result = summarizeVerificationRoute([evidence(), evidence({ observationId: "active", source: "active" })], scope);
    expect(result).toMatchObject({ samples: 2, passiveSamples: 1, activeSamples: 1, weightIdentity: "unverified", determinism: "not_tested" });
    expect(result).not.toHaveProperty("confidence");
  });
  it("不自洽和不安全整数的用量保留为未知", () => {
    const result = summarizeVerificationRoute([evidence({ totalTokens: 999, usageStatus: "contradictory" }), evidence({ observationId: "huge", totalTokens: Number.MAX_SAFE_INTEGER + 1 })], scope);
    expect(result).toMatchObject({ knownReportedTokens: 0, unknownUsageSamples: 2 });
  });
});

describe("判定呈现不产生 API 副作用", () => {
  it("页面同时显示可判定结论与独立的身份状态", () => {
    const html = renderToStaticMarkup(createElement(ResponseModelEvidence, { audit: audit() }));
    expect(html).toContain('data-judgment="claim_consistent"'); expect(html).toContain("型号声明一致（非权重认证）");
    expect(html).toContain("未证实"); expect(html).toContain("下一步"); expect(invokeMock).not.toHaveBeenCalled();
  });
  it("路由摘要不泄露凭据指纹或上下文哈希，不声称稳定或确定已认证", () => {
    const html = renderToStaticMarkup(createElement(VerificationRouteSummary, { recent: [evidence()], providerId: "p", model: "expected" }));
    expect(html).toContain("当前路由检测结论"); expect(html).toContain("不是权重认证"); expect(html).toContain("尚未进行相同输入的重复采样");
    expect(html).not.toContain("PRIVATE_"); expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("旧记录的完整性保护与摘要边界", () => {
  it.each(["RESPONSE_INVALID", "RESPONSE_INCOMPLETE", "STREAM_INCOMPLETE", "MODEL_EVIDENCE_TRUNCATED", "HTTP_STATUS_UNKNOWN"])("历史无效记录不会重新进入收费复核：%s", code => {
    const e = evidence({ evidenceCodes: ["MODEL_CLAIM_MISMATCH", code] });
    expect(eligibleEvidence(e)).toBe(false);
    expect(modelAuditStatus(audit({ verification: e }))).toBe("unknown");
    expect(summarizeVerificationRoute([e], scope)).toMatchObject({ completed: 0, incomplete: 1, consistentClaims: 0 });
  });
  it.each([{ httpStatus: 500 }, { streamCompleted: false }, { evidenceCodes: ["RESPONSE_INVALID"] }])("详细视图不显示无效旧 MATCH 的匹配徽标 %j", change => {
    const html = renderToStaticMarkup(createElement(VerificationDetails, { evidence: evidence(change) }));
    expect(html).not.toMatch(/<span\b[^>]*>声明匹配<\/span>/);
    expect(html).toContain("声明证据不足");
  });
  it("HTTP 成功但无型号声明时，给出证据不足而非声明一致", () => {
    const summary = summarizeVerificationRoute([evidence({ reportedModels: [], claimConsistency: "MISSING" })], scope);
    expect(summary).toMatchObject({ completed: 1, consistentClaims: 0, missingClaims: 1, label: "型号声明证据不足" });
  });
  it("统计状态错误地标为完整也不能累加矛盾用量", () => {
    expect(summarizeVerificationRoute([evidence({ totalTokens: 999 })], scope)).toMatchObject({ knownUsageSamples: 0, unknownUsageSamples: 1 });
  });
  it("全部用量未知时显示暂无完整报告，而不是报告为零", () => {
    const html = renderToStaticMarkup(createElement(VerificationRouteSummary, { recent: [evidence({ totalTokens: null, usageStatus: "unknown" })], providerId: "p", model: "expected" }));
    expect(html).toContain("暂无完整报告"); expect(html).not.toContain("0 Token");
  });
});
