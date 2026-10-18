import type { ProviderAdvancedConfig } from "../types/provider";

const protectedHeaders = new Set([
  "host",
  "content-length",
  "transfer-encoding",
  "connection",
  "proxy-authorization",
  "content-type",
  "authorization",
  "x-api-key",
  "x-goog-api-key",
  "cookie",
  "set-cookie",
  "accept-encoding",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isSensitiveName(name: string): boolean {
  const normalized = name.toLowerCase().replaceAll("-", "_");
  return ["authorization", "api_key", "apikey", "access_token", "auth_token", "password", "secret", "cookie"]
    .some((needle) => normalized.includes(needle));
}

function containsSensitiveKey(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(containsSensitiveKey);
  if (!isRecord(value)) return false;
  return Object.entries(value).some(([key, nested]) => isSensitiveName(key) || containsSensitiveKey(nested));
}

export function formatJsonObject(value: Record<string, unknown>): string {
  return Object.keys(value).length ? JSON.stringify(value, null, 2) : "";
}

export function parseHeaderOverrides(text: string): Record<string, string> {
  if (!text.trim()) return {};
  const value: unknown = JSON.parse(text);
  if (!isRecord(value)) throw new Error("Header 覆盖必须是 JSON 对象");
  const result: Record<string, string> = {};
  for (const [rawName, rawValue] of Object.entries(value)) {
    const name = rawName.trim().toLowerCase();
    if (!/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name)) throw new Error(`Header 名称无效：${rawName}`);
    if (typeof rawValue !== "string") throw new Error(`Header ${rawName} 的值必须是字符串`);
    if (protectedHeaders.has(name) || isSensitiveName(name)) throw new Error(`Header ${rawName} 由代理安全管理，不能覆盖`);
    if (Object.hasOwn(result, name)) throw new Error(`Header 名称重复：${rawName}`);
    result[name] = rawValue;
  }
  return result;
}

export function parseBodyOverrides(text: string): Record<string, unknown> {
  if (!text.trim()) return {};
  const value: unknown = JSON.parse(text);
  if (!isRecord(value)) throw new Error("Body 覆盖必须是 JSON 对象");
  if (Object.hasOwn(value, "model") || Object.hasOwn(value, "stream")) {
    throw new Error("Body 覆盖不能修改 model 或 stream");
  }
  if (containsSensitiveKey(value)) throw new Error("Body 覆盖不能包含密钥、Token 或密码字段");
  return value;
}

export function validateProviderAdvanced(config: ProviderAdvancedConfig): string {
  if (/[\x00-\x1f\x7f]/.test(config.customUserAgent)) return "User-Agent 不能包含控制字符";
  if (new TextEncoder().encode(config.customUserAgent).length > 512) return "User-Agent 不能超过 512 字节";
  const models = new Set<string>();
  for (const mapping of config.modelMappings) {
    const model = mapping.model.trim().toLowerCase();
    if (!model) return "模型映射中的模型名称不能为空";
    if (models.has(model)) return `模型映射重复：${mapping.model}`;
    if (mapping.contextWindow != null && (!Number.isSafeInteger(mapping.contextWindow) || mapping.contextWindow < 1)) return "上下文窗口必须是正整数，或留空使用默认值";
    models.add(model);
  }
  if (config.codex.context1m && (!Number.isInteger(config.codex.compactionThreshold) || config.codex.compactionThreshold < 10000 || config.codex.compactionThreshold > 10000000)) {
    return "压缩阈值必须在 10000 到 10000000 之间";
  }
  for (const line of config.codex.configToml.split(/\r?\n/)) {
    const key = line.split("=", 1)[0]?.trim() ?? "";
    if (key && !key.startsWith("#") && isSensitiveName(key)) return "config.toml 中不能保存 API Key、Token 或密码";
  }
  return "";
}
