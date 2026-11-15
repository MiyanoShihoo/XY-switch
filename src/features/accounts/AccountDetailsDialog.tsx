import { t, useLanguage } from "../../i18n/language";
import { Button, InlineError, Modal } from "../../ui";
import type { AccountInfo } from "../../types/accounts";
import type { ClientConnection } from "../../types/clients";
import { nativeAccountCanSync, nativeAccountClientId, nativeClientActionIssue } from "../clients/native-account-actions";
import { AccountQuotaView, accountCanRefresh, accountKindLabel, accountStatusLabel, accountTime, authorizationLabel, nativeAccountScopeMessage, safeAccountLabel } from "./account-presentation";

interface AccountDetailsDialogProps {
  account: AccountInfo;
  providerLabel: string;
  nativeRows: ClientConnection[] | undefined;
  timestamp: number;
  disabled: boolean;
  settingsDisabled?: boolean;
  accountsReady: boolean;
  busy: boolean;
  error?: string;
  onClose: () => void;
  onEdit: (mode: "edit" | "replace") => void;
  onRefresh: () => void;
  onNativeSync: () => void;
  onRevoke: () => void;
}

export function AccountDetailsDialog({ account, providerLabel, nativeRows, timestamp, disabled, settingsDisabled = disabled, accountsReady, busy, error,
  onClose, onEdit, onRefresh, onNativeSync, onRevoke }: AccountDetailsDialogProps) {
  const language = useLanguage();
  const expiresAt = accountTime(account.expiresAt, language);
  const expired = expiresAt !== t("未知") && account.expiresAt !== null && account.expiresAt <= timestamp;
  const nativeId = account.kind === "nativeManaged" ? nativeAccountClientId(account.client) : null;
  const nativeIssue = nativeId ? nativeClientActionIssue(nativeId, nativeRows) : "";
  return <div className="accounts-dialog"><Modal open size="lg" className="accounts-detail-modal" title={safeAccountLabel(account.alias)} description={providerLabel} busy={busy} onClose={onClose} footer={<>
    <Button variant="secondary" disabled={settingsDisabled || account.status === "revoked"} onClick={() => onEdit("edit")}>{account.destinationApproved === true ? t("账号设置") : t("重新批准请求地址")}</Button>
    {account.kind === "apiKey" && <Button variant="secondary" disabled={settingsDisabled || account.status === "revoked"} onClick={() => onEdit("replace")}>{t("更新 API key")}</Button>}
    {account.kind === "oauth" && <Button disabled={disabled || !accountCanRefresh(account, timestamp)} onClick={onRefresh}>{t("刷新凭据")}</Button>}
    {nativeId && <Button disabled={disabled || !accountsReady || !nativeAccountCanSync(account, nativeRows)} onClick={onNativeSync}>{account.client === "codexDesktop" || account.client === "codexCli" ? t("通过 Codex 刷新") : t("同步 OpenCode 状态")}</Button>}
  </>}>
    <div className="accounts-detail">
      <div className="accounts-detail__summary"><span>{accountKindLabel(account.kind)}</span><span className="accounts-status" data-status={account.status}>{accountStatusLabel(account.status, account.kind)}</span></div>
      <InlineError message={error} />
      {account.destinationApproved !== true && account.status !== "revoked" && <p className="accounts-notice">{t("请求地址待批准；请在账号设置中检查供应商并重新确认授权。")}</p>}
      {nativeIssue && <p className="accounts-notice" role="status">{nativeIssue}</p>}
      {account.cleanupPending && <p className="accounts-notice" role="status">{account.status === "revoked"
        ? t("安全存储清理待完成；本地引用保持撤销，可重试清理。")
        : t("旧凭据的安全存储清理待完成；当前账号引用仍有效。")}</p>}
      {account.kind === "nativeManaged" && <p className="accounts-detail-note">{nativeAccountScopeMessage()}</p>}
      <div className="accounts-detail__columns">
      <section className="accounts-detail-section" aria-labelledby="account-authorization-heading">
        <h3 id="account-authorization-heading">{t("授权与状态")}</h3>
        <dl className="accounts-metadata">
          <div><dt>{t("凭据类型")}</dt><dd>{accountKindLabel(account.kind)}</dd></div>
          <div><dt>{t("绑定供应商")}</dt><dd>{providerLabel}</dd></div>
          <div><dt>{t("授权状态")}</dt><dd>{authorizationLabel(account.authorizationStatus)}</dd></div>
          <div><dt>{t("请求地址授权")}</dt><dd>{account.destinationApproved === true ? t("当前请求地址已批准") : t("请求地址待批准")}</dd></div>
          <div><dt>{t("来源")}</dt><dd>{account.source === "selectedFile" ? t("用户指定文件") : account.source === "nativeClient" ? t("原生客户端") : t("手动添加")}</dd></div>
          <div><dt>{t("凭据有效期")}</dt><dd className="accounts-numeric" data-expired={expired}>{expiresAt}{expired ? " · " + t("已过期") : ""}</dd></div>
          <div><dt>{t("冷却截止")}</dt><dd className="accounts-numeric">{account.cooldownUntil !== null && account.cooldownUntil > timestamp ? accountTime(account.cooldownUntil, language) : t("无有效冷却")}</dd></div>
        </dl>
        {account.kind === "nativeManaged" && (!nativeId || account.status === "loginRequired") && <p className="accounts-detail-note">{t("请在对应原生客户端完成登录或更新授权。")}</p>}
        {account.kind === "oauth" && !account.canRefresh && account.status !== "revoked" && <p className="accounts-detail-note">{t("此账号尚无可用的授权刷新接口，请重新授权或等待客户端接入。")}</p>}
      </section>
      <section className="accounts-detail-section accounts-detail__quota" aria-labelledby="account-quota-heading">
        <h3 id="account-quota-heading">{t("额度明细")}</h3>
        <AccountQuotaView quota={account.quota} timestamp={timestamp} />
        {account.availableModels.length > 0 && <div className="accounts-models"><h4>{t("账号声明的模型")}</h4><ul>{account.availableModels.map(model => <li key={model}><code>{model}</code></li>)}</ul></div>}
      </section>
      </div>
      {account.kind === "oauth" && <section className="accounts-authorization-note" aria-labelledby="account-boundary-heading">
        <h3 id="account-boundary-heading">{t("授权能力边界")}</h3>
        <p>{t("OAuth 需要合法应用注册及推理授权。当前面板仅导入明确授权的 OAuth 凭据；PKCE 登录和 SIWC 订阅推理尚待接入。")}</p>
        <p>{t("刷新仅在已注册的授权接口可用时启用。原生客户端令牌不会导出或移用于订阅推理。")}</p>
      </section>}
      <section className="accounts-cleanup" aria-labelledby="account-cleanup-heading">
        <div><h3 id="account-cleanup-heading">{t("本地引用与清理")}</h3><p className="accounts-detail-note">{t("撤销本应用的账号引用并清理本应用保存的凭据。不会撤销供应商授权、退出原生客户端或删除来源文件。")}</p></div>
        <Button variant="danger" size="sm" disabled={disabled || (account.status === "revoked" && !account.cleanupPending)} onClick={onRevoke}>{account.status === "revoked" && account.cleanupPending ? t("重试本地清理") : t("撤销本地引用")}</Button>
      </section>
    </div>
  </Modal></div>;
}
