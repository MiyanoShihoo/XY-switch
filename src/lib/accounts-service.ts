import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type { AccountInfo, AccountPolicy, AddAccountRequest, ImportAccountRequest, LocalRevocation, ReplaceCredentialRequest, SelectedAccountImport, UpdateAccountRequest } from "../types/accounts";

export const accountsNative = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const DEFAULT_ACCOUNT_POLICY: Readonly<AccountPolicy> = Object.freeze({
  mode: "rotate", fixedAccountId: null, affinity: true,
  failureCooldownSeconds: 30, rateLimitCooldownSeconds: 60, maxCooldownSeconds: 3600,
});

function knownError(code: string): string {
  switch (code) {
    case "account_native_required": return t("账号池操作需要桌面应用。");
    case "account_authorization_required": return t("请明确授权本次账号操作及所选来源。");
    case "account_destination_authorization_required": return t("供应商请求地址尚未批准或已改变，请检查地址并重新授权。");
    case "account_invalid_input": return t("请检查别名、供应商、凭据类型和有效期。别名不能包含邮箱或凭据。");
    case "account_not_found": return t("账号本地引用不存在，请刷新列表。");
    case "account_provider_unavailable": return t("绑定的供应商不存在或当前不可用。");
    case "account_store_unavailable": return t("账号元数据存储暂不可用，操作已阻断。");
    case "account_credential_store_unavailable": return t("账号安全存储暂不可用，请检查桌面集成与系统凭据服务。");
    case "account_credential_missing": return t("安全存储中的账号凭据缺失，请重新授权。");
    case "account_revision_conflict": return t("账号已被其他操作更新，请刷新后重试。");
    case "account_revoked": return t("此账号的本地引用已撤销。");
    case "account_cancelled": return t("操作已取消，后续请求不会派发。");
    case "account_attempt_not_admitted": return t("请求尚未通过现有任务与预算账本登记。");
    case "account_attempt_already_dispatched": return t("本次尝试已派发或结算，不能重复发送。");
    case "account_selection_stale": return t("账号凭据版本已变化，请重新选择。");
    case "account_no_eligible_account": return t("当前策略下没有可用账号，请检查授权、冷却和额度。");
    case "account_refresh_not_configured": return t("客户端尚未提供已授权的刷新接口，请在原生客户端重新登录。");
    case "account_refresh_failed": return t("凭据刷新失败或超时，账号进入受控冷却。");
    case "account_refresh_rate_limited": return t("凭据刷新受到限流，请等待冷却结束。");
    case "account_login_required": return t("凭据已失效，请在对应客户端重新登录或重新授权。");
    case "account_invalid_quota_evidence": return t("额度证据缺失、过期或与账号不匹配，未采用该额度。");
    case "account_import_selection_expired": return t("文件选择已过期或已使用，请重新选择具体文件。");
    case "account_import_invalid_file": return t("所选文件不符合账号导入格式或大小限制。");
    default: return t("账号池操作失败，请检查桌面集成后重试。");
  }
}

export class AccountServiceError extends Error {
  readonly code: string;
  constructor(code: string) {
    super(knownError(code));
    this.name = "AccountServiceError";
    this.code = /^account_[a-z_]+$/.test(code) && knownError(code) !== knownError("") ? code : "account_operation_failed";
  }
}

export function accountErrorText(error: unknown): string {
  if (error instanceof AccountServiceError) return knownError(error.code);
  const message = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  return /^account_[a-z_]+$/.test(message) ? knownError(message) : knownError("");
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!accountsNative()) throw new AccountServiceError("account_native_required");
  try { return await invoke<T>(command, args); }
  catch (error) {
    const message = typeof error === "string" ? error : error instanceof Error ? error.message : "";
    throw new AccountServiceError(/^account_[a-z_]+$/.test(message) ? message : "account_operation_failed");
  }
}

export const listAccounts = () => call<AccountInfo[]>("list_accounts");
async function authorizedCall<T>(command: string, args: Record<string, unknown>, confirmed: boolean): Promise<T> {
  if (confirmed !== true) throw new AccountServiceError("account_authorization_required");
  return call<T>(command, args);
}

export const addAccount = (request: AddAccountRequest) => authorizedCall<AccountInfo>("add_account", { request }, request.confirmed);
export const updateAccount = (request: UpdateAccountRequest) => authorizedCall<AccountInfo>("update_account", { request }, request.confirmed);
export const replaceAccountCredentials = (request: ReplaceCredentialRequest) => authorizedCall<AccountInfo>("replace_account_credentials", { request }, request.confirmed);
export const chooseAccountImport = () => call<SelectedAccountImport | null>("choose_account_import");
export const importAccount = (request: ImportAccountRequest) => authorizedCall<AccountInfo>("import_account", { request }, request.confirmed);
export const getAccountPolicy = (providerId: string) => call<AccountPolicy>("get_account_policy", { providerId });
export const saveAccountPolicy = (providerId: string, policy: AccountPolicy, confirmed: boolean) => authorizedCall<AccountPolicy>("save_account_policy", { providerId, policy, confirmed }, confirmed);
export const refreshAccount = (id: string, confirmed: boolean) => authorizedCall<AccountInfo>("refresh_account", { id, confirmed }, confirmed);
export const revokeAccount = (id: string, confirmed: boolean) => authorizedCall<LocalRevocation>("revoke_account", { id, confirmed }, confirmed);
