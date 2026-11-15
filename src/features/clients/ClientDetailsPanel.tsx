import { useEffect, useState } from "react";
import { ExternalLink, Monitor, Terminal } from "lucide-react";
import { t, translateMessage, useLanguage } from "../../i18n/language";
import { Button, InlineError } from "../../ui";
import { clientAuthorizationUrl } from "../../lib/clients-service";
import type { AppView } from "../../lib/workspace-navigation";
import type { AccountInfo } from "../../types/accounts";
import type { ClientConnection, ClientSettings, NativeLoginMethod, NativeLoginProgress, NativeProviderConnection } from "../../types/clients";
import type { AppTakeoverStatus } from "../../types/takeover";
import { accountStatusLabel, accountTime, nativeAccountScopeMessage, safeAccountLabel } from "../accounts/account-presentation";
import { NativeCodeForm, OpenCodeLoginForm } from "./OpenCodeLoginForm";
import { nativeCandidateAvailable, nativeClientActionIssue, nativeLoginPending } from "./native-account-actions";

export type ClientAction = { kind: "read" | "refresh" | "login" | "poll" | "cancel" | "logout" | "gateway" | "methods"; row: ClientConnection; enabled?: boolean }
  | { kind: "opencode-start"; row: ClientConnection; method: NativeLoginMethod; inputs: Record<string, string> }
  | { kind: "complete"; row: ClientConnection; code: string }
  | { kind: "disconnect"; row: ClientConnection; connection: NativeProviderConnection }
  | { kind: "settings"; settings: ClientSettings };

const authLabels: Record<string, string> = { chatgpt: "订阅账号", apiKey: "API Key", apikey: "API Key", none: "尚未登录", not_logged_in: "尚未登录", connected_providers: "已连接原生提供商" };
const protocolLabels: Record<string, string> = { responses_gateway: "Responses API", messages_gateway: "Messages API", chat_gateway: "Chat Completions API", gemini_gateway: "Gemini API" };
export const nativeLoginEndLabels: Partial<Record<string, string>> = { expired: "原生授权已过期", failed: "原生授权未完成", cancelled: "原生授权已取消" };

interface ClientDetailsPanelProps {
  row: ClientConnection;
  rows: ClientConnection[] | undefined;
  accounts: AccountInfo[];
  accountsReady: boolean;
  takeover?: AppTakeoverStatus;
  login: NativeLoginProgress | null;
  loginMethods: NativeLoginMethod[] | null;
  disabled: boolean;
  busy: boolean;
  error?: string;

  onAction: (action: ClientAction) => void;
  onBind: () => void;
  onCloseMethods: () => void;
  onOpenPaths: () => void;
  onNavigate?: (view: AppView) => void;
}

export function ClientDetailsPanel({ row, rows, accounts, accountsReady, takeover, login, loginMethods, disabled, busy, error,
  onAction, onBind, onCloseMethods, onOpenPaths, onNavigate }: ClientDetailsPanelProps) {
  const language = useLanguage();
  const [timestamp, setTimestamp] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const timer = window.setInterval(() => setTimestamp(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const codex = row.loginMethod === "codex_device_code";
  const opencode = row.loginMethod === "opencode_oauth";
  const loginIsPrimary = !row.account || ["none", "not_logged_in"].includes(row.account.authMode);
  const ClientIcon = row.id.includes("desktop") ? Monitor : Terminal;
  const pendingLogin = nativeLoginPending(row.id, rows, login);
  const nativeBlocked = Boolean(nativeClientActionIssue(row.id, rows, login));
  const candidates = row.account?.poolCandidates ?? [];
  const canBind = candidates.some(candidate => nativeCandidateAvailable(candidate, timestamp));
  const authorizationUrl = login?.clientId === row.id && login.status === "pending" ? clientAuthorizationUrl(login.verificationUrl, row.id) : null;
  const endedLogin = nativeLoginEndLabels[(login?.clientId === row.id ? login.status : row.loginStatus) ?? ""];
  const checkedAt = row.account?.checkedAt;
  const quotas = row.account?.quotas.filter(quota => quota.usedPercent !== null && Number.isFinite(quota.usedPercent)
    && quota.usedPercent >= 0 && quota.usedPercent <= 100
    && (quota.resetAt === null || (Number.isSafeInteger(quota.resetAt) && quota.resetAt > row.account!.checkedAt && quota.resetAt <= 253402300799))) ?? [];
  const quotaValidUntil = typeof checkedAt === "number" && Number.isSafeInteger(checkedAt) && checkedAt >= 0 && checkedAt <= timestamp + 5 && quotas.length
    ? quotas.reduce((expiresAt, quota) => Math.min(expiresAt, quota.resetAt ?? expiresAt), checkedAt + 60) : null;
  const quotaExpired = quotaValidUntil !== null && quotaValidUntil <= timestamp;
  const configurationLabel = row.nativeVerification === "preview" ? t("预览，未读取")
    : !row.takeoverTarget ? t("原生订阅") : takeover ? t(takeover.active ? "本地网关" : "原生配置") : t("尚未读取");
  return <section className="client-inspector" id="client-inspector" aria-labelledby="client-inspector-name" aria-busy={busy}>
    <header className="client-inspector__header">
      <ClientIcon size={24} aria-hidden="true" />
      <div className="client-inspector__identity"><p>{t("正在查看")} · {t(row.id.includes("desktop") ? "桌面应用" : "命令行工具")}</p><h2 id="client-inspector-name" tabIndex={-1}>{row.label}</h2></div>
      {row.nativeVerification === "preview" && <span className="client-inspector__status">{t("浏览器预览")}</span>}
    </header>
    <div className="client-detail-panel">
      {row.configurationPending && <p className="workspace-route-notice" role="status">{t("客户端路径已保存，请重启 XY Switch 后继续操作。")}</p>}
      <InlineError message={[error, row.setupError, takeover?.lastError].filter(Boolean).map(message => translateMessage(message!)).join(" · ")} />
      {pendingLogin && <p className="workspace-route-notice" role="status">{t("原生授权正在等待完成，可查看进度或取消。")}</p>}
      {!pendingLogin && endedLogin && <p className="workspace-route-notice" role="status">{t(endedLogin)} · {t("可重新发起原生授权。")}</p>}
      {(codex || opencode) && <div className="client-account-toolbar">
        <span className="client-toolbar-label">{t("账号与登录")}</span>
        <div className="client-actions" role="group" aria-label={t("账号与登录")}>
          {pendingLogin ? <>
            <Button size="sm" disabled={disabled} onClick={() => onAction({ kind: "poll", row })}>{t("查看登录进度")}</Button>
            <Button variant="quiet" size="sm" disabled={disabled} onClick={() => onAction({ kind: "cancel", row })}>{t("取消原生登录")}</Button>
          </> : <Button variant={loginIsPrimary ? "primary" : "secondary"} size="sm" disabled={disabled || Boolean(row.setupError)} onClick={() => onAction({ kind: codex ? "login" : "methods", row })}>{t(codex ? "设备授权登录" : "提供商授权登录")}</Button>}
          <Button variant="secondary" size="sm" disabled={disabled || nativeBlocked} onClick={() => onAction({ kind: "read", row })}>{t("读取账号")}</Button>
          {row.account && <Button variant="secondary" size="sm" disabled={disabled || nativeBlocked || !accountsReady || !canBind} onClick={onBind}>{t("绑定原生账号")}</Button>}
          {codex && <Button variant="quiet" size="sm" disabled={disabled || nativeBlocked} onClick={() => onAction({ kind: "refresh", row })}>{t("刷新登录状态")}</Button>}
        </div>
      </div>}
      <div className="client-detail-layout">
      <section className="client-detail-section client-routing-section" aria-labelledby="client-connection-heading">
        <div className="client-section-heading"><h3 id="client-connection-heading">{t("连接与配置")}</h3></div>
        <div className="client-connection-controls">
          <dl className="client-route-summary" aria-label={t("接入状态")}>
            <div><dt>{t("当前配置")}</dt><dd>{configurationLabel}</dd></div>
            <div><dt>{t("客户端运行")}</dt><dd>{t("未检测")}</dd></div>
            <div><dt>{t("请求验证")}</dt><dd>{t("尚未验证")}</dd></div>
          </dl>
          <p className="client-detail-note" id="client-connection-evidence">{t("配置状态仅说明接入方式；运行与请求尚无当前证据，历史成功不能证明当前连通。")}</p>
          <div className="client-actions" role="group" aria-label={t("连接与配置")} aria-describedby="client-connection-evidence">
            {row.takeoverTarget && <Button variant="secondary" disabled={disabled || pendingLogin || Boolean(row.setupError) || !takeover} onClick={() => onAction({ kind: "gateway", row, enabled: !takeover?.active })}>{t(takeover?.active ? "恢复原配置" : "连接本地网关")}</Button>}
            <Button variant="quiet" disabled={busy || !onNavigate} onClick={() => onNavigate?.("mcp")}>{t("配置共享 MCP")}</Button>
          </div>
          {row.takeoverTarget && !takeover && row.nativeVerification !== "preview" && <p className="client-detail-note">{t("当前配置尚未读取，刷新状态后再修改接入方式。")}</p>}
        </div>

        <p className="client-detail-note">{translateMessage(row.loginInstructions)}</p>
      </section>
      {(codex || opencode) && <section className="client-detail-section client-identity-section" aria-labelledby="client-account-heading">
        <h3 id="client-account-heading">{t("账号与登录")}</h3>

        {row.account ? <section className="client-account" aria-label={t("原生账号状态")}>
          <dl className="client-details">
            <div><dt>{t("登录方式")}</dt><dd>{t(authLabels[row.account.authMode] ?? "原生账号状态已读取")}</dd></div>
            {row.account.plan && <div><dt>{t("套餐")}</dt><dd>{row.account.plan}</dd></div>}
            <div><dt>{t("读取时间")}</dt><dd className="client-numeric">{accountTime(row.account.checkedAt, language)}</dd></div>
          </dl>
          <section className="client-quota" aria-label={t("原生额度")}><h4>{t("原生额度")}</h4>
            {quotaValidUntil !== null && !quotaExpired ? <>
              {quotas.map((quota, index) => <div className="client-quota__row" key={index}>
                <span>{t("已用额度")} <strong className="client-numeric">{quota.usedPercent === null ? t("未知") : quota.usedPercent.toFixed(1) + "%"}</strong>{quota.windowMinutes !== null && Number.isSafeInteger(quota.windowMinutes) && quota.windowMinutes > 0 && quota.windowMinutes <= 525600 ? " · " + quota.windowMinutes + t("分钟窗口") : ""}</span>
                {quota.resetAt !== null && <span className="client-detail-note client-numeric">{t("重置于")} {accountTime(quota.resetAt, language)}</span>}
              </div>)}
              <p className="client-detail-note client-numeric">{t("证据有效至：{time}", { time: accountTime(quotaValidUntil, language) })}</p>
            </> : <span>{quotaExpired ? t("额度未知；原生额度证据已过期，请重新读取账号。") : t("额度未知；原生接口未提供可用数据。")}</span>}
          </section>
        </section> : <p className="client-detail-note">{t(codex ? "尚未授权读取账号；额度未知。" : "账号在原生客户端登录；XY Switch 未读取其凭据。")}</p>}
        {candidates.some(candidate => candidate.boundAccountId !== null) && <div className="client-bindings" aria-label={t("原生账号绑定状态")}>
          {candidates.filter(candidate => candidate.boundAccountId !== null).map(candidate => {
            const bound = accounts.find(account => account.id === candidate.boundAccountId);
            return <span key={candidate.id}>{t("已绑定")} · {bound ? safeAccountLabel(bound.alias) + " · " + accountStatusLabel(bound.status, bound.kind) : t("原生账号引用")}</span>;
          })}
        </div>}
        {loginMethods && <OpenCodeLoginForm methods={loginMethods} disabled={disabled || pendingLogin} onCancel={onCloseMethods} onStart={(method, inputs) => { onCloseMethods(); onAction({ kind: "opencode-start", row, method, inputs }); }} />}
        {login?.clientId === row.id && login.status === "pending" && <section className="client-login" aria-label={t("原生授权")}>
          <strong>{t(codex ? "设备授权码" : "提供商授权登录")}</strong>
          {login.userCode && <code className="client-login__code">{login.userCode}</code>}
          {login.instructions && <p className="client-login__instructions">{login.instructions}</p>}
          {authorizationUrl && <a href={authorizationUrl} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">{t("打开官方授权页面")} <ExternalLink size={12} /></a>}
          {login.requiresCode && <NativeCodeForm disabled={disabled} onSubmit={code => onAction({ kind: "complete", row, code })} />}
          <span className="client-detail-note">{t("授权码只保存在当前页面内存中，离开页面后可重新查看登录进度。")}</span>
        </section>}

        <p className="client-detail-note">{nativeAccountScopeMessage()}</p>
        {(codex || Boolean(row.account?.providers?.length)) && <section className="client-session-controls" aria-label={t("刷新与退出")}>
          {codex && <div className="client-actions">

            <Button variant="quiet" size="sm" disabled={disabled || pendingLogin} onClick={() => onAction({ kind: "logout", row })}>{t("退出账号")}</Button>
          </div>}
          {row.account?.providers?.length ? <ul className="client-providers" aria-label={t("原生提供商连接")}>
            {row.account.providers.map((connection, index) => <li key={JSON.stringify([connection.providerId, connection.connectionId, index])}>
              <span>{connection.label}</span>
              {connection.disconnectable ? <Button variant="quiet" size="sm" disabled={disabled || pendingLogin} onClick={() => onAction({ kind: "disconnect", row, connection })}>{t("退出此连接")}</Button> : <span className="client-detail-note">{t("由环境配置提供，仅显示状态")}</span>}
            </li>)}
          </ul> : null}
        </section>}
      </section>}
      <details className="client-configuration">
        <summary><span>{t("配置与路径")}</span><span className="client-detail-note">{t("查看实际配置路径")}</span></summary>
        <div className="client-configuration-body">
          <div className="client-section-heading"><h3>{t("配置元数据")}</h3><Button variant="quiet" size="sm" disabled={busy} onClick={onOpenPaths}>{t("客户端路径")}</Button></div>
          <dl className="client-details"><div><dt>{t("配置归属")}</dt><dd><code>{row.configOwner || t("未提供配置归属")}</code></dd></div><div><dt>{t("客户端标识")}</dt><dd><code>{row.id}</code></dd></div></dl>
        <dl className="client-route-summary">
          <div><dt>{t(row.takeoverTarget ? "可用网关接口" : "接入方式")}</dt><dd>{row.takeoverTarget ? <code>{protocolLabels[row.inferenceMode] ?? t("未知")}</code> : t("原生订阅与共享 MCP")}</dd></div>
          <div><dt>{t("配置文件")}</dt><dd>{row.nativeVerification === "preview" ? t("预览，未读取") : row.configDetected ? t("已发现配置") : t("未发现配置")}</dd></div>
        </dl>
          {row.configPaths.length > 0 ? <div className="client-config-paths"><h4>{t("实际配置路径")}</h4>{row.configPaths.map(configPath => <code className="client-config-path" key={configPath}>{configPath}</code>)}</div>
            : <p className="client-detail-note">{t("尚未提供配置路径。")}</p>}
          <p className="client-detail-note">{t("Codex 桌面与 CLI 共用 CODEX_HOME；OpenCode 桌面与终端共用所选配置。发现配置文件不代表客户端正在运行或已经验收。")}</p>
        </div>
      </details>
      </div>
    </div>
  </section>;
}
