import { t } from "../../i18n/language";
import type { BudgetStatus } from "../../types/governance";
import { Badge } from "../../ui";
import { usd } from "./shared";

export function BudgetLedger({ value }: { value: BudgetStatus }) {
  return <aside className="gov-ledger" aria-label={t("用量与预留")}>
    <header className="gov-ledger__heading"><h3>{t("已保存策略")}</h3><Badge tone={value.policy.enabled ? "success" : "neutral"}>{value.policy.enabled ? t("已启用") : t("已停用")}</Badge></header>
    <p className="page-description">{t("用量与预留以已保存策略为准。")}</p>
    <div className="gov-table-scroll" tabIndex={0} role="region" aria-label={t("用量与预留")}><table className="gov-ledger__table">
      <thead><tr><th scope="col">{t("用量与预留")}</th><th scope="col">{t("当前")}</th><th scope="col">{t("上限")}</th></tr></thead>
      <tbody><tr><th scope="row">Token</th><td>{value.reservedTokens.toLocaleString()}</td><td>{value.policy.dailyTokens.toLocaleString()}</td></tr>
        <tr><th scope="row">USD</th><td>{usd(value.reservedMicrousd)}</td><td>{usd(value.policy.dailyMicrousd)}</td></tr></tbody>
    </table></div>
    <dl className="gov-ledger__total"><dt>{t("今日准入尝试")}</dt><dd>{value.attempts.toLocaleString()}</dd></dl>
    <p className="page-description">{t("预留额度不等同实际账单；实际用量与未知费用请查看用量与审计。")}</p>
  </aside>;
}
