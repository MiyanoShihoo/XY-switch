import { useEffect, useId, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { t, localeTag } from "../../i18n/language";
import { governanceDiff, type DiffLine } from "../../lib/governance-diff";
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
export function diffReviewRows(lines: readonly DiffLine[]) {
  let beforeLine = 1, afterLine = 1, removed = 0, added = 0;
  let previousKind: "same" | "remove" | "add" = "same";
  const rows = lines.map(line => {
    const side = line.kind === "fold" ? previousKind : line.kind;
    const count = line.kind === "fold" ? line.count ?? 0 : 1;
    const row = { ...line, side, beforeLine: side === "add" ? undefined : beforeLine, afterLine: side === "remove" ? undefined : afterLine };
    if (side !== "add") beforeLine += count;
    if (side !== "remove") afterLine += count;
    if (side === "remove") removed += count;
    if (side === "add") added += count;
    if (line.kind !== "fold") previousKind = line.kind;
    return row;
  });
  return { rows, added, removed };
}
export function DiffPreview({ before, after, identity, beforeLabel, afterLabel }: { before: string; after: string; identity?: string; beforeLabel?: string; afterLabel?: string }) {
  const headingId = useId(), legendId = useId();
  const { rows, added, removed } = useMemo(() => diffReviewRows(governanceDiff(before, after)), [before, after]);
  const beforeTitle = beforeLabel ?? t("修改前"), afterTitle = afterLabel ?? t("修改后");
  const completeBefore = beforeLabel ? t("完整内容 · {label}", { label: beforeLabel }) : t("完整修改前内容");
  const completeAfter = afterLabel ? t("完整内容 · {label}", { label: afterLabel }) : t("完整修改后内容");
  const ending = (value: string) => value.endsWith("\r\n") ? "CRLF" : value.endsWith("\n") ? "LF" : value.endsWith("\r") ? "CR" : t("无末尾换行");
  return <section className="gov-diff" aria-label={t("变更预览")}>
    <header className="gov-diff__header">
      {identity ? <code id={headingId} className="gov-diff__identity" tabIndex={0}>{identity}</code> : <strong id={headingId}>{t("变更预览")}</strong>}
      <dl className="gov-diff__counts"><div data-kind="add"><dt>{t("新增")}</dt><dd>+{added}</dd></div><div data-kind="remove"><dt>{t("移除")}</dt><dd>−{removed}</dd></div></dl>
    </header>
    <p id={legendId} className="gov-diff__legend">{t("+ 新增，− 移除，CR 回车符；双侧行号分别对应两侧内容，折叠行可在下方完整内容中查看。")}</p>
    {rows.length ? <div className="gov-diff__scroll" tabIndex={0} role="region" aria-labelledby={headingId} aria-describedby={legendId}><table className="gov-diff-lines">
      <thead><tr><th scope="col">{beforeTitle}</th><th scope="col">{afterTitle}</th><th scope="col"><span className="gov-visually-hidden">{t("变更类型")}</span></th><th scope="col">{t("内容")}</th></tr></thead>
      <tbody>{rows.map((line, index) => <tr key={index} data-kind={line.kind} data-side={line.side}>
        <td className="gov-diff__number">{line.beforeLine === undefined ? "" : line.kind === "fold" ? line.beforeLine + "–" + (line.beforeLine + (line.count ?? 0) - 1) : line.beforeLine}</td>
        <td className="gov-diff__number">{line.afterLine === undefined ? "" : line.kind === "fold" ? line.afterLine + "–" + (line.afterLine + (line.count ?? 0) - 1) : line.afterLine}</td>
        <td className="gov-diff__sign"><span aria-hidden="true">{line.side === "remove" ? "−" : line.side === "add" ? "+" : " "}</span><span className="gov-visually-hidden">{t(line.side === "remove" ? "移除" : line.side === "add" ? "新增" : "未更改")}</span></td>
        <td className="gov-diff__code">{line.kind === "fold" ? <span>{t("已折叠 {count} 行", { count: line.count ?? 0 })}{line.side === "same" ? "" : " · " + t(line.side === "remove" ? "移除" : "新增")}</span> : <pre>{line.text ? <>{line.text.endsWith("\r") ? line.text.slice(0, -1) : line.text}{line.text.endsWith("\r") && <span className="gov-diff__eol" aria-label={t("回车符")}>CR</span>}</> : <><span aria-hidden="true">{"\u200b"}</span><span className="gov-visually-hidden">{t("空行")}</span></>}</pre>}</td>
      </tr>)}</tbody>
    </table></div> : <p className="gov-diff__empty">{t("内容没有变化")}</p>}
    <div className="gov-diff__complete"><details><summary>{completeBefore}</summary><pre tabIndex={0} role="region" aria-label={completeBefore}>{before}</pre></details><details><summary>{completeAfter}</summary><pre tabIndex={0} role="region" aria-label={completeAfter}>{after}</pre></details></div>
    <p className="gov-diff__ending"><span>{t("末尾换行")}</span><span>{beforeTitle}: <code>{ending(before)}</code></span><span>{afterTitle}: <code>{ending(after)}</code></span></p>
  </section>;
}
export function useClock() {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => { const timer = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000); return () => clearInterval(timer); }, []);
  return now;
}
export const checkStatus = (status: string) => status === "passed" ? t("验收通过") : status === "failed" ? t("验收失败") : t("执行中");
