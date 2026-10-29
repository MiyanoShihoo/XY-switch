import { translateMessage, useLanguage, t, localeTag } from "../../i18n/language";
import { Button, Field, Input, Select, Textarea } from "../../ui";
import { useEffect, useRef, useState } from "react";
import { analyzeModelAudit, analyzeModelReference, generateModelReference, loadModelReference } from "../../lib/model-audit-service";
import { HARD_AUDIT_FLAGS, modelAuditStatus } from "../../lib/model-audit";
import type { ModelFingerprint, PassiveAuditReport, Pretokenizer, ReferenceReport } from "../../types/model-audit";
import type { UsageFilter, UsageLogEntry } from "../../types/usage";
import "./audit.css";
import { ShadowAuditPanel } from "./ShadowAuditPanel";

export function ModelAuditPanel({ filter, logs }: { filter: UsageFilter; logs: UsageLogEntry[] }) {
  useLanguage();
  const [report, setReport] = useState<PassiveAuditReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  useEffect(() => {
    generation.current += 1;
    setReport(null);
    setError("");
    setBusy(false);
    return () => { generation.current += 1; };
  }, [filter]);

  const analyze = async () => {
    const requestGeneration = ++generation.current;
    setBusy(true);
    setError("");
    setReport(null);
    try {
      const result = await analyzeModelAudit(filter);
      if (generation.current === requestGeneration) setReport(result);
    } catch (cause) {
      if (generation.current === requestGeneration) setError(String(cause));
    } finally {
      if (generation.current === requestGeneration) setBusy(false);
    }
  };
  const evidence = logs.filter((log) => log.audit?.flags.some((flag) => HARD_AUDIT_FLAGS.has(flag))).length;
  const mismatches = logs.filter((log) => modelAuditStatus(log.audit) === "mismatched").length;
  const untested = logs.filter((log) => !log.audit).length;
  const gaps = logs.filter((log) => log.audit?.gaps.length).length;
  return (
    <section className="model-audit-panel" aria-labelledby="model-audit-title">
      <div className="audit-panel-heading">
        <div><h2 id="model-audit-title">{t("模型与用量审计")}</h2><p>{t("被动记录业务响应中的结构信息。模型名称来自上游自报，无法据此确认真实身份。")}</p></div>
        <Button type="button" variant="primary" onClick={() => void analyze()} disabled={busy}>{busy ? t("分析中…") : t("分析当前筛选范围")}</Button>
      </div>
      <div className="audit-counts" aria-label={t("当前页审计计数")}>
        <span>{t("当前页")} <strong>{logs.length}</strong> {t("次请求")}</span>
        <span className={evidence ? "audit-evidence" : ""}>{t("硬证据")} <strong>{evidence}</strong> {t("次")}</span>
        <span>{t("自报不一致")} <strong>{mismatches}</strong> {t("次")}</span>
        <span className="audit-untested">{t("未采集")} <strong>{untested}</strong> {t("次 · 有缺口")} <strong>{gaps}</strong> {t("次")}</span>
      </div>
      <p className="audit-note">{t("未采集或检查有缺口不等于没问题。计数可能重叠；自报差异与硬证据分开统计。分析由你手动触发。")}</p>
      {error && <p className="audit-error" role="alert">{t("分析未完成：")}{translateMessage(error)}</p>}
      {report && <PassiveReport report={report} />}
      <ShadowAuditPanel filter={filter} logs={logs} />
      <details className="audit-reference-section">
        <summary>{t("离线参照库分析")}</summary>
        <ReferenceAnalysis />
      </details>
    </section>
  );
}

export function PassiveReport({ report }: { report: PassiveAuditReport }) {
  useLanguage();
  const [showAll, setShowAll] = useState(false);
  const comparisons = showAll ? report.comparisons : report.comparisons.slice(0, 8);
  return (
    <div className="audit-passive-report" aria-live="polite">
      <h3>{t("筛选范围分析结果")}</h3>
      <p>{t("本次读取")} {report.sampleCount.toLocaleString(localeTag())} {t("次业务请求，已采集")} {report.auditedRequests.toLocaleString(localeTag())} {t("次；硬证据")} {report.evidenceRequests} {t("次，自报不一致")} {report.modelMismatchRequests} {t("次，存在缺口")} {report.gapRequests} {t("次，未采集")} {report.untestedRequests} {t("次。")}</p>
      {report.truncated && <p className="audit-untested">{t("结果仅覆盖最近")} {report.sampleLimit.toLocaleString(localeTag())} {t("次请求，已达到分析上限。")}</p>}
      {report.auditedRequests === 0 && <p className="audit-untested">{t("未测到：当前范围没有审计观测。这不等于没问题。")}</p>}
      <p>{t("优先比较同一渠道的不同时间窗；跨渠道只比较模型名与语言构成分桶相同的组。切分与时延每组至少需要 20 个完整、未批处理的可比响应；整段计费两组各需至少 20 个合格计费样本。差异不等于模型被替换。")}</p>
      <p className="audit-note">{t("计费比率统一使用 UTF-8 字节，排除推理用量未知、工具载荷、非文本或缺失字节记录。比值为两组均值的较大值 / 较小值，不表示涨跌方向；样本不足或指标缺失时显示不可比。")}</p>
      {comparisons.length > 0 ? (
        <div className="audit-table-scroll"><table className="audit-table">
          <thead><tr><th scope="col">{t("比较对象")}</th><th scope="col">{t("模型 / 构成")}</th><th scope="col">{t("两组有效样本")}</th><th scope="col">{t("切分与时延")}</th><th scope="col">{t("整段计费比较")}</th></tr></thead>
          <tbody>{comparisons.map((comparison, index) => {
            const billingComparable = comparison.billingSamplesLeft >= 20 && comparison.billingSamplesRight >= 20
              && comparison.tokenByteRatio != null && comparison.frameByteRatio != null && comparison.billingStatus !== "insufficient";
            return <tr key={`${comparison.kind}-${index}`}>
            <td><strong>{comparison.kind === "peer" ? t("跨渠道") : t("跨时间")}</strong><br />{comparison.left}<br />{t("对比")} {comparison.right}</td>
            <td>{comparison.requestedModel}<br />{compositionLabel(comparison.composition)}</td>
            <td>{t("切分")} {comparison.samplesLeft} / {comparison.samplesRight}<br />{t("计费")} {comparison.billingSamplesLeft ?? 0} / {comparison.billingSamplesRight ?? 0}</td>
            <td>{t("帧长")} {ratio(comparison.deltaRatio)}<br />{t("首字")} {ratio(comparison.ttfbRatio)}<br /><span className={comparison.status === "insufficient" ? "audit-untested" : comparison.status === "changed" ? "audit-statistical" : "audit-clear"}>
              {comparison.status === "insufficient" ? t("未测到：样本不足") : comparison.status === "changed" ? t("存在统计差异") : t("未见明显差异")}
            </span></td>
            <td className="audit-billing-comparison">{t("token / 字节")} {ratio(billingComparable ? comparison.tokenByteRatio : null)}<br />{t("字节 / 帧")} {ratio(billingComparable ? comparison.frameByteRatio : null)}<br />
              <span className={!billingComparable ? "audit-untested" : comparison.billingStatus === "changed" ? "audit-statistical" : "audit-clear"}>{!billingComparable ? t("不可比：计费样本不足或指标缺失") : comparison.billingStatus === "changed" ? t("计费比率存在统计差异") : t("计费比率未见明显差异")}</span>
            </td>
          </tr>; })}</tbody>
        </table></div>
      ) : <p className="audit-untested">{t("未测到：没有可比较的渠道或时间组。这不等于没问题。")}</p>}
      {report.comparisons.length > 8 && <Button type="button" variant="secondary" size="sm" onClick={() => setShowAll(!showAll)}>{showAll ? t("收起比较") : t("展开全部 {value0} 组比较", { value0: report.comparisons.length })}</Button>}
      {report.groups.length > 0 && <details className="audit-group-details"><summary>{t("查看采样分组（")}{report.groups.length} {t("组）")}</summary>
        <div className="audit-table-scroll"><table className="audit-table"><thead><tr><th scope="col">{t("渠道 / 请求模型")}</th><th scope="col">{t("时间窗 / 构成")}</th><th scope="col">{t("切分 / 计费 / 总样本")}</th><th scope="col">{t("平均帧字符 / 首字 ms")}</th><th scope="col">{t("平均 token/字节 · 字节/帧")}</th></tr></thead><tbody>
          {report.groups.map((group, index) => <tr key={index}><td>{group.providerId}<br />{group.requestedModel}</td><td>{new Date(group.windowStart).toLocaleString(localeTag())}<br />{compositionLabel(group.composition)}</td><td>{group.comparableSamples} / {group.billingSamples ?? 0} / {group.samples}</td><td>{measurement(group.meanDeltaChars)} / {measurement(group.meanTtfbMs)}</td><td className="audit-billing-group">{billingMean(group.meanTokensPerByte, group.billingSamples)} / {billingMean(group.meanBytesPerFrame, group.billingSamples)}{!(group.billingSamples >= 20) && <><br /><span className="audit-untested">{t("计费样本不足 20")}</span></>}</td></tr>)}
        </tbody></table></div>
      </details>}
      <p className="audit-note">{t("尚未排除网络拥塞、渠道缓冲、请求复杂度和采样偏差。帧批处理记录不参与切分与时延结论。")}</p>
      {report.notes.length > 0 && <ul className="audit-limitations">{report.notes.map((note, index) => <li key={index}>{translateMessage(note)}</li>)}</ul>}
    </div>
  );
}

function ReferenceAnalysis() {
  useLanguage();
  const [observationPath, setObservationPath] = useState("");
  const [candidatePaths, setCandidatePaths] = useState("");
  const [tokenizerPath, setTokenizerPath] = useState("");
  const [model, setModel] = useState("");
  const [pretokenizer, setPretokenizer] = useState<Pretokenizer | "">("");
  const [generated, setGenerated] = useState<ModelFingerprint[]>([]);
  const [report, setReport] = useState<ReferenceReport | null>(null);
  const [busy, setBusy] = useState<"generate" | "analyze" | null>(null);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const invalidate = () => { generation.current += 1; setReport(null); setError(""); };
  useEffect(() => () => { generation.current += 1; }, []);

  const generate = async () => {
    const requestGeneration = ++generation.current;
    setBusy("generate"); setError(""); setReport(null);
    try {
      const reference = await generateModelReference({ tokenizerPath: tokenizerPath.trim(), model: model.trim(), pretokenizer: pretokenizer || undefined });
      if (requestGeneration === generation.current) setGenerated((previous) => [...previous, reference]);
    } catch (cause) {
      if (requestGeneration === generation.current) setError(String(cause));
    } finally { setBusy(null); }
  };
  const analyze = async () => {
    const requestGeneration = ++generation.current;
    setBusy("analyze"); setError(""); setReport(null);
    try {
      const paths = candidatePaths.split(/\r?\n/).map((path) => path.trim()).filter(Boolean);
      const [observation, ...candidates] = await Promise.all([loadModelReference(observationPath.trim()), ...paths.map(loadModelReference)]);
      const result = await analyzeModelReference({ observation, candidates: [...candidates, ...generated] });
      if (requestGeneration === generation.current) setReport(result);
    } catch (cause) {
      if (requestGeneration === generation.current) setError(String(cause));
    } finally { setBusy(null); }
  };
  return (
    <div className="audit-reference-form">
      <p>{t("导入同一语料方案的观测指纹与候选指纹。这里只在本地计算；定位渠道需要你提供独立采集的观测指纹，不能从被动用量数字反推出真实模型。")}</p>
      <p className="audit-note">{t("本地生成只读取分词器并使用固定测试语料；仅保存结构指纹，不保存业务正文、响应头值或工具参数。不会自动发送探针。")}</p>
      <Field label={t("观测指纹 JSON 绝对路径")}><Input value={observationPath} disabled={busy !== null} onChange={(event) => { setObservationPath(event.target.value); invalidate(); }} placeholder="C:\\audit\\observation.json" spellCheck={false} /></Field>
      <Field label={t("候选指纹 JSON 绝对路径（每行一个）")}><Textarea rows={3} value={candidatePaths} disabled={busy !== null} onChange={(event) => { setCandidatePaths(event.target.value); invalidate(); }} placeholder="C:\\audit\\reference.json" spellCheck={false} /></Field>
      <details className="audit-tokenizer-form"><summary>{t("从本地分词器生成候选指纹")}</summary>
        <Field label={t("分词器文件绝对路径")}><Input value={tokenizerPath} disabled={busy !== null} onChange={(event) => setTokenizerPath(event.target.value)} placeholder={t("C:\\\\audit\\\\tokenizer.json 或 vocab.tiktoken")} spellCheck={false} /></Field>
        <div className="audit-form-row"><Field label={t("候选名称")}><Input value={model} disabled={busy !== null} onChange={(event) => setModel(event.target.value)} placeholder={t("本地词表名称")} /></Field>
          <Field label={t("预分词规则")}><Select value={pretokenizer} disabled={busy !== null} onChange={(event) => setPretokenizer(event.target.value as Pretokenizer | "")}><option value="">{t("自动识别文件规则")}</option><option value="cl100k">cl100k</option><option value="o200k">o200k</option><option value="gpt2">GPT-2</option></Select></Field></div>
        <Button type="button" variant="secondary" size="sm" disabled={busy !== null || !tokenizerPath.trim() || !model.trim()} onClick={() => void generate()}>{busy === "generate" ? t("生成中…") : t("生成候选指纹")}</Button>
      </details>
      {generated.length > 0 && <ul className="audit-generated-references">{generated.map((reference, index) => <li key={`${reference.sourceSha256}-${index}`}><strong>{reference.model}</strong> · {reference.groups.length} {t("组 ·")} {reference.tokenizerKind}<br /><code>SHA-256 {reference.sourceSha256}</code><div>
        <Button type="button" variant="secondary" size="sm" onClick={() => downloadReference(reference)}>{t("导出指纹 JSON")}</Button>
        <Button type="button" variant="secondary" size="sm" disabled={busy !== null} onClick={() => { setGenerated((previous) => previous.filter((_, i) => i !== index)); invalidate(); }}>{t("移除")}</Button></div>
        {reference.limitations.map((limitation, item) => <p className="audit-note" key={item}>{translateMessage(limitation)}</p>)}
      </li>)}</ul>}
      <div><Button type="button" variant="primary" disabled={busy !== null || !observationPath.trim() || (!candidatePaths.trim() && generated.length === 0)} onClick={() => void analyze()}>{busy === "analyze" ? t("分析中…") : t("分析参照库")}</Button></div>
      {error && <p className="audit-error" role="alert">{t("参照分析未完成：")}{translateMessage(error)}</p>}
      {report && <div className="audit-reference-report" aria-live="polite">
        <h3>{t("词表等价类排名")}</h3>
        {report.status === "untested" ? <p className="audit-untested">{t("未测到：可比的稳定分词组不足。这不等于没问题。")}</p> : <div className="audit-table-scroll"><table className="audit-table"><thead><tr><th scope="col">{t("候选")}</th><th scope="col">{t("模型下的相对概率")}</th><th scope="col">{t("一致 / 可比组")}</th></tr></thead><tbody>
          {report.candidates.map((candidate, index) => <tr key={`${candidate.model}-${index}`} className={candidate.outside ? "audit-outside-row" : ""}><td>{candidate.outside ? t("库外：不属于任何库内候选") : candidate.model}</td><td>{(candidate.probability * 100).toFixed(2)}%</td><td>{candidate.outside ? "—" : `${candidate.matched} / ${candidate.comparable}`}</td></tr>)}
        </tbody></table></div>}
        <p>{t("概率依赖候选库、先验和似然配置，不是客观概率。全部匹配只能落在词表等价类里；即使只有一个候选，也不能确认具体模型。库外档始终保留。")}</p>
        <p className="audit-note">{t("参与比较：")}{report.comparedGroups.join("、") || t("无")}</p>
        {report.limitations.length > 0 && <ul className="audit-limitations">{report.limitations.map((limitation, index) => <li key={index}>{translateMessage(limitation)}</li>)}</ul>}
      </div>}
    </div>
  );
}

function downloadReference(reference: ModelFingerprint): void {
  const url = URL.createObjectURL(new Blob([JSON.stringify(reference, null, 2)], { type: "application/json" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `${reference.model.replace(/[^\p{L}\p{N}._-]/gu, "_") || "reference"}.fingerprint.json`;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function measurement(value: number | null): string { return value == null ? t("未测到") : value.toFixed(2); }
function billingMean(value: number | null, samples: number): string { return !(samples >= 20) || value == null ? t("不可比") : value.toFixed(3); }
function ratio(value: number | null): string { return value == null ? t("不可比") : `${value.toFixed(2)}×`; }
function compositionLabel(value: string): string { return ({ digit: t("数字为主"), han: t("汉字为主"), space: t("空白为主"), mixed: t("混合文本"), other: t("其他文本") } as Record<string, string>)[value] ?? value; }
