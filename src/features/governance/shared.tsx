import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { t, localeTag } from "../../i18n/language";
import { governanceDiff } from "../../lib/governance-diff";
import { usePageOperation } from "../shared/usePageOperation";
import { Field, InlineError, Input } from "../../ui";

export const stamp = (seconds: number) => seconds ? new Date(seconds * 1000).toLocaleString(localeTag()) : "—";
export const usd = (microusd: number) => new Intl.NumberFormat(localeTag(), { style: "currency", currency: "USD", maximumFractionDigits: 6 }).format(microusd / 1000000);
export function useGovernanceOperation() {
  const client = useQueryClient(), op = usePageOperation();
  const refresh = () => client.invalidateQueries({ queryKey: ["governance"] }, { throwOnError: true });
  return { ...op, refresh };
}
export function OperationMessage({ op }: { op: ReturnType<typeof useGovernanceOperation> }) {
  return <>{op.error && <InlineError message={op.error} />}{op.message && <p role="status" className="gov-success">{op.message}</p>}</>;
}
export function Confirmation({ word, value, onChange, disabled }: { word: string; value: string; onChange: (v: string) => void; disabled?: boolean }) {
  return <Field label={t("输入确认文字") + " · " + word}><Input autoComplete="off" spellCheck={false} value={value} onChange={e => onChange(e.target.value)} disabled={disabled} /></Field>;
}
export function DiffPreview({ before, after }: { before: string; after: string }) {
  const lines = useMemo(() => governanceDiff(before, after), [before, after]);
  return <section className="gov-diff" aria-label={t("变更预览")}>
    <p className="page-description">{t("红色为移除，绿色为新增；折叠部分可在下方完整文件中查看。")}</p>
    {lines.length ? <pre className="gov-diff-lines">{lines.map((line, index) => <span key={index} data-kind={line.kind}>{line.kind === "fold" ? t("已折叠 {count} 行", { count: line.count ?? 0 }) : (line.kind === "remove" ? "− " : line.kind === "add" ? "+ " : "  ") + line.text}{"\n"}</span>)}</pre> : <p>{t("内容没有变化")}</p>}
    <div className="gov-grid"><details><summary>{t("完整修改前内容")}</summary><pre>{before}</pre></details><details><summary>{t("完整修改后内容")}</summary><pre>{after}</pre></details></div>
    <p className="page-description">{t("末尾换行")} · {t("修改前")}: {before.endsWith("\n") ? "LF" : "—"} · {t("修改后")}: {after.endsWith("\n") ? "LF" : "—"}</p>
  </section>;
}
export function useClock() {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => { const timer = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000); return () => clearInterval(timer); }, []);
  return now;
}
export const checkStatus = (status: string) => status === "passed" ? t("验收通过") : status === "failed" ? t("验收失败") : t("执行中");
