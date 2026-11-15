import { useMemo, useState } from "react";
import { ChevronRight, Database } from "lucide-react";
import { Button } from "../../ui";
import type { AgentSnapshot, AgentUsageGroup } from "../../types/agents";
import { count, money, timestamp } from "./format";
import { clientName, modeText, useAgentText, type AgentText } from "./i18n";
import { ConnectionBadge, EmptyObservation } from "./presentation";

export const dimensionLabels: Record<AgentUsageGroup["dimension"], AgentText> = { client: "按客户端", model: "按模型", team: "按团队", agent: "按成员", task: "按任务", session: "按会话" };
export function ResourceView({ snapshot, onSource, onUsage }: { snapshot: AgentSnapshot; onSource: (id: string) => void; onUsage: (dimension: AgentUsageGroup["dimension"], id: string) => void }) {
  const { t, locale } = useAgentText();
  const [dimension, setDimension] = useState<AgentUsageGroup["dimension"]>("client");
  const [limit, setLimit] = useState(40);
  const groups = useMemo(() => snapshot.usageGroups.filter(group => group.dimension === dimension).sort((left, right) => right.usage.totalTokens - left.usage.totalTokens || left.id.localeCompare(right.id)), [snapshot.usageGroups, dimension]);
  return <section className="ag-resources"><div className="ag-view-heading"><div><h2>{t("资源与用量")}</h2><p>{t("真实数据源")} · {t("按维度查看累计用量")}</p></div></div>
    <div className="ag-source-list">{snapshot.sources.map(source => <button type="button" key={source.id} className="ag-source-row" onClick={() => onSource(source.id)}><Database size={20} strokeWidth={1.4} aria-hidden="true" /><span className="ag-source-row__name"><strong>{source.name}</strong><small title={source.clientId}>{clientName(source.clientId)} · {count(source.fileCount, locale)} {t("文件数")}</small></span><ConnectionBadge connection={source.connection} /><span className="ag-mode" data-mode={source.mode}>{modeText(source.mode, t)}</span><time title={t("最近检查")}>{timestamp(source.lastCheckedAt, locale, true)}</time><ChevronRight size={16} aria-hidden="true" /></button>)}</div>
    {!snapshot.sources.length && <EmptyObservation title={t("没有可用数据源")} />}
    <div className="ag-usage-heading"><h3>{t("累计 Token")}</h3><label className="ag-inline-field"><span>{t("维度")}</span><select value={dimension} onChange={event => { setDimension(event.target.value as AgentUsageGroup["dimension"]); setLimit(40); }}>{Object.entries(dimensionLabels).map(([key, value]) => <option key={key} value={key}>{t(value)}</option>)}</select></label></div>
    {groups.length ? <><div className="ag-usage-table-wrap"><table className="ag-usage-table"><thead><tr><th scope="col">{t("名称")}</th><th scope="col">Token</th><th scope="col">{t("用量记录")}</th><th scope="col">{t("估算费用")}</th></tr></thead><tbody>{groups.slice(0, limit).map(group => <tr key={group.id}><th scope="row"><button type="button" onClick={() => onUsage(group.dimension, group.id)} title={group.label}>{group.dimension === "client" ? clientName(group.id) : group.label}<ChevronRight size={14} aria-hidden="true" /></button></th><td>{group.usage.records ? count(group.usage.totalTokens, locale) : "—"}</td><td>{count(group.usage.records, locale)}</td><td>{money(group.usage.estimatedCostUsd, locale)}</td></tr>)}</tbody></table></div><div className="ag-list-footer"><span>{t("显示")} {count(Math.min(limit, groups.length), locale)} / {count(groups.length, locale)}</span>{limit < groups.length && <Button size="sm" variant="secondary" onClick={() => setLimit(value => value + 40)}>{t("显示更多")}</Button>}</div></> : <EmptyObservation title={t("没有该维度的用量")} />}
    <p className="ag-note">{t("观测数据不包含 CPU / 内存采样。")}</p>
  </section>;
}
