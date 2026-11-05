export type PluginKind = "mcp" | "skill" | "provider";
export type PluginClient = "codex-desktop" | "codex-cli" | "claude-desktop" | "claude-desktop-3p" | "claude-code" | "opencode-desktop" | "opencode-cli" | "gemini-cli";
export const pluginClients: readonly PluginClient[] = ["codex-desktop", "codex-cli", "claude-desktop", "claude-desktop-3p", "claude-code", "opencode-desktop", "opencode-cli", "gemini-cli"];
export const maxPluginPackageBytes = 2 * 1024 * 1024;

export interface PluginPermissions { network: string[]; readPaths: string[]; writePaths: string[]; process: boolean; credentials: string[] }
export const emptyPluginPermissions = (): PluginPermissions => ({ network: [], readPaths: [], writePaths: [], process: false, credentials: [] });
export interface PluginManifest {
  schemaVersion: 1; id: string; name: string; description: string; author: string; version: string;
  minimumHostVersion: string; kind: PluginKind; entry: string; clients: PluginClient[];
  permissions: PluginPermissions; dependencies: string[];
}
export interface PluginPackage { manifest: PluginManifest; files: Record<string, string> }
export interface PublishedPlugin { package: PluginPackage; digest: string; signature: string; signingKeyId: string }
export type ToolOperation = { type: "json_passthrough" } | { type: "read_text"; pathArgument: string } | { type: "http_get"; urlArgument: string };
export interface ToolInputProperty { type: "string" | "boolean" | "number" | "integer" | "object" | "array"; description?: string; maxLength?: number; enum?: unknown[] }
export interface ToolInputSchema { type: "object"; properties: Record<string, ToolInputProperty>; required: string[]; additionalProperties: false }
export interface DeclarativeTool { name: string; description: string; inputSchema: ToolInputSchema; operation: ToolOperation }
export interface ToolBundle { schemaVersion: 1; tools: DeclarativeTool[] }
export interface ProviderMapping { schemaVersion: 1; upstreamFormat: "openai_chat" | "openai_responses" | "anthropic" | "gemini"; models: Record<string, string> }
export interface PackageValidation { digest: string; canonicalSize: number; fileCount: number; manifest: PluginManifest; runtimeSupported: boolean; warnings: string[] }
export interface TrustedKeyInput { id: string; label: string; publicKey: string }
export interface TrustedSigningKey extends TrustedKeyInput { fingerprint: string; enabled: boolean }
export interface ClientPermissionChange { client: PluginClient; added: PluginPermissions }
export interface InstallPreview {
  previewId: string; expiresAt: number; validation: PackageValidation; signingKeyId: string;
  signingKeyFingerprint: string; previousVersion: string | null; permissionChanges: ClientPermissionChange[];
}
export interface InstallConfirmation {
  previewId: string; digest: string; acceptedPermissions: PluginPermissions;
  clients: PluginClient[]; confirmed: boolean; enable: boolean;
}
export interface PluginVersionInfo { version: string; digest: string; installedAt: number; signingKeyId: string; verification: string; withdrawn: boolean }
export interface InstalledPlugin {
  id: string; manifest: PluginManifest | null; activeVersion: string | null; digest: string | null;
  installed: boolean; enabledClients: PluginClient[]; versions: PluginVersionInfo[]; verification: string; warnings: string[];
}
export interface VerifiedTool { pluginId: string; version: string; digest: string; client: PluginClient; tool: DeclarativeTool }
export interface VerifiedSkill { pluginId: string; version: string; digest: string; content: string }
export interface VerifiedProviderMapping { pluginId: string; version: string; digest: string; mapping: ProviderMapping }
export interface ToolCallPreview {
  previewId: string; expiresAt: number; pluginId: string; version: string; pluginDigest: string; client: PluginClient;
  tool: string; argumentsDigest: string; requiredPermissions: PluginPermissions; effect: string;
}
export interface ToolCallConfirmation { previewId: string; pluginDigest: string; argumentsDigest: string; acceptedPermissions: PluginPermissions; confirmed: boolean }
export interface ToolExecutionResult { pluginId: string; version: string; digest: string; client: PluginClient; tool: string; value: unknown; redacted: boolean }
export interface PluginWithdrawal { id: string; version: string; digest: string; reason: "security" | "privacy" | "withdrawn"; signingKeyId: string; signature: string }
export interface PluginError { code: string; message: string }
