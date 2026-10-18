import { Button, Field, Input, Select, Toggle } from "../../ui";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { analyzeShadowAudit, getShadowAuditSettings, saveShadowAuditSetting } from "../../lib/model-audit-service";
import { shadowReasonLabel, shadowSettingRequest } from "../../lib/shadow-audit";
import type { Pretokenizer, ShadowAuditReport, ShadowAuditSetting, ShadowAuditStatus, ShadowRegressionReport } from "../../types/model-audit";
import type { UsageFilter, UsageLogEntry } from "../../types/usage";

const EMPTY_SETTING: ShadowAuditSetting = { enabled: false, providerId: "", model: "", tokenizerPath: "", pretokenizer: null };

export function ShadowAuditPanel({ filter, logs }: { filter: UsageFilter; logs: UsageLogEntry[] }) {
  const [settings, setSettings] = useState<ShadowAuditStatus[]>([]);
  const [draft, setDraft] = useState<ShadowAuditSetting>({ ...EMPTY_SETTING });
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [analyzing, setAnalyzing] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [report, setReport] = useState<ShadowAuditReport | null>(null);
  const analysisGeneration = useRef(0);
  const providerListId = useId();
  const modelListId = useId();
  const providers = useMemo(() => [...new Set(logs.map((log) => log.providerId))], [logs]);
  const models = useMemo(() => [...new Set(logs.filter((log) => !draft.providerId || log.providerId === draft.providerId).map((log) => log.audit?.observation.forwardedModel || log.audit?.observation.requestedModel || log.model).filter(Boolean))], [logs, draft.providerId]);

  useEffect(() => {
    analysisGeneration.current += 1;
    setReport(null);
    setAnalyzing(false);
    return () => { analysisGeneration.current += 1; };
  }, [filter]);

  const load = async () => {
    setLoading(true); setError("");
    try { setSettings(await getShadowAuditSettings()); setLoaded(true); }
    catch (cause) { setError(`配置读取失败：${String(cause)}`); }
    finally { setLoading(false); }
  };

  const save = async (value: ShadowAuditSetting) => {
    setSaving(true); setError(""); setMessage("");
    try {
      const saved = await saveShadowAuditSetting(shadowSettingRequest(value));
      setSettings((previous) => [...previous.filter((entry) => entry.providerId !== saved.providerId || entry.model !== saved.model), saved]);
      setLoaded(true);
      setMessage(saved.error ? "配置已保存，但分词器当前不可用；请查看配置错误。" : saved.enabled ? "已保存启用配置，仅采集此后符合条件的新轮次。" : "已保存停用配置。");
      analysisGeneration.current += 1; setReport(null); setAnalyzing(false);
    } catch (cause) { setError(`配置保存失败：${String(cause)}`); }
    finally { setSaving(false); }
  };

  const analyze = async () => {
    const requestGeneration = ++analysisGeneration.current;
    setAnalyzing(true); setError(""); setReport(null);
    try {
      const result = await analyzeShadowAudit(filter);
      if (requestGeneration === analysisGeneration.current) setReport(result);
    } catch (cause) {
      if (requestGeneration === analysisGeneration.current) setError(`差分分析未完成：${String(cause)}`);
    } finally { if (requestGeneration === analysisGeneration.current) setAnalyzing(false); }
  };

  return (
    <details className="audit-reference-section audit-shadow-section" onToggle={(event) => { if (event.currentTarget.open && !loaded && !loading) void load(); }}>
      <summary>被动影子差分（默认关闭）</summary>
      <div className="audit-reference-form">
        <p>按渠道与实际转发模型启用后，后台用本地分词器计算相邻轮次的新增文本，并与上游输入 token 增量比较。它只观察正常业务，不发送探针。</p>
        <p className="audit-note">仅匹配新产生、完整结束且只追加的纯文本对话。缓存、工具/多模态、缺失用量或无法唯一配对的轮次会被排除。匹配只在有界内存中进行，正文不持久化；重启、过期或达到容量上限会失去基线。历史正文不能补算。</p>
        <p className="audit-note">前缀核对仅覆盖客户端请求和本地实际转发内容，无法据此证明远端没有改写系统提示词。</p>
        <div className="audit-shadow-actions"><Button type="button" variant="secondary" size="sm" onClick={() => void load()} disabled={loading || saving}>{loading ? "读取中…" : "刷新配置"}</Button></div>
        {loaded && settings.length === 0 && <p className="audit-untested">尚未配置，影子采样默认关闭。</p>}
        {settings.length > 0 && <div className="audit-table-scroll"><table className="audit-table"><thead><tr><th scope="col">渠道 / 实际转发模型</th><th scope="col">采样状态</th><th scope="col">分词器</th><th scope="col">操作</th></tr></thead><tbody>
          {settings.map((setting) => <tr key={JSON.stringify([setting.providerId, setting.model])}>
            <td>{setting.providerId}<br />{setting.model}</td>
            <td className={setting.error ? "audit-error" : setting.enabled ? "audit-clear" : "audit-untested"}>{setting.error ? "配置不可用" : setting.enabled ? "已启用" : "已停用"}<br />后台丢弃 {setting.droppedRequests} 次</td>
            <td className="audit-shadow-source"><span>{setting.tokenizerPath || "未配置文件"}</span><br /><code>SHA-256 {setting.sourceSha256 || "未加载"}</code>{setting.error && <p className="audit-error">{setting.error}</p>}</td>
            <td><div className="audit-shadow-actions"><Button type="button" variant="secondary" size="sm" disabled={saving} onClick={() => { setDraft(shadowSettingRequest(setting)); setMessage(""); }}>编辑</Button>
              <Button type="button" variant="secondary" size="sm" disabled={saving || loading} onClick={() => void save({ ...shadowSettingRequest(setting), enabled: !setting.enabled })}>{setting.enabled ? "停用" : "启用"}</Button></div></td>
          </tr>)}
        </tbody></table></div>}
        <form onSubmit={(event) => { event.preventDefault(); void save(draft); }}>
          <fieldset className="audit-shadow-fields" disabled={saving || loading}>
            <legend>新增或编辑渠道配置</legend>
            <div className="audit-shadow-grid">
              <Field label="渠道 ID（精确匹配）"><Input required list={providerListId} value={draft.providerId} onChange={(event) => setDraft({ ...draft, providerId: event.target.value })} placeholder="请求日志中的供应商 ID" spellCheck={false} /></Field>
              <Field label="实际转发模型（精确匹配）"><Input required list={modelListId} value={draft.model} onChange={(event) => setDraft({ ...draft, model: event.target.value })} placeholder="有路由改写时填写路由后模型" spellCheck={false} /></Field>
            </div>
            <datalist id={providerListId}>{providers.map((provider) => <option key={provider} value={provider} />)}</datalist>
            <datalist id={modelListId}>{models.map((model) => <option key={model} value={model} />)}</datalist>
            <Field label="本地分词器绝对路径"><Input required={draft.enabled} value={draft.tokenizerPath} onChange={(event) => setDraft({ ...draft, tokenizerPath: event.target.value })} placeholder="C:\\audit\\o200k_base.tiktoken 或 tokenizer.json" spellCheck={false} /></Field>
            <div className="audit-shadow-grid">
              <Field label="预分词规则"><Select value={draft.pretokenizer ?? ""} onChange={(event) => setDraft({ ...draft, pretokenizer: (event.target.value || null) as Pretokenizer | null })}><option value="">自动识别文件规则</option><option value="cl100k">cl100k</option><option value="o200k">o200k</option><option value="gpt2">GPT-2</option></Select></Field>
              <div className="form-switch"><span>保存后启用此渠道与模型的影子采样</span><Toggle aria-label="启用影子采样" checked={draft.enabled} onCheckedChange={enabled => setDraft({ ...draft, enabled })} /></div>
            </div>
            <div className="audit-shadow-actions"><Button type="submit" variant="primary" disabled={!draft.providerId.trim() || !draft.model.trim() || (draft.enabled && !draft.tokenizerPath.trim())}>{saving ? "保存中…" : "保存配置"}</Button><Button type="button" variant="secondary" size="sm" onClick={() => setDraft({ ...EMPTY_SETTING })}>新建配置</Button></div>
          </fieldset>
        </form>
        {message && <p role="status">{message}</p>}
        {error && <p className="audit-error" role="alert">{error}</p>}
        <div className="audit-shadow-analyze"><p>分析沿用上方筛选条件，只读取已经采集的结构化配对。3 组只是回归计算下限；可靠判读还需 5 个不同的预期增量值和 32 token 的跨度，并结合残差、斜率区间与时间稳定性。噪声较大或区间不明确时继续采样。</p>
          <Button type="button" variant="primary" disabled={analyzing || saving} onClick={() => void analyze()}>{analyzing ? "分析中…" : "分析已采集差分"}</Button>
        </div>
        {report && <ShadowAuditResults report={report} />}
      </div>
    </details>
  );
}

export function ShadowAuditResults({ report }: { report: ShadowAuditReport }) {
  const [showAll, setShowAll] = useState(false);
  const groups = showAll ? report.groups : report.groups.slice(0, 8);
  return (
    <div className="audit-shadow-results" aria-live="polite">
      <h3>影子差分分析结果</h3>
      <p>本次读取 {report.sampleCount} 条结构记录：配对 {report.pairCount} 组，基线 {report.baselineCount} 条，排除 {report.excludedCount} 条。后台累计丢弃 {report.droppedRequests} 次。</p>
      {report.truncated && <p className="audit-untested">达到 {report.sampleLimit} 条分析上限，仅覆盖最近记录。</p>}
      {groups.length === 0 && <p className="audit-untested">未测到：没有可分析的配对分组。这不等于没问题；启用后还需产生符合条件的新对话轮次。</p>}
      <div className="audit-shadow-groups">{groups.map((group) => <article className="audit-shadow-group" key={JSON.stringify([group.providerId, group.model, group.tokenizerSha256, group.partitionId])}>
        <h4>{group.providerId} · {group.model}</h4>
        <p className="audit-note audit-shadow-source">词表 SHA-256 <code>{group.tokenizerSha256}</code><br />运行内配对分区 <code>{group.partitionId}</code>（不包含对话正文）</p>
        <ShadowRegressionDetails regression={group.regression} />
      </article>)}</div>
      {report.groups.length > 8 && <Button type="button" variant="secondary" size="sm" onClick={() => setShowAll(!showAll)}>{showAll ? "收起分组" : `展开全部 ${report.groups.length} 个分组`}</Button>}
      {report.reasons.length > 0 && <details className="audit-group-details"><summary>基线与排除原因</summary><ul>{report.reasons.map(({ reason, count }) => <li key={reason}>{shadowReasonLabel(reason)}：{count} 条 <code>{reason}</code></li>)}</ul></details>}
      <p>斜率或截距偏离只能作为统计线索。本地分词器误差、角色模板、隐藏的 prompt 改写和样本相关性仍可能影响结果，不能直接认定虚报或模型替换。</p>
      {report.notes.length > 0 && <ul className="audit-limitations">{report.notes.map((note, index) => <li key={index}>{note}</li>)}</ul>}
    </div>
  );
}

export function ShadowRegressionDetails({ regression }: { regression: ShadowRegressionReport }) {
  const untested = regression.status === "untested";
  return (
    <div>
      <p className={untested ? "audit-untested" : regression.status === "suspicious" ? "audit-statistical" : "audit-clear"}>
        <strong>{untested ? "未测到：尚不满足稳定回归条件" : regression.status === "suspicious" ? "存在统计偏离，需核查" : "当前样本内未见稳定偏离"}</strong>
      </p>
      <p>有效配对 {regression.sampleCount} 组，回归计算下限 {regression.minimumSamples} 组；达到计算下限不代表证据充足。{untested && "以下数值仅供诊断，不能作为结论；这不等于没问题。"}</p>
      <dl className="audit-shadow-metrics">
        <div><dt>斜率</dt><dd>{metric(regression.slope)}</dd></div>
        <div><dt>截距（token）</dt><dd>{metric(regression.intercept)}</dd></div>
        <div><dt>斜率条件性约 95% 区间</dt><dd>{regression.slopeCiLow == null || regression.slopeCiHigh == null ? "未测到" : `${metric(regression.slopeCiLow)} ～ ${metric(regression.slopeCiHigh)}`}</dd></div>
        <div><dt>残差 RMSE（token）</dt><dd>{metric(regression.residualRmse)}</dd></div>
        <div><dt>预期增量跨度（token）</dt><dd>{metric(regression.xSpan)}</dd></div>
        <div><dt>截距漂移（token）</dt><dd>{metric(regression.interceptDrift)}</dd></div>
      </dl>
      {regression.gaps.length > 0 && <ul className="audit-untested">{regression.gaps.map((gap) => <li key={gap}>{shadowReasonLabel(gap)} <code>{gap}</code></li>)}</ul>}
      {regression.limitations.length > 0 && <ul className="audit-limitations">{regression.limitations.map((limitation, index) => <li key={index}>{limitation}</li>)}</ul>}
    </div>
  );
}

function metric(value: number | null): string { return value == null ? "未测到" : value.toFixed(3); }
