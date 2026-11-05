import { PluginServiceError } from "../../lib/plugins-service";
import { pluginClients, type InstallConfirmation, type InstallPreview, type PluginClient, type PluginPermissions, type PluginWithdrawal, type ToolCallConfirmation, type ToolCallPreview, type TrustedKeyInput, type TrustedSigningKey } from "../../types/plugins";

export interface TrustedKeyReview { input: TrustedKeyInput; preview: TrustedSigningKey }
export interface ToolDraft {
  pluginId: string; version: string; pluginDigest: string; client: PluginClient; tool: string; argumentsJson: string;
}
export interface ToolReview { draft: ToolDraft; preview: ToolCallPreview }
export interface InstallChoices { digest: string; clients: PluginClient[]; accepted: boolean; enable: boolean }
export interface WithdrawalReview { raw: string; notice: PluginWithdrawal }

const digestPattern = /^[a-f0-9]{64}$/;

function reject(): never { throw new PluginServiceError("plugin_confirmation"); }

function validPreview(expiresAt: number, now: number): void {
  if (!Number.isFinite(expiresAt) || expiresAt <= now) throw new PluginServiceError("plugin_preview_expired");
}

export function copyPermissions(permissions: PluginPermissions): PluginPermissions {
  return { network: [...permissions.network], readPaths: [...permissions.readPaths], writePaths: [...permissions.writePaths], process: permissions.process, credentials: [...permissions.credentials] };
}

export function confirmKeyReview(review: TrustedKeyReview, current: TrustedKeyInput, fingerprint: string, independentSource: boolean): TrustedKeyInput {
  const input = review.input;
  const preview = review.preview;
  if (!independentSource || !digestPattern.test(fingerprint) || fingerprint !== preview.fingerprint
    || current.id !== input.id || current.label !== input.label || current.publicKey !== input.publicKey
    || preview.id !== input.id || preview.label !== input.label || preview.publicKey !== input.publicKey) reject();
  return { ...input };
}

export function confirmInstallReview(preview: InstallPreview, choices: InstallChoices, now = Date.now()): InstallConfirmation {
  validPreview(preview.expiresAt, now);
  if (!choices.accepted || !digestPattern.test(choices.digest) || choices.digest !== preview.validation.digest
    || !choices.clients.length || new Set(choices.clients).size !== choices.clients.length
    || choices.clients.some(client => !pluginClients.includes(client) || !preview.validation.manifest.clients.includes(client))
    || choices.enable && !preview.validation.runtimeSupported) reject();
  return { previewId: preview.previewId, digest: preview.validation.digest,
    acceptedPermissions: copyPermissions(preview.validation.manifest.permissions), clients: [...choices.clients], confirmed: true, enable: choices.enable };
}

export function sameToolDraft(left: ToolDraft, right: ToolDraft): boolean {
  return left.client === right.client && left.pluginId === right.pluginId && left.version === right.version
    && left.pluginDigest === right.pluginDigest && left.tool === right.tool && left.argumentsJson === right.argumentsJson;
}

export function confirmToolReview(review: ToolReview, current: ToolDraft, argumentsDigest: string, accepted: boolean, now = Date.now()): ToolCallConfirmation {
  const preview = review.preview;
  validPreview(preview.expiresAt, now);
  if (!accepted || !sameToolDraft(review.draft, current) || !digestPattern.test(argumentsDigest) || argumentsDigest !== preview.argumentsDigest
    || !digestPattern.test(preview.pluginDigest) || preview.client !== current.client || preview.pluginId !== current.pluginId
    || preview.version !== current.version || preview.pluginDigest !== current.pluginDigest || preview.tool !== current.tool) reject();
  return { previewId: preview.previewId, pluginDigest: preview.pluginDigest, argumentsDigest: preview.argumentsDigest,
    acceptedPermissions: copyPermissions(preview.requiredPermissions), confirmed: true };
}

export function readWithdrawalReview(raw: string): WithdrawalReview {
  if (new TextEncoder().encode(raw).length > 4096) throw new PluginServiceError("plugin_package_size");
  let value: unknown;
  try { value = JSON.parse(raw); } catch { throw new PluginServiceError("plugin_schema"); }
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new PluginServiceError("plugin_schema");
  const record = value as Record<string, unknown>;
  const fields = ["id", "version", "digest", "reason", "signingKeyId", "signature"];
  if (Object.keys(record).length !== fields.length || fields.some(field => !Object.hasOwn(record, field) || typeof record[field] !== "string")
    || !record.id || !record.version || !record.signingKeyId || !digestPattern.test(record.digest as string)
    || !/^[a-f0-9]{128}$/.test(record.signature as string) || !["security", "privacy", "withdrawn"].includes(record.reason as string)) {
    throw new PluginServiceError("plugin_schema");
  }
  return { raw, notice: record as unknown as PluginWithdrawal };
}

export function confirmWithdrawalReview(review: WithdrawalReview, current: string, accepted: boolean): string {
  if (!accepted || review.raw !== current) reject();
  return review.raw;
}
