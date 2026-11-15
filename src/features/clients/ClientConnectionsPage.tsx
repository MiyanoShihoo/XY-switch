import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, RefreshCw } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { Button, InlineError, LoadingState, Toast } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { cancelClientLogin, clearClientLoginInputs, clientRuntimeAvailable, completeClientLogin, disconnectClientProvider, loadClientConnections, loadClientLoginMethods, loadClientSettings, logoutClientAccount, pollClientLogin, readClientAccount, saveClientSettings, startClientLogin, startOpenCodeLogin } from "../../lib/clients-service";
import { loadAppTakeovers, setAppTakeoverEnabled } from "../../lib/takeover-service";
import { accountErrorText, listAccounts } from "../../lib/accounts-service";
import { ACCOUNT_QUERY_KEYS } from "../accounts/account-actions";
import { errorText } from "../../lib/error";
import type { AppView } from "../../lib/workspace-navigation";
import type { ClientConnection, ClientId, ClientSettings, NativeLoginMethod, NativeLoginProgress } from "../../types/clients";
import type { AppTakeoverStatus } from "../../types/takeover";
import { ClientDetailsPanel, type ClientAction } from "./ClientDetailsPanel";
import { ClientDirectory } from "./ClientDirectory";
import { ClientPathSettingsDialog } from "./ClientPathSettingsDialog";
import { NativeAccountBindingDialog } from "./NativeAccountBindingDialog";
import { NativeAccountConfirmDialog } from "./NativeAccountConfirmDialog";
import { CLIENT_CONNECTIONS_QUERY_KEY, currentClientConnections, markNativeLoginStarted, nativeClientActionIssue, nativeLoginPending, runNativeAccountOperation } from "./native-account-actions";
import { clientConfigurationLabel, groupClientConfigurations } from "./client-configuration";
import "./clients.css";

const defaultSettings: ClientSettings = { opencodeConfig: null, opencodeSchema: null, claudeDesktopProfile: null, codexBinary: null, opencodeBinary: null };
const actionLabels: Record<ClientAction["kind"], string> = { read: "读取账号状态", refresh: "刷新登录状态", login: "设备授权登录", poll: "查看登录进度", cancel: "取消原生登录", logout: "退出客户端账号", gateway: "确认客户端配置变更", settings: "保存客户端路径", methods: "查询原生登录方式", "opencode-start": "提供商授权登录", complete: "提交原生授权码", disconnect: "退出原生提供商连接" };

export function clearSensitiveAction(action: ClientAction | null): void {
  if (action?.kind === "opencode-start") clearClientLoginInputs(action.inputs);
  if (action?.kind === "complete") action.code = "";
}

export function clientGatewayActionIssue(approved: ClientAction, rows: ClientConnection[] | undefined, takeovers: AppTakeoverStatus[] | undefined, login: NativeLoginProgress | null): string {
  if (approved.kind !== "gateway") return "";
  const row = rows?.find(item => item.id === approved.row.id);
  const takeover = takeovers?.find(item => item.app === row?.takeoverTarget);
  if (!row || !takeover || !row.takeoverTarget) return t("请先读取客户端配置状态，再确认连接或恢复。");
  if (nativeLoginPending(row.id, rows, login)) return t("原生登录进行中，请完成或取消登录后再读取、绑定或刷新账号。");
  const group = groupClientConfigurations(rows ?? []).find(item => item.rows.some(member => member.id === row.id));
  if (group?.rows.some(member => member.configurationPending)) return t("客户端路径已保存，请重启 XY Switch 后继续操作。");
  if (row.setupError || row.configOwner !== approved.row.configOwner || row.takeoverTarget !== approved.row.takeoverTarget
    || JSON.stringify(row.configPaths) !== JSON.stringify(approved.row.configPaths) || takeover.active === approved.enabled) {
    return t("客户端配置状态已改变，请刷新并重新确认。");
  }
  return "";
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

export function ClientConnectionsPage({ onNavigate, configurationControl }: { onNavigate?: (view: AppView) => void; configurationControl?: ReactNode }) {
  useLanguage();
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
  const [selectedClientId, setSelectedClientId] = useState<ClientId | null>(null);
  const [filter, setFilter] = useState("");
  const pageRef = useRef<HTMLDivElement>(null);
  const returnPosition = useRef<{ clientId: ClientId; scroll: Array<{ element: HTMLElement; top: number; left: number }>; x: number; y: number } | null>(null);
  const [pathsOpen, setPathsOpen] = useState(false);
  const [loginMethods, setLoginMethods] = useState<{ clientId: string; methods: NativeLoginMethod[] } | null>(null);
  useEffect(() => { if (settings.data) setDraft(settings.data); }, [settings.data]);
  useEffect(() => () => clearSensitiveAction(action), [action]);
  useLayoutEffect(() => {
    if (selectedClientId !== null) {
      pageRef.current?.querySelector<HTMLElement>("#client-inspector-name")?.focus({ preventScroll: true });
      pageRef.current?.scrollIntoView({ block: "start" });
    } else if (returnPosition.current) {
      const position = returnPosition.current;
      const target = pageRef.current?.querySelector<HTMLElement>("#client-row-" + position.clientId)
        ?? pageRef.current?.querySelector<HTMLElement>("#client-filter");
      target?.focus({ preventScroll: true });
      for (const { element, top, left } of position.scroll) { element.scrollTop = top; element.scrollLeft = left; }
      window.scrollTo(position.x, position.y);
      returnPosition.current = null;
    }
  }, [selectedClientId]);
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
  const refresh = async () => { await Promise.all([connections.refetch(), takeovers.refetch(), settings.refetch(), queryClient.invalidateQueries({ queryKey: ACCOUNT_QUERY_KEYS.list })]); };
  const gatewayIssue = (approved: ClientAction) => clientGatewayActionIssue(approved, currentClientConnections(queryClient),
    queryClient.getQueryState(["takeovers"])?.status === "success" ? queryClient.getQueryData<AppTakeoverStatus[]>(["takeovers"]) : undefined, login);
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
        const issue = gatewayIssue(approved);
        if (issue) throw new Error(issue);
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
    if (completed && approved.kind === "settings") setPathsOpen(false);
  };
  const problem = operation.error || (connections.error ? errorText(connections.error) : accounts.error ? accountErrorText(accounts.error) : settings.error ? errorText(settings.error) : takeovers.error ? errorText(takeovers.error) : "");
  const bindingRow = connections.data?.find(row => row.id === bindingClientId);
  const selectedRow = connections.data?.find(row => row.id === selectedClientId);
  const configurationGroups = groupClientConfigurations(connections.data ?? []);
  const selectedConfiguration = configurationGroups.find(group => group.rows.some(row => row.id === selectedRow?.id));
  const selectionBusy = operation.busy || action !== null || bindingClientId !== null || pathsOpen;
  const selectClient = (id: ClientId) => {
    if (selectedClientId === null) {
      const scroll: Array<{ element: HTMLElement; top: number; left: number }> = [];
      for (let element = pageRef.current?.parentElement; element; element = element.parentElement) {
        scroll.push({ element, top: element.scrollTop, left: element.scrollLeft });
      }
      returnPosition.current = { clientId: id, scroll, x: window.scrollX, y: window.scrollY };
    }
    setLoginMethods(null);
    setSelectedClientId(id);
  };
  const showAction = (next: ClientAction) => { operation.setError(""); setAction(next); };
  const actionIssue = action?.kind === "gateway" ? gatewayIssue(action)
    : action && action.kind !== "settings" && !["poll", "cancel", "complete"].includes(action.kind)
      ? nativeClientActionIssue(action.row.id, connections.isSuccess ? connections.data : undefined, login) : "";
  const affectedClients = action?.kind === "gateway"
    ? configurationGroups.find(group => group.rows.some(row => row.id === action.row.id))?.rows : undefined;
  const actionMessage = action ? confirmationText(action) + (affectedClients && affectedClients.length > 1
    ? "\n" + t("共享配置涉及以下客户端：") + "\n" + affectedClients.map(row => row.label).join(" · ") : "") : "";
  return <div className="workspace-page clients-page" ref={pageRef}>
    <header className="client-page-heading">
      {selectedClientId === null ? <div className="client-directory-heading"><h1>{t("客户端")}</h1></div>
        : <Button variant="quiet" className="client-back" disabled={selectionBusy} onClick={() => { setLoginMethods(null); setSelectedClientId(null); }}><ArrowLeft size={16} aria-hidden="true" />{t("返回客户端列表")}</Button>}
      <div className="client-page-actions">
        {onNavigate && <><Button variant="quiet" size="sm" disabled={selectionBusy} onClick={() => onNavigate("accounts")}>{t("账号池")}</Button><Button variant="quiet" size="sm" disabled={selectionBusy} onClick={() => onNavigate("diagnostics")}>{t("连接诊断")}</Button></>}
        <Button variant="quiet" size="sm" disabled={selectionBusy} onClick={() => setPathsOpen(true)}>{t("客户端路径")}</Button>
        <Button variant="quiet" size="sm" aria-label={t("刷新状态")} title={t("刷新状态")} loading={connections.isFetching || takeovers.isFetching} disabled={selectionBusy} onClick={() => void refresh()}><RefreshCw size={16} aria-hidden="true" /></Button>
      </div>
    </header>
    <InlineError message={problem} onRetry={() => void refresh()} retrying={connections.isFetching} />
    {operation.message && <Toast title={operation.message} onDismiss={() => operation.setMessage("")} />}
    {!native && <p className="workspace-route-notice">{t("浏览器预览仅展示入口；真实安装、登录、额度和配置需要在桌面应用中验证。")}</p>}
    {pendingConfiguration && <p className="workspace-route-notice" role="status">{t("客户端路径已保存，请重启 XY Switch 后继续操作。")}</p>}
    {connections.isLoading ? <LoadingState label={t("读取客户端入口…")} /> : selectedClientId === null ? <ClientDirectory
      rows={connections.data ?? []} takeovers={takeovers.isSuccess ? takeovers.data : undefined} login={login}
      filter={filter} busy={selectionBusy} disabled={!native || !connections.isSuccess || !takeovers.isSuccess}
      configurationControl={configurationControl} onFilter={setFilter} onSelect={selectClient} onAction={showAction} /> : selectedRow && selectedConfiguration ? <div className="client-detail-workspace">
      <div className="client-shared-context">
        <span className="client-shared-owner">{t("配置归属")} · <strong>{clientConfigurationLabel(selectedConfiguration)}</strong></span>
        {selectedConfiguration.rows.length > 1 ? <>
          <div className="client-member-switch" role="group" aria-label={t("共享此配置的客户端")}>{selectedConfiguration.rows.map(row =>
            <button key={row.id} type="button" aria-label={t("选择客户端 {client}", { client: row.label })} aria-pressed={selectedRow.id === row.id}
              aria-controls="client-inspector" disabled={selectionBusy} onClick={() => selectClient(row.id)}>{row.label}</button>
          )}</div>
          <p className="client-shared-note">{t("这些入口共用同一配置；接管和恢复会影响共享该配置的客户端。")}</p>
        </> : !selectedConfiguration.owner && <p className="client-shared-note">{t("未提供归属的入口分别展示，不推断共享关系。")}</p>}
      </div>
      <ClientDetailsPanel key={selectedRow.id} row={selectedRow} rows={connections.isSuccess ? connections.data : undefined} accounts={accounts.data ?? []} accountsReady={accounts.isSuccess}
        takeover={takeovers.isSuccess ? takeovers.data?.find(item => item.app === selectedRow.takeoverTarget) : undefined} login={login} loginMethods={loginMethods?.clientId === selectedRow.id ? loginMethods.methods : null}
        disabled={!native || !connections.isSuccess || operation.busy || selectedConfiguration.rows.some(row => row.configurationPending) || bindingClientId !== null || action !== null || pathsOpen}
        busy={selectionBusy} onAction={showAction}
        onBind={() => { operation.setError(""); setBindingClientId(selectedRow.id); }} onCloseMethods={() => setLoginMethods(null)} onOpenPaths={() => setPathsOpen(true)} onNavigate={onNavigate} />
    </div> : <p className="workspace-route-notice" role="status">{t("该客户端已不在当前列表中，请返回列表重新选择。")}</p>}
    {pathsOpen && <ClientPathSettingsDialog draft={draft} disabled={!native || !settings.isSuccess} busy={operation.busy || action !== null}
      settingsState={settings.isPending ? "loading" : settings.isError ? "error" : undefined}
      pendingLogin={anyPendingLogin} pendingConfiguration={pendingConfiguration} onChange={setDraft} onClose={() => setPathsOpen(false)}
      onSave={() => { operation.setError(""); setAction({ kind: "settings", settings: { ...draft } }); }} />}
    {bindingRow && <NativeAccountBindingDialog row={bindingRow} rows={connections.isSuccess ? connections.data : undefined} login={login} accountsReady={accounts.isSuccess}
      onClose={() => setBindingClientId(null)} onBound={() => { setBindingClientId(null); operation.setMessage(t("原生账号已绑定，状态与额度已同步。")); }} />}
    {action && <NativeAccountConfirmDialog title={t(actionLabels[action.kind])} message={actionMessage} confirmLabel={t("授权并继续")} busy={operation.busy} disabled={Boolean(actionIssue)} error={actionIssue || operation.error} onCancel={() => { clearSensitiveAction(action); setAction(null); }} onConfirm={() => void confirm()} />}
  </div>;
}
