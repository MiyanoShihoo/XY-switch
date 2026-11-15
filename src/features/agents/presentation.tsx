import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy, Workflow } from "lucide-react";
import { IconButton } from "../../ui";
import type { AgentContext, AgentStatus, AgentUsage, SourceConnection } from "../../types/agents";
import { connectionText, statusText, useAgentText } from "./i18n";
import { count, money, timestamp } from "./format";

export function StatusBadge({ status }: { status: AgentStatus }) {
  const { t } = useAgentText();
  return <span className="ag-status" data-status={status}><span aria-hidden="true" />{statusText(status, t)}</span>;
}
export function ConnectionBadge({ connection }: { connection: SourceConnection }) {
  const { t } = useAgentText();
  return <span className="ag-connection" data-connection={connection}><span aria-hidden="true" />{connectionText(connection, t)}</span>;
}
export function EmptyObservation({ title, children, icon }: { title: string; children?: ReactNode; icon?: ReactNode }) {
  return <div className="ag-empty">{icon ?? <Workflow size={30} strokeWidth={1.3} aria-hidden="true" />}<h3>{title}</h3>{children && <div>{children}</div>}</div>;
}
export function Fact({ label, children, mono = false }: { label: string; children: ReactNode; mono?: boolean }) {
  return <div className="ag-fact"><dt>{label}</dt><dd className={mono ? "ag-mono" : undefined}>{children ?? "—"}</dd></div>;
}
export function CopyValue({ value }: { value: string | null | undefined }) {
  const { t } = useAgentText();
  const [copied, setCopied] = useState(false), [failed, setFailed] = useState(false);
  const generation = useRef(0);
  useEffect(() => {
    ++generation.current; setCopied(false); setFailed(false);
    return () => { ++generation.current; };
  }, [value]);
  if (!value) return <>—</>;
  return <span className="ag-copy-value"><span className="ag-mono">{value}</span><IconButton size="sm" aria-label={t(copied ? "已复制" : "复制")} title={t(copied ? "已复制" : "复制")} onClick={() => {
    const request = generation.current;
    void Promise.resolve().then(() => navigator.clipboard.writeText(value)).then(() => {
      if (request === generation.current) { setCopied(true); setFailed(false); }
    }).catch(() => { if (request === generation.current) setFailed(true); });
  }}>{copied ? <Check size={13} /> : <Copy size={13} />}</IconButton>{failed && <small role="status">{t("复制失败")}</small>}</span>;
}
export function UsageDetails({ usage }: { usage: AgentUsage }) {
  const { t, locale } = useAgentText();
  const known = usage.records > 0;
  return <section className="ag-detail-section"><h3>{t("累计 Token")}</h3><div className="ag-usage-total">{known ? count(usage.totalTokens, locale) : "—"}<small>{count(usage.records, locale)} {t("用量记录")}</small></div>
    <dl className="ag-facts ag-facts--numbers">
      <Fact label={t("输入")}>{known ? count(usage.inputTokens, locale) : "—"}</Fact><Fact label={t("输出")}>{known ? count(usage.outputTokens, locale) : "—"}</Fact>
      <Fact label={t("缓存读取")}>{known ? count(usage.cacheReadTokens, locale) : "—"}</Fact><Fact label={t("缓存写入")}>{known ? count(usage.cacheWriteTokens, locale) : "—"}</Fact>
      <Fact label={t("推理")}>{known ? count(usage.reasoningTokens, locale) : "—"}</Fact><Fact label={t("实际费用")}>{money(usage.actualCostUsd, locale)}</Fact>
      <Fact label={t("估算费用")}>{money(usage.estimatedCostUsd, locale)}</Fact><Fact label={t("费用未知记录")}>{count(usage.unknownCostRecords, locale)}</Fact>
    </dl><p className="ag-note">{t("缓存和推理为细分项，不额外叠加到总量。")}</p><p className="ag-note">{t("费用估算截至快照时刻；实际费用仅展示来源明确报告的值。")}</p></section>;
}
export function ContextDetails({ context }: { context: AgentContext | null }) {
  const { t, locale } = useAgentText();
  return <section className="ag-detail-section"><h3>{t("上下文")}</h3>{context ? <><div className="ag-context-value">{count(context.usedTokens, locale)} <span>/ {count(context.windowTokens, locale)}</span></div>
    {context.windowTokens == null && <p className="ag-note">{t("窗口未提供")}</p>}<dl className="ag-facts"><Fact label={t("测量于")}>{timestamp(context.measuredAt, locale)}</Fact><Fact label={t("测量依据")}>{context.basis}</Fact></dl></> : <p className="ag-note">{t("没有上下文测量")}</p>}</section>;
}
