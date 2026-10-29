import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelAuditCell } from "./features/usage/ModelAuditCell";
import { ModelAuditPanel, PassiveReport } from "./features/usage/ModelAuditPanel";
import { auditEvidenceState, byteBoundExplanation, modelAuditStatus, normalizeReferenceReport } from "./lib/model-audit";
import { analyzeModelAudit, analyzeModelReference, generateModelReference, loadModelReference } from "./lib/model-audit-service";
import type { AuditRecord, ModelFingerprint, PassiveAuditComparison, PassiveAuditGroup, PassiveAuditReport } from "./types/model-audit";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const honest: AuditRecord = {
  observation: { requestedModel: "requested", forwardedModel: "requested", upstreamModel: "requested", inputTokens: 5, outputTokens: 20, totalTokens: 25, reasoningTokens: 10, maxTokens: 50, visibleOutputBytes: 12, toolPayloadBytes: 0, complete: true, textOnly: true },
  flags: [], gaps: [], modelStatus: "matched",
  axes: { digit: "not_comparable", han: "not_comparable", space: "not_comparable" },
};

describe("审计模型自报与缺口", () => {
  it("保留一致、不一致、无法判断三个状态", () => {
    expect(modelAuditStatus(honest)).toBe("matched");
    expect(modelAuditStatus({ ...honest, modelStatus: "mismatched", observation: { ...honest.observation, upstreamModel: "different" } })).toBe("mismatched");
    expect(modelAuditStatus({ ...honest, observation: { ...honest.observation, upstreamModel: null } })).toBe("unknown");
    expect(modelAuditStatus({ ...honest, observation: { ...honest.observation, requestedModel: "" } })).toBe("matched");
    expect(modelAuditStatus({ ...honest, observation: { ...honest.observation, requestedModel: "", forwardedModel: "" } })).toBe("unknown");
    expect(modelAuditStatus(null)).toBe("unknown");
  });

  it("旧计费模型不能替代上游自报，未测到与已测无异常使用不同展示", () => {
    const unknown = renderToStaticMarkup(createElement(ModelAuditCell, { legacyModel: "requested" }));
    const clear = renderToStaticMarkup(createElement(ModelAuditCell, { legacyModel: "requested", audit: honest }));
    expect(unknown).toContain("无法判断");
    expect(unknown).toContain("audit-untested");
    expect(unknown).toContain("这不等于没问题");
    expect(unknown).not.toContain("自报一致");
    expect(clear).toContain("自报一致");
    expect(clear).toContain("audit-clear");
    expect(clear).toContain("不能证明真实模型身份");
  });

  it("未扣除推理与中断响应不算检查正常，证据与缺口可共存", () => {
    expect(auditEvidenceState(honest)).toBe("clear");
    expect(auditEvidenceState({ ...honest, gaps: ["byte_bound.reasoning_unknown"] })).toBe("untested");
    expect(auditEvidenceState({ ...honest, observation: { ...honest.observation, complete: false } })).toBe("untested");
    const flagged = { ...honest, flags: ["inv.byte_bound"], gaps: ["usage.fields_missing"] };
    expect(auditEvidenceState(flagged)).toBe("evidence");
    const html = renderToStaticMarkup(createElement(ModelAuditCell, { legacyModel: "requested", audit: flagged }));
    expect(html).toContain("1 项硬证据");
    expect(html).toContain("缺少用量字段");
    expect(byteBoundExplanation(flagged)).toContain("推理 10 token");
    expect(byteBoundExplanation(flagged)).toContain("工具载荷 0 字节");
    expect(byteBoundExplanation(flagged)).toContain("特殊 token 余量 4");
  });

  it("页面呈现不自动触发分析或探针", () => {
    invokeMock.mockClear();
    const html = renderToStaticMarkup(createElement(ModelAuditPanel, { filter: {}, logs: [] }));
    expect(invokeMock).not.toHaveBeenCalled();
    expect(html).toContain("分析当前筛选范围");
    expect(html).toContain("不会自动发送探针");
    expect(html).not.toContain("可信度");
  });
});

describe("按语言分桶的整段计费比较", () => {
  const group: PassiveAuditGroup = { providerId: "p", requestedModel: "m", windowStart: 0, composition: "han", samples: 30, comparableSamples: 30, billingSamples: 24, meanTokensPerByte: 0.25, meanBytesPerFrame: 4, meanDeltaChars: 1, meanTtfbMs: 20, meanDigitRun: 0, meanHanRun: 1, meanSpaceRun: 0 };
  const comparison: PassiveAuditComparison = { kind: "history", left: "p @ earlier", right: "p @ later", requestedModel: "m", composition: "han", status: "similar", samplesLeft: 30, samplesRight: 30, deltaRatio: 1, ttfbRatio: 1, billingSamplesLeft: 24, billingSamplesRight: 25, billingStatus: "changed", tokenByteRatio: 2, frameByteRatio: 1.25 };
  const report: PassiveAuditReport = { sampleLimit: 10000, sampleCount: 60, truncated: false, auditedRequests: 60, evidenceRequests: 0, modelMismatchRequests: 0, gapRequests: 0, untestedRequests: 0, groups: [group], comparisons: [comparison], notes: [] };

  it("计费与切分结论独立显示，并保留 UTF-8 单位和各自样本量", () => {
    const html = renderToStaticMarkup(createElement(PassiveReport, { report }));
    expect(html).toContain("计费 24 / 25");
    expect(html).toContain("token / 字节 2.00×");
    expect(html).toContain("字节 / 帧 1.25×");
    expect(html).toContain("计费比率存在统计差异");
    expect(html).toContain('class="audit-clear">未见明显差异');
    expect(html).toContain("0.250 / 4.000");
    expect(html).toContain("两组各需至少 20 个合格计费样本");
    expect(html).toContain("跨渠道只比较模型名与语言构成分桶相同的组");
    expect(html).toContain("UTF-8 字节");
  });

  it("切分样本充足不能替代计费门槛，缺值或少于20份时不可比", () => {
    const missing = { ...comparison, kind: "peer" as const, billingSamplesRight: 30, billingStatus: "insufficient" as const, tokenByteRatio: null, frameByteRatio: null };
    const tooFew = { ...comparison, billingSamplesLeft: 19 };
    const html = renderToStaticMarkup(createElement(PassiveReport, { report: { ...report, comparisons: [tooFew, missing], groups: [{ ...group, billingSamples: 19 }] } }));
    expect(html.match(/不可比：计费样本不足或指标缺失/g)).toHaveLength(2);
    expect(html).not.toContain("token / 字节 2.00×");
    expect(html).not.toContain("计费比率存在统计差异");
    expect(html).toContain("计费样本不足 20");
    expect(html).toContain("不可比 / 不可比");
    expect(html).not.toContain("0.250 / 4.000");
  });
});

describe("参照结果完整性", () => {
  const ranked = { status: "ranked", candidates: [
    { model: "equivalent-a", probability: 0.25, comparable: 11, matched: 10, outside: false },
    { model: "outside", probability: 0.75, comparable: 0, matched: 0, outside: true },
  ], comparedGroups: ["digits"], limitations: ["词表等价不能确认模型"] };

  it("按概率排序同时完整保留库外项、组数与限制", () => {
    const report = normalizeReferenceReport(ranked);
    expect(report.candidates[0].outside).toBe(true);
    expect(report.candidates[0].probability).toBe(0.75);
    expect(report.candidates[1].matched).toBe(10);
    expect(report.limitations).toEqual(ranked.limitations);
    expect(ranked.candidates[0].model).toBe("equivalent-a");
  });

  it("拒绝丢失库外档、不完整概率和伪造的未测概率", () => {
    expect(() => normalizeReferenceReport({ ...ranked, candidates: [{ ...ranked.candidates[0], probability: 1 }] })).toThrow("库外");
    expect(() => normalizeReferenceReport({ ...ranked, candidates: ranked.candidates.map((c) => ({ ...c, probability: 0.2 })) })).toThrow("概率总和");
    expect(() => normalizeReferenceReport({ ...ranked, status: "untested" })).toThrow("未测到");
    expect(() => normalizeReferenceReport({ ...ranked, candidates: [{ ...ranked.candidates[0], probability: NaN }] })).toThrow("概率无效");
    expect(normalizeReferenceReport({ ...ranked, status: "untested", candidates: [] }).candidates).toEqual([]);
  });
});

describe("离线分析命令契约", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  });
  const fingerprint: ModelFingerprint = { schemaVersion: 1, corpusId: "corpus", model: "test", sourceSha256: "sha", tokenizerKind: "bpe", pretokenizer: "cl100k", authority: "tokenizer_file", groups: [], limitations: [] };

  it("把筛选与本地文件路径按 Tauri 参数名传递", async () => {
    await analyzeModelAudit({ startTimeMs: 10, providerId: "provider" });
    expect(invokeMock).toHaveBeenLastCalledWith("analyze_model_audit", { filter: { startTimeMs: 10, providerId: "provider" } });
    await loadModelReference("C:\\audit\\sample.json");
    expect(invokeMock).toHaveBeenLastCalledWith("load_model_reference", { path: "C:\\audit\\sample.json" });
    const request = { tokenizerPath: "C:\\audit\\vocab.tiktoken", model: "test", pretokenizer: "cl100k" as const };
    await generateModelReference(request);
    expect(invokeMock).toHaveBeenLastCalledWith("generate_model_reference", { request });
    const automatic = { tokenizerPath: "C:\\audit\\o200k_base.tiktoken", model: "test" };
    await generateModelReference(automatic);
    expect(invokeMock).toHaveBeenLastCalledWith("generate_model_reference", { request: automatic });
  });

  it("参照命令返回未测到时不制造概率", async () => {
    invokeMock.mockResolvedValue({ status: "untested", candidates: [], comparedGroups: [], limitations: ["样本不足"] });
    const request = { observation: fingerprint, candidates: [fingerprint] };
    const result = await analyzeModelReference(request);
    expect(invokeMock).toHaveBeenLastCalledWith("analyze_model_reference", { request });
    expect(result.status).toBe("untested");
    expect(result.candidates).toHaveLength(0);
  });
});
