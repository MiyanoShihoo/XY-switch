import { useEffect, useRef, useState } from "react";
import { ChevronLeft } from "lucide-react";
import { localeTag, useLanguage } from "../../i18n/language";
import { Badge, Button, EmptyState } from "../../ui";
import { HARD_AUDIT_FLAGS, MODEL_STATUS_LABELS, modelAuditStatus } from "../../lib/model-audit";
import type { UsageLogEntry } from "../../types/usage";
import { UsageRequestDetails } from "./UsageRequestDetails";
import { t } from "../../i18n/language";

type Category = "all" | "hard" | "mismatch" | "untested" | "gaps";
const includes = (row: UsageLogEntry, category: Category) => category === "all" ||
  (category === "hard" && Boolean(row.audit?.flags.some(flag => HARD_AUDIT_FLAGS.has(flag)))) ||
  (category === "mismatch" && modelAuditStatus(row.audit) === "mismatched") ||
  (category === "untested" && !row.audit) || (category === "gaps" && Boolean(row.audit?.gaps.length));

export function AuditEvidenceInspector({ logs, providerNames }: { logs: UsageLogEntry[]; providerNames?: Map<string, string> }) {
  useLanguage();
  const [category, setCategory] = useState<Category>("all");
  const [selectedId, setSelectedId] = useState("");
  const lastTrigger = useRef<HTMLButtonElement | null>(null);
  const inspectionHeading = useRef<HTMLHeadingElement>(null);
  const visible = logs.filter(row => includes(row, category));
  const selected = visible.find(row => row.requestId === selectedId);
  useEffect(() => { if (selectedId && !selected) setSelectedId(""); }, [selectedId, selected]);
  useEffect(() => {
    if (selectedId) inspectionHeading.current?.focus();
    else if (lastTrigger.current?.isConnected) lastTrigger.current.focus();
  }, [selectedId]);
  const categories: { value: Category; label: string }[] = [
    { value: "all", label: t("全部记录") }, { value: "hard", label: t("有硬证据") },
    { value: "mismatch", label: t("自报不一致") }, { value: "untested", label: t("未采集") }, { value: "gaps", label: t("有缺口") },
  ];
  return <section className="audit-inspector" aria-label={t("证据检查器")}>
    <div className="audit-categories" role="group" aria-label={t("当前页审计计数")}>
      <span>{t("当前页")}</span>{categories.map(item => <Button key={item.value} variant="quiet" aria-pressed={category === item.value} onClick={() => setCategory(item.value)}>{item.label}<strong>{logs.filter(row => includes(row, item.value)).length}</strong></Button>)}
    </div>
    <p className="audit-note">{t("未采集或检查有缺口不等于没问题。计数可能重叠；自报差异与硬证据分开统计。分析由你手动触发。")}</p>
    <div className="audit-inspector__workspace" data-selected={Boolean(selected)}>
      <div className="audit-index" aria-label={t("证据目录")}>
        <div className="usage-section-heading"><h3>{t("证据目录")}</h3><span>{visible.length} / {logs.length}</span></div>
        {visible.length ? <ol>{visible.map(row => {
          const hard = row.audit?.flags.filter(flag => HARD_AUDIT_FLAGS.has(flag)).length ?? 0;
          const status = modelAuditStatus(row.audit);
          return <li key={row.requestId}><button type="button" className="audit-index__record" aria-current={selectedId === row.requestId ? "true" : undefined} aria-label={t("检查请求 {id}", { id: row.requestId })} onClick={event => { lastTrigger.current = event.currentTarget; setSelectedId(row.requestId); }}>
            <span className="audit-index__time"><time dateTime={new Date(row.createdAt).toISOString()}>{new Date(row.createdAt).toLocaleString(localeTag())}</time><code title={row.requestId}>{row.requestId}</code></span>
            <span className="audit-index__model"><strong>{providerNames?.get(row.providerId) ?? (row.providerId || t("未选择供应商"))}</strong><code title={row.model}>{row.model || t("未记录")}</code></span>
            <span className="audit-index__status"><Badge tone={hard ? "danger" : status === "mismatched" || status === "conflicting" ? "warning" : "neutral"}>{hard ? t("{value0} 项硬证据 · 查看详情", { value0: hard }) : MODEL_STATUS_LABELS[status]}</Badge>{row.audit?.gaps.length ? <small>{t("有缺口")} · {row.audit.gaps.length}</small> : null}</span>
          </button></li>;
        })}</ol> : <EmptyState title={t("当前分类没有记录")} description={t("未采集或检查有缺口不等于没问题。计数可能重叠；自报差异与硬证据分开统计。分析由你手动触发。")} action={category !== "all" ? <Button variant="secondary" onClick={() => setCategory("all")}>{t("查看全部记录")}</Button> : undefined} />}
      </div>
      {selected && <section className="audit-inspection">
        <div className="usage-section-heading"><h3 ref={inspectionHeading} tabIndex={-1}>{t("单条请求证据")}</h3><Button variant="quiet" icon={<ChevronLeft size={15} />} onClick={() => setSelectedId("")}>{t("返回证据目录")}</Button></div>
        <UsageRequestDetails key={selected.requestId} row={selected} providerName={providerNames?.get(selected.providerId) ?? (selected.providerId || t("未选择供应商"))} />
      </section>}
    </div>
  </section>;
}
