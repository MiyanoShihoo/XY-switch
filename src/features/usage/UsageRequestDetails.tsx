import { localeTag, useLanguage } from "../../i18n/language";
import { InlineError } from "../../ui";
import type { UsageLogEntry } from "../../types/usage";
import { ModelAuditCell } from "./ModelAuditCell";
import { RequestDiagnosticsPanel } from "./RequestDiagnosticsPanel";
import { ResponseModelEvidence } from "./ResponseModelEvidence";
import { costLabel, durationLabel, inputOutputTokens, isSessionRow, knownTokenValue, statusLabel, tokenLabel } from "./usage-presentation";
import { t } from "../../i18n/language";

export function UsageRequestDetails({ row, providerName }: { row: UsageLogEntry; providerName: string }) {
  useLanguage();
  const local = isSessionRow(row);
  const total = inputOutputTokens(row);
  const costs = [
    { label: t("输入"), tokens: row.inputTokens, knownCount: row.tokenCoverage?.input, cost: row.inputCostUsd },
    { label: t("输出"), tokens: row.outputTokens, knownCount: row.tokenCoverage?.output, cost: row.outputCostUsd },
    { label: t("缓存读取"), tokens: row.cacheReadTokens, knownCount: row.tokenCoverage?.cacheRead, cost: row.cacheReadCostUsd },
    { label: t("缓存写入"), tokens: row.cacheCreationTokens, knownCount: row.tokenCoverage?.cacheCreation, cost: row.cacheCreationCostUsd },
  ];
  return <article className="usage-request" aria-label={t("单条请求证据")}>
    <header className="usage-request__identity">
      <span className="usage-eyebrow">{t("本条记录")}</span>
      <code tabIndex={0}>{row.requestId}</code>
      <p><time dateTime={new Date(row.createdAt).toISOString()}>{new Date(row.createdAt).toLocaleString(localeTag())}</time><span> · {local ? t("本地会话导入") : row.source === "proxy" || !row.source ? t("本地路由") : row.source}</span></p>
      <p className="usage-footnote">{t("仅检查这条已保存记录，不会发起新请求。")}</p>
    </header>
    <dl className="usage-facts usage-request__context">
      <div><dt>{t("供应商 / 客户端")}</dt><dd className="usage-provider-name">{providerName} / {row.appType}</dd></div>
      <div><dt>{local ? t("会话状态 / 耗时") : t("状态 / 耗时")}</dt><dd>{statusLabel(row)} / {local ? t("未知") : durationLabel(row.latencyMs)}</dd></div>
      <div><dt>{t("计费模型 ID")}</dt><dd><code tabIndex={0}>{row.model || t("未记录")}</code></dd></div>
      <div><dt>{t("总 token / 推理 token")}</dt><dd>{row.totalTokens != null ? tokenLabel(row.totalTokens, 1) : tokenLabel(total.value, total.knownCount, 2)} / {tokenLabel(row.reasoningTokens, 1)}</dd></div>
      <div><dt>{t("成本倍率")}</dt><dd>{row.costMultiplier ? row.costMultiplier + "×" : "—"}</dd></div>
    </dl>
    <section className="usage-request__evidence" aria-label={t("证据及下一步")}>
      <ResponseModelEvidence key={"identity-" + row.requestId} audit={row.audit} local={local} />
      {row.errorMessage && !row.diagnostics && <InlineError message={row.errorMessage} />}
      {!local && <RequestDiagnosticsPanel diagnostics={row.diagnostics} errorCode={row.errorMessage} />}
      {!local && <ModelAuditCell key={row.requestId} audit={row.audit} legacyModel={row.model} />}
    </section>
    <section className="usage-request__costs" aria-label={t("用量与成本")}>
      <div className="usage-section-heading"><h3>{t("用量与成本")}</h3><span>{t("记录成本（USD）")}</span></div>
      <div className="usage-table-region" role="region" tabIndex={0} aria-label={t("用量与成本")}>
        <table className="usage-cost-table"><thead><tr><th scope="col">{t("类别")}</th><th scope="col">{t("记录 token")}</th><th scope="col">{t("记录费用（USD）")}</th></tr></thead>
          <tbody>{costs.map(item => <tr key={item.label}><th scope="row">{item.label}</th><td>{tokenLabel(item.tokens, item.knownCount)}</td><td><span className="usage-amount">{costLabel(knownTokenValue(item.tokens, item.knownCount) === null ? null : item.cost)}</span></td></tr>)}</tbody>
          <tfoot><tr><th scope="row" colSpan={2}>{t("总费用")}</th><td><span className="usage-amount">{costLabel(row.totalCostUsd)}</span></td></tr></tfoot>
        </table>
      </div>
      <p className="usage-footnote">{t("不代表上游最终账单")} · {t("空值不是零费用。")}</p>
      <p className="usage-footnote">{t("输入及缓存按各来源的记录口径展示，不应直接相加。")}</p>
    </section>
  </article>;
}
