import { invoke } from "@tauri-apps/api/core";
import { maxPluginPackageBytes, type InstallConfirmation, type InstalledPlugin, type InstallPreview, type PackageValidation, type PluginClient, type ToolCallConfirmation, type ToolCallPreview, type ToolExecutionResult, type TrustedKeyInput, type TrustedSigningKey, type VerifiedProviderMapping, type VerifiedSkill, type VerifiedTool } from "../types/plugins";

export const maxPluginEnvelopeBytes = maxPluginPackageBytes + 2048;
export const pluginQueryKeys = {
  all: ["local-plugins"] as const,
  installed: ["local-plugins", "installed"] as const,
  keys: ["local-plugins", "trust-keys"] as const,
  tools: (client: PluginClient | "") => ["local-plugins", "tools", client] as const,
};

const messages: Record<string, string> = {
  plugin_desktop_required: "本地插件安装、信任配置与执行需要桌面应用。",
  plugin_package_size: "插件文本超过大小限制：包最多 2 MiB，单文件最多 256 KiB。",
  plugin_schema: "插件 JSON 无效，或包含缺失、额外、重复的字段。",
  plugin_path: "插件路径包含不安全路径、设备名或文件别名。",
  plugin_privacy: "发现隐私信息或无法完整检查的内容，操作已阻止。",
  plugin_semver: "插件版本号不符合支持的语义版本格式。",
  plugin_host_version: "插件需要更新版本的 XY Switch。",
  plugin_permissions: "插件权限声明不符合本地限制。",
  plugin_client: "请明确选择支持的客户端。",
  plugin_dependency: "固定版本依赖缺失、停用、无法验证或存在循环。",
  plugin_entry: "插件入口不是支持的工具声明、Markdown 技能或模型映射。",
  plugin_digest: "插件内容与精确校验摘要不一致，请重新预览。",
  plugin_signature: "发布者签名无法通过本地信任密钥验证。",
  plugin_untrusted_key: "请从独立可信来源取得发布者公钥，并在本地确认其指纹。",
  plugin_trust_conflict: "同一信任密钥名称不能替换为另一个公钥。",
  plugin_confirmation: "请明确确认本次预览、完整权限、客户端与精确摘要。",
  plugin_preview_expired: "预览已过期或本地状态已变化，请重新审核并预览。",
  plugin_immutable_version: "同一插件版本不可覆盖；修改内容后须升级版本并重新审核。",
  plugin_process_unsupported: "当前宿主缺少第三方进程所需的系统隔离，拒绝启用。",
  plugin_credentials_unsupported: "当前宿主不提供凭据访问所需的系统隔离，拒绝启用。",
  plugin_write_unsupported: "当前插件宿主不允许写文件，拒绝启用。",
  plugin_external_read_unsupported: "当前宿主只允许读取已授权的插件包内文本。",
  plugin_disabled: "插件尚未对这个客户端授权启用。",
  plugin_withdrawn: "发布者已撤回这个版本，安装、运行和回滚已阻止。",
  plugin_storage: "无法安全读取插件文件；请检查存储目录。",
  plugin_storage_boundary: "插件存储目录或文件的安全边界已变化，操作已阻止。",
  plugin_database: "无法安全读取或保存插件元数据。",
  plugin_not_found: "找不到所请求的本地插件版本或包内文件。",
  plugin_limit: "本地插件、预览或并发执行数量已达到限制。",
  plugin_tool_arguments: "工具输入或输出超过限制，或与声明的格式不符。",
  plugin_tool_not_found: "此插件未声明所请求的工具。",
  plugin_network: "仅允许明确授权的 HTTPS 站点和安全的公网解析地址。",
  plugin_http: "受限文本请求未完成；跳转、代理和认证均不被接受。",
  plugin_operation_failed: "插件操作未完成，请重新预览；原始错误内容已隐藏。",
};

export class PluginServiceError extends Error {
  readonly code: string;
  constructor(code: string) {
    const safeCode = Object.hasOwn(messages, code) ? code : "plugin_operation_failed";
    super(messages[safeCode]);
    this.name = "PluginServiceError";
    this.code = safeCode;
  }
}

export function pluginErrorText(error: unknown): string {
  if (error instanceof PluginServiceError) return error.message;
  const code = typeof error === "object" && error !== null && "code" in error ? (error as { code: unknown }).code : undefined;
  return new PluginServiceError(typeof code === "string" ? code : "plugin_operation_failed").message;
}

export function pluginStatusText(code: string): string {
  return code === "verified" ? "验证通过" : code === "uninstalled" ? "已卸载，保留签名历史缓存" : new PluginServiceError(code).message;
}

export function isPluginDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function boundedText(text: string, limit: number): void {
  if (!text.trim()) throw new PluginServiceError("plugin_schema");
  if (new TextEncoder().encode(text).length > limit) throw new PluginServiceError("plugin_package_size");
}

function jsonText(value: unknown): string {
  try { return JSON.stringify(value); } catch { throw new PluginServiceError("plugin_schema"); }
}

async function native<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isPluginDesktop()) throw new PluginServiceError("plugin_desktop_required");
  try { return args ? await invoke<T>(command, args) : await invoke<T>(command); }
  catch (error) { throw new PluginServiceError(typeof error === "object" && error !== null && "code" in error && typeof error.code === "string" ? error.code : "plugin_operation_failed"); }
}

export async function readPluginDocument(file: Pick<File, "size" | "arrayBuffer">): Promise<string> {
  if (file.size > maxPluginEnvelopeBytes) throw new PluginServiceError("plugin_package_size");
  try {
    const bytes = await file.arrayBuffer();
    if (bytes.byteLength > maxPluginEnvelopeBytes) throw new PluginServiceError("plugin_package_size");
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    boundedText(text, maxPluginEnvelopeBytes);
    return text;
  } catch (error) { throw error instanceof PluginServiceError ? error : new PluginServiceError("plugin_schema"); }
}

export function validatePluginPackage(packageJson: string): Promise<PackageValidation> {
  boundedText(packageJson, maxPluginEnvelopeBytes);
  return native("plugins_validate_package", { packageJson });
}
export function previewTrustedPluginKey(key: TrustedKeyInput): Promise<TrustedSigningKey> {
  return native("plugins_preview_trusted_key", { keyJson: jsonText(key) });
}
export function configureTrustedPluginKey(key: TrustedKeyInput, fingerprint: string, confirmed: boolean): Promise<TrustedSigningKey> {
  if (!confirmed || !/^[a-f0-9]{64}$/.test(fingerprint)) throw new PluginServiceError("plugin_confirmation");
  return native("plugins_configure_trusted_key", { keyJson: jsonText(key), fingerprint, confirmed });
}
export function listTrustedPluginKeys(): Promise<TrustedSigningKey[]> { return native("plugins_list_trusted_keys"); }
export function revokeTrustedPluginKey(id: string): Promise<void> { return native("plugins_revoke_trusted_key", { id }); }
export function previewPluginInstall(publishedJson: string): Promise<InstallPreview> {
  boundedText(publishedJson, maxPluginEnvelopeBytes);
  return native("plugins_preview_install", { publishedJson });
}
export function previewInstalledPlugin(id: string): Promise<InstallPreview> { return native("plugins_preview_installed", { id }); }
export function confirmPluginInstall(confirmation: InstallConfirmation): Promise<InstalledPlugin> {
  if (!confirmation.confirmed || !confirmation.clients.length) throw new PluginServiceError("plugin_confirmation");
  return native("plugins_confirm_install", { confirmationJson: jsonText(confirmation) });
}
export function listInstalledPlugins(): Promise<InstalledPlugin[]> { return native("plugins_list_installed"); }
export function disablePlugin(id: string, client: PluginClient | null = null): Promise<InstalledPlugin> { return native("plugins_disable", { id, client }); }
export function enablePluginClient(id: string, client: PluginClient, digest: string, confirmed: boolean): Promise<InstalledPlugin> {
  if (!confirmed) throw new PluginServiceError("plugin_confirmation");
  return native("plugins_enable_client", { id, client, digest, confirmed });
}
export function uninstallPlugin(id: string, confirmed: boolean): Promise<InstalledPlugin> {
  if (!confirmed) throw new PluginServiceError("plugin_confirmation");
  return native("plugins_uninstall", { id, confirmed });
}
export function previewPluginRollback(id: string, version: string): Promise<InstallPreview> { return native("plugins_preview_rollback", { id, version }); }
export function applyPluginWithdrawal(withdrawalJson: string): Promise<InstalledPlugin> {
  boundedText(withdrawalJson, 4096);
  return native("plugins_apply_withdrawal", { withdrawalJson });
}
export function pluginToolsForClient(client: PluginClient): Promise<VerifiedTool[]> { return native("plugins_tools", { client }); }
export function pluginSkillsForClient(client: PluginClient): Promise<VerifiedSkill[]> { return native("plugins_skills", { client }); }
export function pluginMappingsForClient(client: PluginClient): Promise<VerifiedProviderMapping[]> { return native("plugins_provider_mappings", { client }); }
export function previewPluginTool(id: string, client: PluginClient, tool: string, argumentsJson: string): Promise<ToolCallPreview> {
  boundedText(argumentsJson, 64 * 1024);
  return native("plugins_preview_tool", { id, client, tool, argumentsJson });
}
export function executePluginTool(client: PluginClient, confirmation: ToolCallConfirmation): Promise<ToolExecutionResult> {
  if (!confirmation.confirmed) throw new PluginServiceError("plugin_confirmation");
  return native("plugins_execute_tool", { client, confirmationJson: jsonText(confirmation) });
}
