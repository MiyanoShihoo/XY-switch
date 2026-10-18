// Synthetic UI fixture only; no requests are sent to an upstream service.
import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import '../../src/design/index.css';
import type { AuditRecord, PassiveAuditGroup, PassiveAuditReport, ShadowAuditReport, ShadowAuditSetting, ShadowAuditStatus } from '../../src/types/model-audit';

const handoffQa = new URLSearchParams(location.search).has('handoff');

const observation = { requestedModel: 'premium-model', forwardedModel: 'premium-model', upstreamModel: 'premium-model', inputTokens: 10, outputTokens: 8, totalTokens: 18, reasoningTokens: 0, maxTokens: 32, visibleOutputBytes: 24, toolPayloadBytes: 0, complete: true, textOnly: true, stream: null };
const audit = { observation, flags: [], gaps: ['scv.no_stream'], modelStatus: 'matched', axes: { digit: 'not_comparable', han: 'not_comparable', space: 'not_comparable' } };
const hanBytes = new TextEncoder().encode('汉'.repeat(12)).byteLength;
const hanAudit: AuditRecord = {
  observation: { ...observation, outputTokens: 6, totalTokens: 16, visibleOutputBytes: hanBytes,
    stream: { textDeltaCount: 12, visibleOutputChars: 12, maxDigitRun: 0, maxHanRun: 1, maxSpaceRun: 0, medianDeltaChars: 1, ttfbMs: 42, digitChars: 0, hanChars: 12, spaceChars: 0, arrivalOffsetsMs: [], batched: false, minimumSamplesMet: true, samplesTruncated: false },
  }, flags: [], gaps: ['scv.token_boundaries_unverified'], modelStatus: 'matched',
  axes: { digit: 'not_comparable', han: 'not_comparable', space: 'not_comparable' },
  streamBilling: { status: 'contradiction', textFrames: 12, visibleTokens: 6, visibleBytes: hanBytes, tokensPerByte: 6 / hanBytes, bytesPerFrame: hanBytes / 12, framesPerToken: 2, gaps: [], limitations: ['合成汉字样本用于验证 UTF-8 字节单位展示。'] },
};
const entries = [hanAudit, { ...audit, modelStatus: 'mismatched', flags: ['inv.model_mismatch', 'inv.byte_bound'], observation: { ...observation, upstreamModel: 'cheap-model-v0', outputTokens: 40, totalTokens: 50, visibleOutputBytes: 2 } }, { ...audit, modelStatus: 'unknown', gaps: ['model.upstream_missing', 'byte_bound.reasoning_unknown', 'scv.no_stream'], observation: { ...observation, upstreamModel: null, reasoningTokens: null } }].map((value, index) => ({
  requestId: String(index), providerId: 'fixture', appType: 'codex', model: value.observation.upstreamModel || 'premium-model', inputTokens: 10, outputTokens: value.observation.outputTokens,
  cacheReadTokens: 0, cacheCreationTokens: 0, totalCostUsd: '0.0001', inputCostUsd: '0.0001', outputCostUsd: '0', cacheReadCostUsd: '0', cacheCreationCostUsd: '0', costMultiplier: '1', isStreaming: value.observation.stream != null, latencyMs: 42, statusCode: handoffQa && index === 1 ? 503 : 200, createdAt: Date.now() - index * 1000, audit: value,
}));
const group: PassiveAuditGroup = { providerId: 'fixture', requestedModel: 'premium-model', windowStart: Date.UTC(2026, 9, 9), composition: 'digit:0/4 han:4/4 space:0/4 chars:<64', samples: 40, comparableSamples: 40, billingSamples: 25, meanTokensPerByte: 1 / 6, meanBytesPerFrame: 3, meanDeltaChars: 1, meanTtfbMs: 42, meanDigitRun: 0, meanHanRun: 1, meanSpaceRun: 0 };
const report: PassiveAuditReport = {
  sampleLimit: 10000, sampleCount: 120, truncated: false, auditedRequests: 120, evidenceRequests: 1, modelMismatchRequests: 1, gapRequests: 3, untestedRequests: 0,
  groups: [group, { ...group, windowStart: Date.UTC(2026, 9, 10), meanTokensPerByte: 1 / 3, meanBytesPerFrame: 6 }, { ...group, providerId: 'fixture-other', windowStart: Date.UTC(2026, 9, 10), billingSamples: 8, meanTokensPerByte: null, meanBytesPerFrame: null }],
  comparisons: [
    { kind: 'history', left: 'fixture @ 2026-10-09', right: 'fixture @ 2026-10-10', requestedModel: 'premium-model', composition: group.composition, status: 'similar', samplesLeft: 40, samplesRight: 40, deltaRatio: 1, ttfbRatio: 1, billingSamplesLeft: 25, billingSamplesRight: 25, billingStatus: 'changed', tokenByteRatio: 2, frameByteRatio: 2 },
    { kind: 'peer', left: 'fixture @ 2026-10-10', right: 'fixture-other @ 2026-10-10', requestedModel: 'premium-model', composition: group.composition, status: 'similar', samplesLeft: 40, samplesRight: 40, deltaRatio: 1, ttfbRatio: 1, billingSamplesLeft: 25, billingSamplesRight: 8, billingStatus: 'insufficient', tokenByteRatio: null, frameByteRatio: null },
  ], notes: ['界面验收使用合成样本；没有向任何上游发请求。'],
};
type ShadowScenario = 'empty' | 'consistent' | 'suspicious' | 'insufficient';
const fixture = {
  calls: [] as { command: string; args: Record<string, unknown> | undefined }[],
  settings: [] as ShadowAuditStatus[],
  scenario: 'empty' as ShadowScenario,
  deferAnalysis: false,
  pendingAnalysis: null as (() => void) | null,
  backendReport: null as ShadowAuditReport | null,
  setScenario(value: ShadowScenario) { this.scenario = value; },
  releaseAnalysis() { this.pendingAnalysis?.(); this.pendingAnalysis = null; },
};
function shadowReport(): ShadowAuditReport {
  const { scenario } = fixture;
  const insufficient = scenario === 'insufficient';
  const empty = scenario === 'empty';
  const sampleCount = empty ? 0 : insufficient ? 2 : scenario === 'consistent' ? 5 : 40;
  const slope = insufficient ? null : scenario === 'suspicious' ? 2 : 1;
  return {
    sampleLimit: 10000, sampleCount, pairCount: sampleCount, baselineCount: 0,
    excludedCount: 0, truncated: false, droppedRequests: 0,
    groups: empty ? [] : [{ providerId: 'fixture', model: 'premium-model',
      tokenizerSha256: 'a'.repeat(64), partitionId: 'synthetic-partition',
      regression: { status: insufficient ? 'untested' : scenario === 'suspicious' ? 'suspicious' : 'consistent',
        sampleCount, minimumSamples: 3, slope, intercept: insufficient ? null : 6,
        slopeCiLow: slope === null ? null : slope - 0.01, slopeCiHigh: slope === null ? null : slope + 0.01,
        residualRmse: insufficient ? null : 0.4, xSpan: insufficient ? 8 : 128,
        interceptDrift: insufficient ? null : 0.1,
        gaps: insufficient ? ['shadow.insufficient_pairs'] : [],
        limitations: ['界面验收使用合成数据；回归计算另由后端测试验证。'],
      },
    }],
    reasons: insufficient ? [{ reason: 'shadow.insufficient_pairs', count: 2 }] : [],
    notes: ['界面验收仅测试结构化 IPC 响应的显示，不调用真实供应商。'],
  };
}
Object.assign(window, { __AUDIT_QA__: fixture, __TAURI_INTERNALS__: { invoke: async (command: string, args?: Record<string, unknown>) => {
  fixture.calls.push({ command, args: args && structuredClone(args) });
  if (command === 'get_usage_logs') return entries;
  if (command === 'get_usage_summary') return { totalRequests:3, successCount:3, totalInputTokens:30, totalOutputTokens:54, totalCacheReadTokens:0, totalCacheCreationTokens:0, totalCostUsd:'0.0003', avgLatencyMs:42 };
  if (command === 'analyze_model_audit') return report;
  if (command === 'get_shadow_audit_settings') return structuredClone(fixture.settings);
  if (command === 'save_shadow_audit_setting') {
    const request = args?.request as ShadowAuditSetting;
    const saved: ShadowAuditStatus = { ...request, sourceSha256: request.enabled ? 'a'.repeat(64) : null, error: null, droppedRequests: 0 };
    fixture.settings = [...fixture.settings.filter((entry) => entry.providerId !== saved.providerId || entry.model !== saved.model), saved];
    return structuredClone(saved);
  }
  if (command === 'analyze_shadow_audit') {
    const result = fixture.backendReport ?? shadowReport();
    if (fixture.deferAnalysis) {
      fixture.deferAnalysis = false;
      await new Promise<void>((resolve) => { fixture.pendingAnalysis = resolve; });
    }
    return result;
  }
  if (['get_usage_trends','get_provider_stats','get_model_stats'].includes(command)) return [];
  throw new Error(`Unexpected fixture command: ${command}`);
} } });
const { UsagePage } = await import('../../src/features/usage/UsagePage');
const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
createRoot(document.getElementById('root')!).render(<main style={{ width: '100%', height: '100vh', overflow: 'auto' }}>
  {handoffQa && <section id="handoff-palette" style={{ padding: '18px 28px', display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }} aria-label="合成样式验收图例">
    <span className="audit-badge audit-unknown">无法判断</span>
    <span className="audit-badge audit-untested">未测到</span>
    <span className="audit-badge audit-mismatched">自报不一致</span>
    <span className="audit-badge audit-evidence">硬证据 / 错误</span>
    <span className="audit-badge audit-clear">已测无异常</span>
    <span className="audit-badge audit-statistical">统计偏离</span>
    <span className="ui-inline-error">表单错误状态</span>
  </section>}
  <QueryClientProvider client={queryClient}><UsagePage providers={[]} /></QueryClientProvider>
</main>);
