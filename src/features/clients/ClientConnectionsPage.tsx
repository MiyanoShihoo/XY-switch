import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, RefreshCw } from "lucide-react";
import { t, translateMessage, useLanguage } from "../../i18n/language";
import { Badge, Button, Card, Field, InlineError, Input, LoadingState, Select, Toast } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { cancelClientLogin, clearClientLoginInputs, clientAuthorizationUrl, clientRuntimeAvailable, completeClientLogin, disconnectClientProvider, loadClientConnections, loadClientLoginMethods, loadClientSettings, logoutClientAccount, pollClientLogin, readClientAccount, saveClientSettings, startClientLogin, startOpenCodeLogin } from "../../lib/clients-service";
import { loadAppTakeovers, setAppTakeoverEnabled } from "../../lib/takeover-service";
import { accountErrorText, listAccounts } from "../../lib/accounts-service";
import { ACCOUNT_QUERY_KEYS } from "../accounts/account-actions";
import { accountStatusLabel, safeAccountLabel } from "../accounts/account-presentation";
import { errorText } from "../../lib/error";
import type { AppView } from "../../lib/workspace-navigation";
import type { ClientConnection, ClientId, ClientSettings, NativeLoginMethod, NativeLoginProgress, NativeProviderConnection } from "../../types/clients";
import { NativeCodeForm, OpenCodeLoginForm } from "./OpenCodeLoginForm";
import { NativeAccountBindingDialog } from "./NativeAccountBindingDialog";
import { NativeAccountConfirmDialog } from "./NativeAccountConfirmDialog";
import { CLIENT_CONNECTIONS_QUERY_KEY, currentClientConnections, markNativeLoginStarted, nativeCandidateAvailable, nativeClientActionIssue, nativeLoginPending, runNativeAccountOperation } from "./native-account-actions";
import "./clients.css";

type ClientAction = { kind: "read" | "refresh" | "login" | "poll" | "cancel" | "logout" | "gateway" | "methods"; row: ClientConnection; enabled?: boolean }
  | { kind: "opencode-start"; row: ClientConnection; method: NativeLoginMethod; inputs: Record<string, string> }
  | { kind: "complete"; row: ClientConnection; code: string }
  | { kind: "disconnect"; row: ClientConnection; connection: NativeProviderConnection }
  | { kind: "settings"; settings: ClientSettings };
const defaultSettings: ClientSettings = { opencodeConfig: null, opencodeSchema: null, claudeDesktopProfile: null, codexBinary: null, opencodeBinary: null };
const authLabels: Record<string, string> = { chatgpt: "订阅账号", apiKey: "API Key", apikey: "API Key", none: "尚未登录", not_logged_in: "尚未登录", connected_providers: "已连接原生提供商" };
const actionLabels: Record<ClientAction["kind"], string> = { read: "读取账号状态", refresh: "刷新登录状态", login: "设备授权登录", poll: "查看登录进度", cancel: "取消原生登录", logout: "退出客户端账号", gateway: "确认客户端配置变更", settings: "保存客户端路径", methods: "查询原生登录方式", "opencode-start": "提供商授权登录", complete: "提交原生授权码", disconnect: "退出原生提供商连接" };

export function clearSensitiveAction(action: ClientAction | null): void {
  if (action?.kind === "opencode-start") clearClientLoginInputs(action.inputs);
  if (action?.kind === "complete") action.code = "";
}

function confirmationText(action: ClientAction): string {
  if (action.kind === "settings") return t("保存所选配置路径。更改路径后需要重启 XY Switch，才能读取或修改新配置。现有客户端配置不会因此立即切换。");
  const paths = action.row.configPaths.join("\n");
  if (action.kind === "gateway") return (action.enabled ? t("将以下配置连接到当前本地网关，并加密保存原配置以便恢复：") : t("将以下配置恢复为接管前保存的内容；外部修改会触发恢复检查：")) + "\n" + paths;
  if (action.row.configOwner === "opencode") {
    if (action.kind === "opencode-start") return t("授权所选 OpenCode 提供商完成原生登录并保存到当前系统用户的原生账号库。桌面与终端共享该状态；输入不会进入 XY Switch 日志或账号池。") + "\n" + action.method.providerLabel + " · " + action.method.label + "\n" + paths;
    if (action.kind === "complete") return t("将当前页面输入的授权码提交给本工具发起的 OpenCode 原生登录。授权码提交后清除，原生客户端负责保存登录状态。") + "\n" + paths;
    if (action.kind === "disconnect") return t("退出所选提供商的这一原生凭据连接。操作前重新核对连接归属；桌面与终端共享该状态。") + "\n" + action.connection.label + "\n" + paths;
    if (action.kind === "cancel") return t("取消本工具发起的 OpenCode 原生登录，结束自有本地服务并释放配置锁。") + "\n" + paths;
    return t("授权通过已安装 OpenCode 的受保护本地服务访问当前系统用户的原生账号元数据。仅显示提供商和连接状态，不导出邮箱、凭据或用户内容。") + "\n" + paths;
  }
  if (action.kind === "logout") return t("退出当前 CODEX_HOME 的原生账号。Codex 桌面和 CLI 共用该登录状态；本操作会更改原生认证文件：") + "\n" + paths;
  if (action.kind === "login") return t("授权启动 Codex 官方登录进程并写入当前 CODEX_HOME 的认证状态。设备码仅在当前页面显示：") + "\n" + paths;
  if (action.kind === "cancel") return t("取消本工具发起的设备登录并释放配置锁；不会退出已有账号：") + "\n" + paths;
  return t("授权访问当前 CODEX_HOME 的原生账号状态；刷新操作可能更新原生认证状态。页面仅显示登录方式、套餐和可用额度，不显示邮箱或凭据：") + "\n" + paths;
}

export function ClientConnectionsPage({ onNavigate }: { onNavigate?: (view: AppView) => void }) {
  const language = useLanguage();
  const queryClient = useQueryClient();
  const connections = useQuery({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY, queryFn: loadClientConnections });
  const accounts = useQuery({ queryKey: ACCOUNT_QUERY_KEYS.list, queryFn: listAccounts, enabled: clientRuntimeAvailable(), retry: false });
  const settings = useQuery({ queryKey: ["clients", "settings"], queryFn: loadClientSettings });
  const takeovers = useQuery({ queryKey: ["takeovers"], queryFn: loadAppTakeovers });
  const operation = usePageOperation();
  const [draft, setDraft] = useState<ClientSettings>(defaultSettings);
  const [action, setAction] = useState<ClientAction | null>(null);
  const [login, setLogin] = useState<NativeLoginProgress | null>(null);
  const [bindingClientId, setBindingClientId] = useState<ClientId | null>(null);
  const [loginMethods, setLoginMethods] = useState<{ clientId: string; methods: NativeLoginMethod[] } | null>(null);
  useEffect(() => { if (settings.data) setDraft(settings.data); }, [settings.data]);
  useEffect(() => () => clearSensitiveAction(action), [action]);
  useEffect(() => {
    if (login?.status !== "pending" || !connections.isSuccess || operation.busy) return;
    const status = connections.data.find(row => row.id === login.clientId)?.loginStatus;
    if (status && ["completed", "cancelled", "failed", "expired"].includes(status)) {
      setLogin(null);
      void queryClient.invalidateQueries({ queryKey: ACCOUNT_QUERY_KEYS.list });
    }
  }, [connections.data, connections.isSuccess, login, operation.busy, queryClient]);
  const native = clientRuntimeAvailable();
  const pendingConfiguration = connections.data?.some(row => row.configurationPending) ?? false;
  const anyPendingLogin = login?.status === "pending" || (connections.data?.some(row => row.loginStatus === "pending") ?? false);
  const refresh = async () => { await Promise.all([connections.refetch(), takeovers.refetch(), queryClient.invalidateQueries({ queryKey: ACCOUNT_QUERY_KEYS.list })]); };
  const confirm = async () => {
    if (!action) return;
    const approved = action;
    const completed = await operation.run(() => runNativeAccountOperation(queryClient, async () => {
      const rows = currentClientConnections(queryClient);
      if (approved.kind !== "settings" && !["poll", "cancel", "complete", "gateway"].includes(approved.kind)) {
        const issue = nativeClientActionIssue(approved.row.id, rows, login);
        if (issue) throw new Error(issue);
      }
      if (approved.kind === "settings") {
        const next = await saveClientSettings(approved.settings, true);
        queryClient.setQueryData(["clients", "settings"], next);
      } else if (approved.kind === "gateway") {
        if (!approved.row.takeoverTarget) throw new Error(t("此客户端使用原生订阅登录，仅连接共享 MCP。"));
        await setAppTakeoverEnabled(approved.row.takeoverTarget, approved.enabled ?? false);
        await queryClient.invalidateQueries({ queryKey: ["proxy"] });
      } else if (approved.kind === "read" || approved.kind === "refresh") {
        await readClientAccount(approved.row.id, true, approved.kind === "refresh");
      } else if (approved.kind === "logout") {
        await logoutClientAccount(approved.row.id, true);
        setLogin(null);
      } else if (approved.kind === "methods") {
        const methods = await loadClientLoginMethods(approved.row.id, true);
        setLoginMethods({ clientId: approved.row.id, methods });
      } else if (approved.kind === "disconnect") {
        await disconnectClientProvider(approved.row.id, approved.connection.providerId, approved.connection.connectionId, true);
      } else if (approved.kind === "cancel") {
        await cancelClientLogin(approved.row.id, true);
        setLogin(null);
      } else {
        try {
          if (approved.kind === "login" || approved.kind === "opencode-start") markNativeLoginStarted(queryClient, approved.row.id);
          const progress = approved.kind === "login" ? await startClientLogin(approved.row.id, true)
            : approved.kind === "opencode-start" ? await startOpenCodeLogin(approved.row.id, approved.method, approved.inputs, true)
            : approved.kind === "complete" ? await completeClientLogin(approved.row.id, approved.code, true)
            : await pollClientLogin(approved.row.id, true);
          setLogin(progress.status === "pending" ? progress : null);
          if (progress.status === "failed" || progress.status === "expired") throw new Error(t("原生登录未完成或已过期，请重新发起授权。"));
        } finally { clearSensitiveAction(approved); }
      }
      await takeovers.refetch();
    }), t("客户端操作已完成"));
    if (completed || approved.kind === "opencode-start" || approved.kind === "complete") setAction(null);
  };
  const problem = operation.error || (connections.error ? errorText(connections.error) : accounts.error ? accountErrorText(accounts.error) : settings.error ? errorText(settings.error) : takeovers.error ? errorText(takeovers.error) : "");
  const bindingRow = connections.data?.find(row => row.id === bindingClientId);
  const actionIssue = action && action.kind !== "settings" && !["poll", "cancel", "complete", "gateway"].includes(action.kind)
    ? nativeClientActionIssue(action.row.id, connections.isSuccess ? connections.data : undefined, login) : "";
  return <div className="workspace-page clients-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("客户端接入")}</h1><p className="page-description">{t("在同一处管理桌面与终端入口、原生登录、网关配置和共享工具。")}</p></div><div className="page-actions"><Button size="sm" variant="secondary" icon={<RefreshCw size={14} />} loading={connections.isFetching || takeovers.isFetching} disabled={operation.busy} onClick={() => void refresh()}>{t("刷新状态")}</Button></div></header>
    <InlineError message={problem} onRetry={() => void refresh()} retrying={connections.isFetching} />
    {operation.message && <Toast title={operation.message} onDismiss={() => operation.setMessage("")} />}
    {!native && <p className="workspace-route-notice">{t("浏览器预览仅展示入口；真实安装、登录、额度和配置需要在桌面应用中验证。")}</p>}
    {pendingConfiguration && <p className="workspace-route-notice" role="status">{t("客户端路径已保存，请重启 XY Switch 后继续操作。")}</p>}
    <p className="page-description">{t("Codex 桌面与 CLI 共用 CODEX_HOME；OpenCode 桌面与终端共用所选配置。发现配置文件不代表客户端正在运行或已经验收。")}</p>
    {connections.isLoading ? <LoadingState label={t("读取客户端入口…")} /> : <div className="client-grid">{(connections.data ?? []).map(row => {
      const takeover = takeovers.data?.find(item => item.app === row.takeoverTarget);
      const codex = row.loginMethod === "codex_device_code";
      const opencode = row.loginMethod === "opencode_oauth";
      const pendingLogin = nativeLoginPending(row.id, connections.data, login);
      const disabled = !native || operation.busy || row.configurationPending || bindingClientId !== null;
      const nativeBlocked = Boolean(nativeClientActionIssue(row.id, connections.isSuccess ? connections.data : undefined, login));
      const candidates = row.account?.poolCandidates ?? [];
      const canBind = candidates.some(candidate => nativeCandidateAvailable(candidate, Math.floor(Date.now() / 1000)));
      const authorizationUrl = login?.clientId === row.id ? clientAuthorizationUrl(login.verificationUrl, row.id) : null;
      return <Card key={row.id} className="client-card">
        <div className="client-card__heading"><h2>{row.label}</h2><Badge tone={row.configurationPending ? "warning" : "neutral"}>{row.configurationPending ? t("待重启") : row.nativeVerification === "preview" ? t("预览") : t("待原生验收")}</Badge></div>
        <p>{translateMessage(row.loginInstructions)}</p>
        <dl className="client-details"><div><dt>{t("配置归属")}</dt><dd>{row.configOwner}</dd></div><div><dt>{t("配置文件")}</dt><dd>{row.configDetected ? t("已发现配置") : t("未发现配置")}</dd></div><div><dt>{t("网关状态")}</dt><dd>{takeover?.active ? t("配置已接管，连接待验证") : row.takeoverTarget ? t("未接管") : t("原生订阅与共享 MCP")}</dd></div></dl>
        {row.configPaths.length > 0 && <details className="details-group"><summary>{t("查看实际配置路径")}</summary><div className="details-group__body">{row.configPaths.map(path => <code className="client-config-path" key={path}>{path}</code>)}</div></details>}
        {(row.setupError || takeover?.lastError) && <InlineError message={translateMessage(row.setupError || takeover?.lastError || "")} />}
        {row.account ? <section className="client-account" aria-label={t("原生账号状态")}><strong>{t(authLabels[row.account.authMode] ?? "原生账号状态已读取")}{row.account.plan ? " · " + row.account.plan : ""}</strong><span>{t("读取时间")} {new Date(row.account.checkedAt * 1000).toLocaleString(language === "en" ? "en-US" : "zh-CN")}</span>{row.account.quotas.length ? row.account.quotas.map((quota, index) => <span key={index}>{t("已用额度")} {quota.usedPercent === null ? t("未知") : quota.usedPercent.toFixed(1) + "%"}{quota.windowMinutes !== null ? " · " + quota.windowMinutes + t("分钟窗口") : ""}{quota.resetAt !== null ? " · " + t("重置于") + " " + new Date(quota.resetAt * 1000).toLocaleString() : ""}</span>) : <span>{t("额度未知；原生接口未提供可用数据。")}</span>}</section> : <p className="muted">{t(codex ? "尚未授权读取账号；额度未知。" : "账号在原生客户端登录；XY Switch 未读取其凭据。")}</p>}
        {row.account?.providers?.length ? <ul className="client-providers" aria-label={t("原生提供商连接")}>
          {row.account.providers.map((connection, index) => <li key={JSON.stringify([connection.providerId, connection.connectionId, index])}>
            <span>{connection.label}</span>
            {connection.disconnectable ? <Button variant="quiet" size="sm" disabled={disabled || pendingLogin} onClick={() => setAction({ kind: "disconnect", row, connection })}>{t("退出此连接")}</Button> : <span className="muted">{t("由环境配置提供，仅显示状态")}</span>}
          </li>)}
        </ul> : null}
        {candidates.some(candidate => candidate.boundAccountId !== null) && <div className="client-bindings" aria-label={t("原生账号绑定状态")}>
          {candidates.filter(candidate => candidate.boundAccountId !== null).map(candidate => {
            const bound = accounts.data?.find(account => account.id === candidate.boundAccountId);
            return <span key={candidate.id}>{t("已绑定")} · {bound ? safeAccountLabel(bound.alias) + " · " + accountStatusLabel(bound.status, bound.kind) : t("原生账号引用")}</span>;
          })}
        </div>}
        {pendingLogin && <p className="workspace-route-notice">{t("原生授权正在等待完成，可查看进度或取消。")}</p>}
        {loginMethods?.clientId === row.id && <OpenCodeLoginForm methods={loginMethods.methods} disabled={disabled || pendingLogin} onCancel={() => setLoginMethods(null)} onStart={(method, inputs) => { setLoginMethods(null); setAction({ kind: "opencode-start", row, method, inputs }); }} />}
        {login?.clientId === row.id && <section className="client-login" aria-label={t("原生授权")}>
          <strong>{t(codex ? "设备授权码" : "提供商授权登录")}</strong>
          {login.userCode && <code>{login.userCode}</code>}
          {login.instructions && <p className="client-login__instructions">{login.instructions}</p>}
          {authorizationUrl && <a href={authorizationUrl} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">{t("打开官方授权页面")} <ExternalLink size={12} /></a>}
          {login.requiresCode && <NativeCodeForm disabled={disabled} onSubmit={code => setAction({ kind: "complete", row, code })} />}
          <span>{t("授权码只保存在当前页面内存中，离开页面后可重新查看登录进度。")}</span>
        </section>}
        <div className="client-actions">
          {(codex || opencode) && <>
            <Button variant="secondary" size="sm" disabled={disabled || nativeBlocked} onClick={() => setAction({ kind: "read", row })}>{t("读取账号")}</Button>
            {codex && <Button variant="secondary" size="sm" disabled={disabled || nativeBlocked} onClick={() => setAction({ kind: "refresh", row })}>{t("刷新登录状态")}</Button>}
            {row.account && <Button variant="secondary" size="sm" disabled={disabled || nativeBlocked || !accounts.isSuccess || !canBind} onClick={() => { operation.setError(""); setBindingClientId(row.id); }}>{t("绑定原生账号")}</Button>}
            <Button size="sm" disabled={disabled || pendingLogin || Boolean(row.setupError)} onClick={() => setAction({ kind: codex ? "login" : "methods", row })}>{t(codex ? "设备授权登录" : "提供商授权登录")}</Button>
            {pendingLogin && <><Button variant="secondary" size="sm" disabled={disabled} onClick={() => setAction({ kind: "poll", row })}>{t("查看登录进度")}</Button><Button variant="quiet" size="sm" disabled={disabled} onClick={() => setAction({ kind: "cancel", row })}>{t("取消原生登录")}</Button></>}
            {codex && <Button variant="quiet" size="sm" disabled={disabled || pendingLogin} onClick={() => setAction({ kind: "logout", row })}>{t("退出账号")}</Button>}
          </>}
          {row.takeoverTarget && <Button variant="secondary" size="sm" disabled={disabled || pendingLogin || Boolean(row.setupError)} onClick={() => setAction({ kind: "gateway", row, enabled: !takeover?.active })}>{t(takeover?.active ? "恢复原配置" : "连接本地网关")}</Button>}
          <Button variant="quiet" size="sm" disabled={!onNavigate} onClick={() => onNavigate?.("mcp")}>{t("配置共享 MCP")}</Button>
        </div>
      </Card>;
    })}</div>}
    <Card className="client-settings">
      <h2>{t("客户端路径")}</h2>
      <p className="page-description">{t("可选择 OpenCode 的实际 JSON/JSONC 配置、Claude Desktop 已保存的第三方配置，以及 Codex 和 OpenCode 原生可执行文件。留空使用系统默认位置。")}</p>
      <div className="client-settings__fields">
        <Field label={t("OpenCode 配置文件")} htmlFor="client-opencode-config"><Input id="client-opencode-config" value={draft.opencodeConfig ?? ""} disabled={operation.busy || pendingConfiguration || anyPendingLogin || !native} onChange={event => setDraft({ ...draft, opencodeConfig: event.target.value.trim() || null })} /></Field>
        <Field label={t("OpenCode 配置版本")} htmlFor="client-opencode-schema"><Select id="client-opencode-schema" value={draft.opencodeSchema ?? ""} disabled={operation.busy || pendingConfiguration || anyPendingLogin || !native} onChange={event => setDraft({ ...draft, opencodeSchema: (event.target.value || null) as ClientSettings["opencodeSchema"] })}><option value="">{t("从现有配置识别")}</option><option value="v1">V1</option><option value="v2">V2</option></Select></Field>
        <Field label={t("Claude Desktop 第三方配置文件")} htmlFor="client-claude-profile"><Input id="client-claude-profile" value={draft.claudeDesktopProfile ?? ""} disabled={operation.busy || pendingConfiguration || anyPendingLogin || !native} onChange={event => setDraft({ ...draft, claudeDesktopProfile: event.target.value.trim() || null })} /></Field>
        <Field label={t("Codex CLI 可执行文件")} htmlFor="client-codex-binary"><Input id="client-codex-binary" value={draft.codexBinary ?? ""} disabled={operation.busy || pendingConfiguration || anyPendingLogin || !native} onChange={event => setDraft({ ...draft, codexBinary: event.target.value.trim() || null })} /></Field>
        <Field label={t("OpenCode 原生可执行文件")} htmlFor="client-opencode-binary"><Input id="client-opencode-binary" value={draft.opencodeBinary ?? ""} disabled={operation.busy || pendingConfiguration || anyPendingLogin || !native} onChange={event => setDraft({ ...draft, opencodeBinary: event.target.value.trim() || null })} /></Field>
      </div>
      {anyPendingLogin && <p className="workspace-route-notice">{t("原生登录期间锁定客户端路径，请先完成或取消登录。")}</p>}
      <Button size="sm" disabled={!native || operation.busy || pendingConfiguration || anyPendingLogin || settings.isLoading} onClick={() => setAction({ kind: "settings", settings: { ...draft } })}>{t("保存路径并在重启后应用")}</Button>
    </Card>
    {bindingRow && <NativeAccountBindingDialog row={bindingRow} rows={connections.isSuccess ? connections.data : undefined} login={login} accountsReady={accounts.isSuccess}
      onClose={() => setBindingClientId(null)} onBound={() => { setBindingClientId(null); operation.setMessage(t("原生账号已绑定，状态与额度已同步。")); }} />}
    {action && <NativeAccountConfirmDialog title={t(actionLabels[action.kind])} message={confirmationText(action)} confirmLabel={t("授权并继续")} busy={operation.busy} disabled={Boolean(actionIssue)} error={actionIssue || operation.error} onCancel={() => { clearSensitiveAction(action); setAction(null); }} onConfirm={() => void confirm()} />}
  </div>;
}
