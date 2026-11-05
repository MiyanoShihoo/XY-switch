import type { QueryClient } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { bindNativeAccount, refreshNativeAccount } from "../../lib/clients-service";
import type { AccountClient, AccountInfo } from "../../types/accounts";
import type { BindNativeAccountRequest, ClientConnection, ClientId, NativeAccountCandidate, NativeLoginProgress } from "../../types/clients";
import { ACCOUNT_QUERY_KEYS, captureAccountDestination, requireAccountAction, type AccountProviderSummary } from "../accounts/account-actions";
import { nativeAccountScopeMessage, validAccountAlias as validateAccountAlias } from "../accounts/account-presentation";

export const CLIENT_CONNECTIONS_QUERY_KEY = ["clients", "connections"] as const;
export type NativeBindingApproval = Omit<BindNativeAccountRequest, "confirmed">;

export function nativeAccountClientId(client: AccountClient | null): ClientId | null {
  switch (client) {
    case "codexDesktop": return "codex-desktop";
    case "codexCli": return "codex-cli";
    case "openCodeDesktop": return "opencode-desktop";
    case "openCodeCli": return "opencode-cli";
    default: return null;
  }
}

function nativeOwner(id: ClientId): string | null {
  if (id === "codex-desktop" || id === "codex-cli") return "codex";
  if (id === "opencode-desktop" || id === "opencode-cli") return "opencode";
  return null;
}

export function nativeLoginPending(id: ClientId, rows: ClientConnection[] | undefined, progress?: NativeLoginProgress | null): boolean {
  const owner = nativeOwner(id);
  return owner !== null && (Boolean(progress?.status === "pending" && nativeOwner(progress.clientId) === owner)
    || Boolean(rows?.some(row => nativeOwner(row.id) === owner && row.loginStatus === "pending")));
}

export function nativeClientActionIssue(id: ClientId, rows: ClientConnection[] | undefined, progress?: NativeLoginProgress | null): string {
  if (nativeLoginPending(id, rows, progress)) return t("原生登录进行中，请完成或取消登录后再读取、绑定或刷新账号。");
  const owner = nativeOwner(id);
  const row = rows?.find(item => item.id === id);
  if (!owner || !row || row.setupError || rows?.some(item => nativeOwner(item.id) === owner && item.configurationPending)) {
    return t("原生客户端状态不可用，请刷新状态并检查客户端配置。");
  }
  return "";
}

export function currentClientConnections(client: QueryClient): ClientConnection[] | undefined {
  return client.getQueryState(CLIENT_CONNECTIONS_QUERY_KEY)?.status === "success"
    ? client.getQueryData<ClientConnection[]>(CLIENT_CONNECTIONS_QUERY_KEY) : undefined;
}

export function markNativeLoginStarted(client: QueryClient, id: ClientId): void {
  client.setQueryData<ClientConnection[]>(CLIENT_CONNECTIONS_QUERY_KEY, rows => rows?.map(row =>
    nativeOwner(row.id) === nativeOwner(id) ? { ...row, account: null, loginStatus: "pending" } : row));
}

export async function runNativeAccountOperation<Result>(client: QueryClient, work: () => Promise<Result>): Promise<Result> {
  try { return await work(); }
  finally {
    await Promise.all([
      client.invalidateQueries({ queryKey: ACCOUNT_QUERY_KEYS.list }),
      client.invalidateQueries({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY }),
    ]);
  }
}

function rememberNativeAccount(client: QueryClient, account: AccountInfo): void {
  client.setQueryData<AccountInfo[]>(ACCOUNT_QUERY_KEYS.list, previous => {
    const accounts = previous ?? [];
    return accounts.some(item => item.id === account.id) ? accounts.map(item => item.id === account.id ? account : item) : [...accounts, account];
  });
}

export function nativeCandidateAvailable(candidate: NativeAccountCandidate, timestamp: number): boolean {
  return Boolean(candidate.id) && candidate.boundAccountId === null && Number.isSafeInteger(candidate.expiresAt) && candidate.expiresAt > timestamp;
}

export function captureNativeBinding(row: ClientConnection, form: { candidateId: string; alias: string; providerId: string }, providers: AccountProviderSummary[], timestamp: number): NativeBindingApproval {
  const candidate = row.account?.poolCandidates.find(item => item.id === form.candidateId);
  if (!candidate || !nativeCandidateAvailable(candidate, timestamp)) throw new Error(t("原生账号候选已过期、已绑定或已改变，请重新授权读取账号。"));
  if (!validateAccountAlias(form.alias)) throw new Error(t("请使用不含邮箱或凭据的账号别名。"));
  const provider = providers.find(item => item.id === form.providerId);
  if (!provider?.enabled) throw new Error(t("请选择可用的目标供应商。"));
  return { clientId: row.id, candidateId: candidate.id, alias: form.alias, providerId: provider.id, approvedDestination: captureAccountDestination(provider) };
}

export async function submitNativeBinding(client: QueryClient, approval: NativeBindingApproval, confirmed: boolean, isActive: () => boolean, progress?: NativeLoginProgress | null): Promise<AccountInfo> {
  requireAccountAction(confirmed, isActive);
  const rows = currentClientConnections(client);
  const issue = nativeClientActionIssue(approval.clientId, rows, progress);
  if (issue) throw new Error(issue);
  if (client.getQueryState(ACCOUNT_QUERY_KEYS.providers)?.status !== "success"
    || client.getQueryState(ACCOUNT_QUERY_KEYS.list)?.status !== "success") throw new Error(t("请先刷新供应商和账号列表，再确认绑定。"));
  const current = captureNativeBinding(rows!.find(row => row.id === approval.clientId)!, approval,
    client.getQueryData<AccountProviderSummary[]>(ACCOUNT_QUERY_KEYS.providers) ?? [], Math.floor(Date.now() / 1000));
  if (current.approvedDestination !== approval.approvedDestination) throw new Error(t("绑定目的地已改变，请检查完整地址并重新确认。"));
  return runNativeAccountOperation(client, async () => {
    const account = await bindNativeAccount({ ...current, confirmed: true });
    rememberNativeAccount(client, account);
    return account;
  });
}

export function nativeAccountCanSync(account: AccountInfo, rows: ClientConnection[] | undefined): boolean {
  const id = nativeAccountClientId(account.client);
  return account.kind === "nativeManaged" && account.enabled && account.destinationApproved === true
    && !["revoked", "disabled", "refreshing", "destinationApprovalRequired"].includes(account.status)
    && id !== null && !nativeClientActionIssue(id, rows);
}

export function nativeAccountSyncCopy(account: Pick<AccountInfo, "client">): { title: string; message: string; confirmLabel: string } {
  const codex = account.client === "codexDesktop" || account.client === "codexCli";
  return {
    title: codex ? t("确认通过 Codex 刷新") : t("确认同步 OpenCode 状态"),
    message: (codex ? t("授权 Codex 原生客户端刷新其管理的凭据，并同步账号状态与额度。桌面与 CLI 共用登录状态。")
      : t("授权读取 OpenCode 原生账号元数据并同步状态与额度；此操作不会强制刷新 token。桌面与终端共用登录状态。")) + " " + nativeAccountScopeMessage(),
    confirmLabel: codex ? t("授权原生刷新") : t("授权同步元数据"),
  };
}

export async function submitNativeAccountSync(client: QueryClient, id: string, confirmed: boolean, isActive: () => boolean): Promise<AccountInfo> {
  requireAccountAction(confirmed, isActive);
  const account = client.getQueryData<AccountInfo[]>(ACCOUNT_QUERY_KEYS.list)?.find(item => item.id === id);
  if (client.getQueryState(ACCOUNT_QUERY_KEYS.list)?.status !== "success" || !account || !nativeAccountCanSync(account, currentClientConnections(client))) {
    throw new Error(t("原生账号暂不能同步，请检查登录进度、启用状态与目的地授权。"));
  }
  return runNativeAccountOperation(client, async () => {
    const updated = await refreshNativeAccount(id, true);
    rememberNativeAccount(client, updated);
    return updated;
  });
}
