import { useEffect, useRef } from "react";
import { ArrowLeft } from "lucide-react";
import { localeTag, t, useLanguage } from "../../i18n/language";
import { Badge, Button, EmptyState } from "../../ui";
import type { UsageLogEntry } from "../../types/usage";
import { costLabel, durationLabel, isSessionRow, statusLabel, tokenLabel } from "./usage-presentation";
import { ResponseModelEvidence } from "./ResponseModelEvidence";
import { UsageRequestDetails } from "./UsageRequestDetails";

export function UsageRequestWorkspace({ rows, providerNames, selected, onSelect }: { rows: UsageLogEntry[]; providerNames: Map<string, string>; selected: UsageLogEntry | null; onSelect: (row: UsageLogEntry | null) => void }) {
  useLanguage();
  const workspace = useRef<HTMLDivElement>(null);
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const directoryHeading = useRef<HTMLHeadingElement>(null);
  const returnTarget = useRef<HTMLButtonElement | null>(null);
  const returnScroll = useRef(0);
  const restoring = useRef(false);
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      if (selected) {
        detailHeading.current?.focus({ preventScroll: true });
        if ((workspace.current?.clientWidth ?? 0) < 1100) detailHeading.current?.parentElement?.scrollIntoView({ block: "start" });
      } else if (restoring.current) {
        const main = workspace.current?.closest<HTMLElement>(".xy-shell-main");
        if (main) main.scrollTop = returnScroll.current;
        (returnTarget.current?.isConnected ? returnTarget.current : directoryHeading.current)?.focus({ preventScroll: true });
        restoring.current = false;
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [selected]);
  const inspect = (row: UsageLogEntry, trigger: HTMLButtonElement) => {
    returnTarget.current = trigger;
    returnScroll.current = workspace.current?.closest<HTMLElement>(".xy-shell-main")?.scrollTop ?? 0;
    onSelect(row);
  };
  const close = () => { restoring.current = true; onSelect(null); };
  return <div ref={workspace} className="usage-requests-workspace" data-inspecting={Boolean(selected)}>
    <section className="usage-request-directory" aria-labelledby="usage-request-directory-title">
      <h2 id="usage-request-directory-title" ref={directoryHeading} tabIndex={-1}>{t("请求记录")}</h2>
      {rows.length ? <div className="usage-table-region usage-log-table" role="region" tabIndex={0} aria-label={t("请求日志")}><table className="xy-ui-table" role="table"><caption className="sr-only">{t("请求日志")}</caption><thead><tr><th scope="col">{t("时间 / 客户端")}</th><th scope="col">{t("供应商 / 模型")}</th><th scope="col">{t("结果 / 成本")}</th><th scope="col" className="usage-inline-evidence">{t("响应模型探测")}</th><th scope="col">{t("详情")}</th></tr></thead><tbody>
        {rows.map(row => <tr key={row.requestId} data-selected={selected?.requestId === row.requestId}>
          <td><time dateTime={new Date(row.createdAt).toISOString()}><span>{new Date(row.createdAt).toLocaleDateString(localeTag())}</span><br /><span>{new Date(row.createdAt).toLocaleTimeString(localeTag(), { hour12: false })}</span></time><small className="usage-cell-note">{row.appType}</small></td>
          <td><strong className="usage-provider-name">{providerNames.get(row.providerId) ?? row.providerId}</strong><code className="usage-cell-note" tabIndex={0}>{row.model || t("未记录")}</code><small className="usage-cell-note">{t("输入 / 输出")} <span className="usage-contributions__tokens">{tokenLabel(row.inputTokens, row.tokenCoverage?.input)} / {tokenLabel(row.outputTokens, row.tokenCoverage?.output)}</span></small></td>
          <td><Badge tone={row.success !== false && row.statusCode >= 200 && row.statusCode < 400 ? "success" : "danger"}>{statusLabel(row)}{row.isStreaming ? t(" · 流式") : ""}</Badge><span className="usage-cell-note usage-amount">{costLabel(row.totalCostUsd)}</span><small className="usage-cell-note">{isSessionRow(row) ? t("本地会话 · 耗时未知") : durationLabel(row.latencyMs)}</small></td>
          <td className="usage-inline-evidence"><ResponseModelEvidence audit={row.audit} local={isSessionRow(row)} compact /></td>
          <td><Button size="sm" variant="quiet" aria-label={t("查看请求 ") + row.requestId} aria-pressed={selected?.requestId === row.requestId} onClick={event => inspect(row, event.currentTarget)}>{t("查看证据")}</Button></td>
        </tr>)}
      </tbody></table></div> : <EmptyState title={t("当前筛选范围没有请求")} />}
      <p className="usage-footnote">{t("每条响应独立判断。上游声明与真实响应模型分开展示；探测详情只分析本条已保存证据，不发起新请求，不套用其他记录的结果。")}</p>
    </section>
    {selected && <section className="usage-request-inspector" aria-labelledby="usage-request-inspector-title">
      <div className="usage-request-inspector__heading"><Button variant="quiet" size="sm" icon={<ArrowLeft size={15} />} onClick={close}>{t("返回请求记录")}</Button><h2 id="usage-request-inspector-title" ref={detailHeading} tabIndex={-1}>{t("单条请求证据")}</h2></div>
      <UsageRequestDetails key={selected.requestId} row={selected} providerName={providerNames.get(selected.providerId) ?? selected.providerId} />
    </section>}
  </div>;
}
