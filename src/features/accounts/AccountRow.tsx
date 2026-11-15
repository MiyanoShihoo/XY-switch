import { useId, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { t } from "../../i18n/language";
import type { AccountInfo } from "../../types/accounts";
import type { ClientConnection } from "../../types/clients";
import { nativeAccountClientId, nativeClientActionIssue } from "../clients/native-account-actions";
import { AccountQuotaSummary, accountKindLabel, accountStatusLabel, authorizationLabel, safeAccountLabel } from "./account-presentation";

interface AccountRowProps {
  account: AccountInfo;
  providerLabel: string;
  participation?: string;
  nativeRows: ClientConnection[] | undefined;
  timestamp: number;
  busy: boolean;
  selected: boolean;
  actions?: ReactNode;
  onView: () => void;
}

export function AccountRow({ account, providerLabel, participation, nativeRows, timestamp, busy, selected, actions, onView }: AccountRowProps) {
  const descriptionId = useId();
  const nativeId = account.kind === "nativeManaged" ? nativeAccountClientId(account.client) : null;
  const nativeIssue = nativeId ? nativeClientActionIssue(nativeId, nativeRows) : "";
  const hasNotice = Boolean(nativeIssue || account.cleanupPending || (account.destinationApproved !== true && account.status !== "revoked"));
  return <li className="accounts-list__row" data-selected={selected}>
    <button type="button" className="accounts-list__select" disabled={busy} aria-haspopup="dialog" aria-label={t("查看账号 {account}", { account: safeAccountLabel(account.alias) })}
      aria-describedby={descriptionId + "-state " + descriptionId + "-quota" + (hasNotice ? " " + descriptionId + "-notices" : "")} onClick={onView}>
      <span className="accounts-list__identity"><strong>{safeAccountLabel(account.alias)}</strong><span className="accounts-list__identity-meta"><span>{providerLabel}</span><span>{accountKindLabel(account.kind)}</span></span></span>
      <span className="accounts-list__state" id={descriptionId + "-state"}>
        <span className="accounts-list__authorization"><span className="accounts-status" data-status={account.status}>{accountStatusLabel(account.status, account.kind)}</span>
          {account.status !== "revoked" && <span>{authorizationLabel(account.authorizationStatus)}</span>}</span>
        {participation && <span className="accounts-list__participation">{participation}</span>}
      </span>
      <span className="accounts-list__quota" id={descriptionId + "-quota"} data-label={t("额度证据")}><AccountQuotaSummary quota={account.quota} timestamp={timestamp} /></span>
      <ChevronRight className="accounts-list__open" size={16} aria-hidden="true" />
    </button>
    {actions && <div className="accounts-list__actions">{actions}</div>}
    {hasNotice && <div className="accounts-list__notices" id={descriptionId + "-notices"}>
      {account.destinationApproved !== true && account.status !== "revoked" && <p className="accounts-notice">{t("请求地址待批准；请在账号设置中检查供应商并重新确认授权。")}</p>}
      {nativeIssue && <p className="accounts-notice" role="status">{nativeIssue}</p>}
      {account.cleanupPending && <p className="accounts-notice" role="status">{account.status === "revoked"
        ? t("安全存储清理待完成；本地引用保持撤销，可重试清理。")
        : t("旧凭据的安全存储清理待完成；当前账号引用仍有效。")}</p>}
    </div>}
  </li>;
}
