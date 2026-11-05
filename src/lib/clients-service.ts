import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type { BindNativeAccountRequest, ClientConnection, ClientId, ClientSettings, NativeAccountSnapshot, NativeLoginMethod, NativeLoginProgress } from "../types/clients";
import type { AccountInfo } from "../types/accounts";

export function clientRuntimeAvailable(): boolean { return "__TAURI_INTERNALS__" in window; }

export function clearClientLoginInputs(inputs: Record<string, string>): void {
  for (const key of Object.keys(inputs)) {
    inputs[key] = "";
    delete inputs[key];
  }
}

function requireAuthorization(confirmed: boolean): void {
  if (confirmed !== true) throw new Error(t("客户端账号操作需要明确授权"));
  if (!clientRuntimeAvailable()) throw new Error(t("浏览器预览不能读取或修改真实客户端账号"));
}

async function invokeClient<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  try { return await invoke<T>(command, args); }
  catch { throw new Error(t("客户端操作未完成；请刷新状态并检查原生客户端设置。")); }
}

const emptySettings: ClientSettings = { opencodeConfig: null, opencodeSchema: null, claudeDesktopProfile: null, codexBinary: null, opencodeBinary: null };
const previewRows: Array<Pick<ClientConnection, "id" | "label" | "configOwner" | "takeoverTarget" | "mcpTarget" | "loginMethod" | "inferenceMode">> = [
  { id: "codex-desktop", label: "Codex Desktop", configOwner: "codex", takeoverTarget: "codex", mcpTarget: "codex", loginMethod: "codex_device_code", inferenceMode: "responses_gateway" },
  { id: "codex-cli", label: "Codex CLI", configOwner: "codex", takeoverTarget: "codex", mcpTarget: "codex", loginMethod: "codex_device_code", inferenceMode: "responses_gateway" },
  { id: "claude-desktop", label: "Claude Desktop", configOwner: "claude-desktop", takeoverTarget: null, mcpTarget: "claude-desktop", loginMethod: "native_app", inferenceMode: "native_subscription" },
  { id: "claude-desktop-3p", label: "Claude Desktop 3P", configOwner: "claude-desktop-3p", takeoverTarget: "claude-desktop-3p", mcpTarget: "claude-desktop", loginMethod: "native_gateway_sso", inferenceMode: "messages_gateway" },
  { id: "claude-code", label: "Claude Code", configOwner: "claude", takeoverTarget: "claude", mcpTarget: "claude", loginMethod: "native_terminal", inferenceMode: "messages_gateway" },
  { id: "opencode-desktop", label: "OpenCode Desktop", configOwner: "opencode", takeoverTarget: "opencode", mcpTarget: "opencode", loginMethod: "opencode_oauth", inferenceMode: "chat_gateway" },
  { id: "opencode-cli", label: "OpenCode CLI", configOwner: "opencode", takeoverTarget: "opencode", mcpTarget: "opencode", loginMethod: "opencode_oauth", inferenceMode: "chat_gateway" },
  { id: "gemini-cli", label: "Gemini CLI", configOwner: "gemini", takeoverTarget: "gemini", mcpTarget: "gemini", loginMethod: "native_terminal", inferenceMode: "gemini_gateway" },
];

export async function loadClientConnections(): Promise<ClientConnection[]> {
  if (clientRuntimeAvailable()) return invokeClient<ClientConnection[]>("list_client_connections");
  return previewRows.map(row => ({ ...row, configPaths: [], configDetected: false, loginInstructions: t("请在桌面应用中查看真实路径和原生登录指引。"), nativeVerification: "preview", setupError: null, configurationPending: false, loginStatus: null, account: null }));
}

export async function loadClientSettings(): Promise<ClientSettings> {
  return clientRuntimeAvailable() ? invokeClient<ClientSettings>("get_client_settings") : { ...emptySettings };
}

export async function saveClientSettings(settings: ClientSettings, confirmed: boolean): Promise<ClientSettings> {
  requireAuthorization(confirmed);
  return invokeClient<ClientSettings>("save_client_settings", { settings });
}

export async function readClientAccount(clientId: ClientId, confirmed: boolean, refresh = false): Promise<NativeAccountSnapshot> {
  requireAuthorization(confirmed);
  return invokeClient<NativeAccountSnapshot>("read_client_account", { clientId, confirmed, refresh });
}

export async function bindNativeAccount(request: BindNativeAccountRequest): Promise<AccountInfo> {
  requireAuthorization(request.confirmed);
  return invokeClient<AccountInfo>("bind_native_account", { request });
}

export async function refreshNativeAccount(id: string, confirmed: boolean): Promise<AccountInfo> {
  requireAuthorization(confirmed);
  return invokeClient<AccountInfo>("refresh_native_account", { id, confirmed });
}

export async function startClientLogin(clientId: ClientId, confirmed: boolean): Promise<NativeLoginProgress> {
  requireAuthorization(confirmed);
  return invokeClient<NativeLoginProgress>("start_client_login", { clientId, confirmed });
}

export async function pollClientLogin(clientId: ClientId, confirmed: boolean): Promise<NativeLoginProgress> {
  requireAuthorization(confirmed);
  return invokeClient<NativeLoginProgress>("poll_client_login", { clientId, confirmed });
}

export async function cancelClientLogin(clientId: ClientId, confirmed: boolean): Promise<void> {
  requireAuthorization(confirmed);
  return invokeClient<void>("cancel_client_login", { clientId, confirmed });
}

export async function logoutClientAccount(clientId: ClientId, confirmed: boolean): Promise<void> {
  requireAuthorization(confirmed);
  return invokeClient<void>("logout_client_account", { clientId, confirmed });
}

export async function loadClientLoginMethods(clientId: ClientId, confirmed: boolean): Promise<NativeLoginMethod[]> {
  requireAuthorization(confirmed);
  return invokeClient<NativeLoginMethod[]>("list_client_login_methods", { clientId, confirmed });
}

export async function startOpenCodeLogin(clientId: ClientId, method: NativeLoginMethod, inputs: Record<string, string>, confirmed: boolean): Promise<NativeLoginProgress> {
  try {
    requireAuthorization(confirmed);
    return await invokeClient<NativeLoginProgress>("start_opencode_client_login", { clientId, confirmed, providerId: method.providerId, methodId: method.methodId, inputs });
  } finally { clearClientLoginInputs(inputs); }
}

export async function completeClientLogin(clientId: ClientId, code: string, confirmed: boolean): Promise<NativeLoginProgress> {
  const request = { clientId, confirmed, code };
  code = "";
  try {
    requireAuthorization(confirmed);
    return await invokeClient<NativeLoginProgress>("complete_client_login", request);
  } finally { request.code = ""; }
}

export async function disconnectClientProvider(clientId: ClientId, providerId: string, connectionId: string | null, confirmed: boolean): Promise<void> {
  requireAuthorization(confirmed);
  return invokeClient<void>("disconnect_client_provider", { clientId, confirmed, providerId, connectionId });
}

export function clientAuthorizationUrl(value: string | null, clientId: ClientId): string | null {
  if (!value || value.length > 8192) return null;
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password || (url.port && url.port !== "443")) return null;
    if (clientId.startsWith("codex-") && !["auth.openai.com", "chatgpt.com", "www.chatgpt.com"].includes(url.hostname)) return null;
    return url.href;
  } catch { return null; }
}
