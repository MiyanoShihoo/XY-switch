import { applyPluginWithdrawal, confirmPluginInstall, previewPluginInstall, PluginServiceError } from "./plugins-service";
import { createMarketplaceFetch, MarketplaceTransportError } from "./marketplace-transport";
import { maxPluginPackageBytes, pluginClients, type InstalledPlugin, type PluginClient, type PluginKind, type PluginManifest, type PluginPackage, type PluginPermissions, type PluginWithdrawal, type PublishedPlugin } from "../types/plugins";
import { submissionStatuses, type MarketplaceAuditEvent, type MarketplaceCatalogEntry, type MarketplaceCatalogQuery, type MarketplaceClientOptions, type MarketplaceDecisionRequest, type MarketplaceInstallOptions, type MarketplaceInstallPlan, type MarketplaceListQuery, type MarketplacePageResult, type MarketplaceReport, type MarketplaceSession, type MarketplaceSubmission, type MarketplaceSubmissionSummary, type MarketplaceUser, type MarketplaceVersionRequest, type MarketplaceWithdrawalRequest, type SubmissionStatus } from "../types/marketplace";

export const marketplaceMessages = {
  invalid_origin: "请输入 HTTPS 市场地址；开发 HTTP 仅允许 localhost、127.0.0.1 或 [::1]。",
  invalid_request: "请检查输入、版本号和包格式。",
  invalid_package: "插件包不符合市场格式要求。",
  privacy_rejected: "发现隐私信息或无法完整检查的内容，投稿已阻止。请在本机清理后重试。",
  authentication_required: "请重新登录独立市场账号。",
  invalid_credentials: "市场用户名或密码不正确。",
  forbidden: "当前市场身份无权执行此操作；作者不能审核自己的投稿。",
  not_found: "此版本不可公开下载，可能尚未发布或已撤回。",
  conflict: "版本、摘要或审核状态已变化，请刷新后重新审阅。",
  rate_limited: "操作过于频繁，请稍后重试。",
  unavailable: "市场暂时无法安全完成操作，请稍后重试。",
  invalid_response: "市场响应未通过安全校验，操作已停止。",
  network: "无法连接市场。请检查服务地址、网络和 HTTPS 配置。",
  session_changed: "市场连接或身份已变化，请重新操作。",
  confirmation: "请核对完整权限、签名指纹和客户端，再明确确认。",
  untrusted_key: "此签名公钥尚未在本机独立信任，安装已阻止。请通过可信渠道核对公钥。",
  withdrawn: "发布者已撤回此版本，下载和安装已停止。",
  local_install: "本机插件安装校验未通过，请在本地插件页检查签名信任和运行权限。",
  operation_failed: "市场操作未完成；原始错误内容已隐藏。",
} as const;

export type MarketplaceErrorCode = keyof typeof marketplaceMessages;
export class MarketplaceServiceError extends Error {
  readonly code: MarketplaceErrorCode;
  constructor(code: string) {
    const safeCode = Object.hasOwn(marketplaceMessages, code) ? code as MarketplaceErrorCode : "operation_failed";
    super(marketplaceMessages[safeCode]);
    this.name = "MarketplaceServiceError";
    this.code = safeCode;
  }
}

export function marketplaceErrorText(error: unknown): string {
  return new MarketplaceServiceError(error instanceof MarketplaceServiceError ? error.code : "operation_failed").message;
}

export function normalizeMarketplaceOrigin(input: string, allowDevelopmentHttp = false): string {
  const raw = input.trim();
  const authority = /^(https?):\/\/([^/?#]+)\/?$/.exec(raw);
  try {
    if (!authority || raw.length > 256 || /[\s\\]/.test(raw)) throw new Error();
    const url = new URL(raw);
    const localAuthority = /^(localhost|127\.0\.0\.1|\[::1\])(?::[0-9]+)?$/.test(authority[2]);
    if (url.username || url.password || !url.hostname || url.port === "0" || url.pathname !== "/" || url.search || url.hash
      || !(url.protocol === "https:" || allowDevelopmentHttp && localAuthority && url.protocol === "http:")) throw new Error();
    return url.origin;
  } catch { throw new MarketplaceServiceError("invalid_origin"); }
}

const encoder = new TextEncoder();
const maxEnvelopeBytes = maxPluginPackageBytes + 64 * 1024;
const hexDigest = /^[a-f0-9]{64}$/;
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
type JsonRecord = Record<string, unknown>;

function fail(code: MarketplaceErrorCode = "invalid_response"): never { throw new MarketplaceServiceError(code); }
function record(value: unknown): JsonRecord {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail();
  return value as JsonRecord;
}
function fields(value: unknown, required: readonly string[]): JsonRecord {
  const object = record(value);
  if (Object.keys(object).length !== required.length || required.some(key => !Object.hasOwn(object, key))) fail();
  return object;
}
function text(value: unknown, maximum = 4096): string {
  if (typeof value !== "string" || encoder.encode(value).length > maximum || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u202a-\u202e\u2066-\u2069\u200b\ufeff]/u.test(value)
    || /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(value)) fail();
  return value;
}
function integer(value: unknown, maximum = Number.MAX_SAFE_INTEGER): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0 || value > maximum) fail();
  return value;
}
function bool(value: unknown): boolean { if (typeof value !== "boolean") fail(); return value; }
function choice<Value extends string>(value: unknown, allowed: readonly Value[]): Value {
  if (typeof value !== "string" || !allowed.includes(value as Value)) fail();
  return value as Value;
}
function strings(value: unknown, maximum = 512, count = 16): string[] {
  if (!Array.isArray(value) || value.length > count) fail();
  return value.map(item => text(item, maximum));
}
function nullableText(value: unknown, maximum = 4096): string | null { return value === null ? null : text(value, maximum); }
function digest(value: unknown): string { const result = text(value, 64); if (!hexDigest.test(result)) fail(); return result; }
function submissionId(value: unknown): string { const result = text(value, 36); if (!uuid.test(result)) fail("invalid_request"); return result; }

export function parseStrictMarketplaceJson(source: string, maximum = maxPluginPackageBytes): unknown {
  if (typeof source !== "string" || encoder.encode(source).length > maximum) fail("invalid_package");
  let position = 0;
  let nodes = 0;
  const space = () => { while (position < source.length && /[ \t\r\n]/.test(source[position])) position++; };
  const readString = (): string => {
    const start = position++;
    while (position < source.length) {
      const character = source[position++];
      if (character === '"') return JSON.parse(source.slice(start, position)) as string;
      if (character === "\\") position++;
    }
    return fail("invalid_package");
  };
  const value = (depth: number): unknown => {
    if (++nodes > 100_000 || depth > 48) fail("invalid_package");
    space();
    const character = source[position];
    if (character === '"') return readString();
    if (character === "{" || character === "[") {
      position++;
      const object: JsonRecord = Object.create(null) as JsonRecord;
      const array: unknown[] = [];
      const seen = new Set<string>();
      const closing = character === "{" ? "}" : "]";
      space();
      if (source[position] === closing) { position++; return character === "{" ? object : array; }
      while (position < source.length) {
        space();
        if (character === "{") {
          if (source[position] !== '"') fail("invalid_package");
          const key = readString();
          if (seen.has(key) || ["__proto__", "constructor", "prototype"].includes(key)) fail("invalid_package");
          seen.add(key);
          space();
          if (source[position++] !== ":") fail("invalid_package");
          object[key] = value(depth + 1);
        } else array.push(value(depth + 1));
        space();
        const separator = source[position++];
        if (separator === closing) return character === "{" ? object : array;
        if (separator !== ",") fail("invalid_package");
      }
      fail("invalid_package");
    }
    const token = /^(?:true|false|null|-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)/.exec(source.slice(position));
    if (!token) fail("invalid_package");
    position += token[0].length;
    const scalar: unknown = JSON.parse(token[0]);
    if (typeof scalar === "number" && !Number.isFinite(scalar)) fail("invalid_package");
    return scalar;
  };
  try { const result = value(0); space(); if (position !== source.length) fail("invalid_package"); return result; }
  catch { throw new MarketplaceServiceError("invalid_package"); }
}

function pluginId(value: unknown): string {
  const id = text(value, 96);
  if (!id || id.split(".").some(part => part.length > 48 || !/^[a-z][a-z0-9-]*$/.test(part) || part.endsWith("-"))) fail("invalid_request");
  return id;
}
function version(value: unknown): string {
  const result = text(value, 64);
  const match = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(result);
  if (!match || match.slice(1, 4).some(part => BigInt(part) > 18446744073709551615n)
    || match[4]?.split(".").some(part => /^[0-9]+$/.test(part) && part.length > 1 && part.startsWith("0"))) fail("invalid_request");
  return result;
}
function dependency(value: unknown): string {
  const result = text(value, 161);
  const separator = result.indexOf("@");
  if (separator < 1) fail("invalid_package");
  pluginId(result.slice(0, separator));
  version(result.slice(separator + 1));
  return result;
}
function packagePath(path: string): string {
  const segments = path.split("/");
  if (!path || path.length > 240 || segments.length > 12 || segments.some(segment => !segment || segment.length > 96
    || segment.startsWith(".") || segment.endsWith(".") || !/^[A-Za-z0-9._-]+$/.test(segment)
    || /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/i.test(segment.split(".")[0]))) fail("invalid_package");
  return path;
}
function permissions(value: unknown): PluginPermissions {
  const object = fields(value, ["network", "readPaths", "writePaths", "process", "credentials"]);
  return { network: strings(object.network), readPaths: strings(object.readPaths), writePaths: strings(object.writePaths), process: bool(object.process), credentials: strings(object.credentials) };
}
function manifest(value: unknown): PluginManifest {
  const object = fields(value, ["schemaVersion", "id", "name", "description", "author", "version", "minimumHostVersion", "kind", "entry", "clients", "permissions", "dependencies"]);
  if (object.schemaVersion !== 1) fail("invalid_package");
  const clients = strings(object.clients, 32, pluginClients.length).map(client => choice(client, pluginClients));
  if (!clients.length || new Set(clients).size !== clients.length) fail("invalid_package");
  return { schemaVersion: 1, id: pluginId(object.id), name: text(object.name, 512), description: text(object.description, 16 * 1024),
    author: text(object.author, 320), version: version(object.version), minimumHostVersion: version(object.minimumHostVersion),
    kind: choice(object.kind, ["mcp", "skill", "provider"]), entry: packagePath(text(object.entry, 240)), clients,
    permissions: permissions(object.permissions), dependencies: strings(object.dependencies, 161, 32).map(dependency) };
}
function typedPackage(value: unknown): PluginPackage {
  const object = fields(value, ["manifest", "files"]);
  const typedManifest = manifest(object.manifest);
  const rawFiles = record(object.files);
  const names = Object.keys(rawFiles).sort();
  if (!names.length || names.length > 512 || !Object.hasOwn(rawFiles, typedManifest.entry)) fail("invalid_package");
  const files: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const name of names) files[packagePath(name)] = text(rawFiles[name], 256 * 1024);
  return { manifest: typedManifest, files };
}

export function canonicalMarketplacePackage(value: PluginPackage): string {
  const packageValue = typedPackage(value);
  const filesJson = Object.keys(packageValue.files).sort().map(path => `${JSON.stringify(path)}:${JSON.stringify(packageValue.files[path])}`).join(",");
  const canonical = `{"manifest":${JSON.stringify(packageValue.manifest)},"files":{${filesJson}}}`;
  if (encoder.encode(canonical).length > maxPluginPackageBytes) fail("invalid_package");
  return canonical;
}
export async function marketplacePackageDigest(packageValue: PluginPackage): Promise<string> {
  try { return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(canonicalMarketplacePackage(packageValue)))), byte => byte.toString(16).padStart(2, "0")).join(""); }
  catch (error) { throw error instanceof MarketplaceServiceError ? error : new MarketplaceServiceError("unavailable"); }
}

export function parseMarketplacePackage(source: string, secrets: readonly string[] = []): PluginPackage {
  try {
    const value = parseStrictMarketplaceJson(source);
    inspectMarketplacePrivacy(value, secrets);
    const packageValue = typedPackage(value);
    canonicalMarketplacePackage(packageValue);
    return packageValue;
  } catch (error) { throw new MarketplaceServiceError(error instanceof MarketplaceServiceError && error.code === "privacy_rejected" ? "privacy_rejected" : "invalid_package"); }
}

const privacyPatterns = [
  /-----BEGIN [A-Z ]*PRIVATE KEY-----/i,
  /\b(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|xox[baprs]-[A-Za-z0-9-]{10,})\b/,
  /\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b/,
  /\bbearer\s+[A-Za-z0-9._~+/-]{8,}/i,
  /\b[A-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Z0-9](?:[A-Z0-9-]*[A-Z0-9])?(?:\.[A-Z0-9](?:[A-Z0-9-]*[A-Z0-9])?)+\b/i,
  /(?:[A-Z]:[\\/]Users[\\/][^\\/\s"']+|\/(?:Users|home)\/[^/\s"']+\/)/i,
  /\b1[3-9][0-9]{9}\b/, /\+[1-9][0-9](?:[ .()-]?[0-9]){7,13}\b/, /\b[1-9][0-9]{16}[0-9Xx]\b/,
  /(?:data:[^\s,;]+;base64,|-----BEGIN (?:CERTIFICATE|OPENSSH PRIVATE KEY)-----)/i,
];
function placeholder(value: string): boolean {
  const trimmed = value.trim().replace(/^['"]|['"]$/g, "");
  return !trimmed || /^(null|None|undefined)$/.test(trimmed) || /^\$\{.*\}$|^\{\{.*\}\}$|^<.*>$/.test(trimmed)
    || /^(process\.env\.|os\.environ\[|std::env::var\(|\$env:)/.test(trimmed);
}
function base64Bytes(encoded: string): Uint8Array | null {
  if (encoded.length % 4 === 1) return null;
  const bytes: number[] = [];
  let pending = 0;
  let bits = 0;
  let padding = false;
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  for (const character of encoded.replace(/-/g, "+").replace(/_/g, "/")) {
    if (character === "=") { padding = true; continue; }
    const number = alphabet.indexOf(character);
    if (padding || number < 0) return null;
    pending = (pending << 6) | number;
    bits += 6;
    if (bits >= 8) { bits -= 8; bytes.push(pending >> bits); pending &= (1 << bits) - 1; }
  }
  return pending === 0 ? new Uint8Array(bytes) : null;
}
function unescapePrivacy(value: string): string {
  const bytes: number[] = [];
  for (let position = 0; position < value.length;) {
    const remaining = value.slice(position);
    const escaped = /^(?:\\u([a-f0-9]{4})|\\x([a-f0-9]{2})|%([a-f0-9]{2}))/i.exec(remaining);
    if (escaped) {
      const number = Number.parseInt(escaped[1] ?? escaped[2] ?? escaped[3], 16);
      if (escaped[1]) { if (number >= 0xd800 && number <= 0xdfff) fail("privacy_rejected"); bytes.push(...encoder.encode(String.fromCodePoint(number))); }
      else bytes.push(number);
      position += escaped[0].length;
    } else {
      const character = String.fromCodePoint(value.codePointAt(position)!);
      bytes.push(...encoder.encode(character));
      position += character.length;
    }
  }
  try { return new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(bytes)); }
  catch { return fail("privacy_rejected"); }
}

export function inspectMarketplacePrivacy(value: unknown, secrets: readonly string[] = []): void {
  let inspectionValue = value;
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const root = value as JsonRecord;
    if (root.manifest && typeof root.manifest === "object" && !Array.isArray(root.manifest)) {
      const metadata = root.manifest as JsonRecord;
      if (Array.isArray(metadata.dependencies)) {
        const dependencies = metadata.dependencies.map(item => {
          if (typeof item !== "string") return item;
          if (secrets.some(secret => secret.length > 0 && item.includes(secret))) fail("privacy_rejected");
          try { dependency(item); } catch { return item; }
          return item.split("@");
        });
        inspectionValue = { ...root, manifest: { ...metadata, dependencies } };
      }
    }
  }
  let nodes = 0;
  let joined = "";
  let joinedBytes = 0;
  const scan = (source: string, depth: number, rejectOpaqueEncoding = true): void => {
    if (depth > 4 || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u202a-\u202e\u2066-\u2069\u200b\ufeff]/u.test(source)
      || secrets.some(secret => secret.length > 0 && source.includes(secret)) || privacyPatterns.some(pattern => pattern.test(source))) fail("privacy_rejected");
    for (const assignment of source.matchAll(/\b(?:api[_ -]?key|access[_ -]?token|refresh[_ -]?token|client[_ -]?secret|password|passwd|secret|authorization|cookie|session[_ -]?token)["']?\s*[:=]\s*(?:["']([^"'\r\n]{1,256})["']|([^\s,;})\]]{1,256}))/gi)) {
      if (!placeholder(assignment[1] ?? assignment[2])) fail("privacy_rejected");
    }
    const unescaped = unescapePrivacy(source);
    if (unescaped !== source) scan(unescaped, depth + 1, rejectOpaqueEncoding);
    if ((source.startsWith("{") && source.endsWith("}")) || (source.startsWith("[") && source.endsWith("]"))) {
      let nested: unknown;
      try { nested = parseStrictMarketplaceJson(source, maxEnvelopeBytes); } catch { nested = undefined; }
      if (nested !== undefined) {
        const joinedOffset = joined.length;
        visit(nested, 0, depth + 1);
        scan(joined.slice(joinedOffset), depth + 1, false);
      }
    }
    for (const chunk of source.matchAll(/[A-Za-z0-9+/_-]{24,}={0,2}/g)) {
      const encoded = chunk[0];
      if (encoded.length % 2 === 0 && /^[a-f0-9]+$/i.test(encoded)) {
        try {
          const decoded = new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(encoded.match(/../g)!.map(pair => Number.parseInt(pair, 16))));
          if (!/[\u0000-\u001f\u007f]/u.test(decoded)) scan(decoded, depth + 1, rejectOpaqueEncoding);
        } catch (error) { if (error instanceof MarketplaceServiceError) throw error; }
      }
      const decodedBytes = base64Bytes(encoded);
      if (decodedBytes) {
        try {
          const decoded = new TextDecoder("utf-8", { fatal: true }).decode(decodedBytes);
          if (decoded !== encoded && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/u.test(decoded)) scan(decoded, depth + 1, rejectOpaqueEncoding);
          else if (rejectOpaqueEncoding && encoded.length >= 96) fail("privacy_rejected");
        } catch (error) { if (error instanceof MarketplaceServiceError) throw error; if (rejectOpaqueEncoding && encoded.length >= 96) fail("privacy_rejected"); }
      } else if (rejectOpaqueEncoding && encoded.length >= 256) fail("privacy_rejected");
    }
  };
  const visit = (node: unknown, depth: number, encodingDepth = 0): void => {
    if (++nodes > 100_000 || depth > 48) fail("privacy_rejected");
    if (typeof node === "string") {
      scan(node, encodingDepth);
      joinedBytes += encoder.encode(node).length;
      if (joinedBytes > maxEnvelopeBytes) fail("privacy_rejected");
      joined += node;
    } else if (Array.isArray(node)) node.forEach(item => visit(item, depth + 1, encodingDepth));
    else if (node && typeof node === "object") for (const [key, child] of Object.entries(node)) {
      visit(key, depth + 1, encodingDepth);
      if (/^(apikey|password|passwd|clientsecret|secret|accesstoken|refreshtoken|idtoken|authorization|cookie|setcookie|privatekey|sessiontoken|bearertoken)$/.test(key.replace(/[^a-z0-9]/gi, "").toLowerCase())
        && (typeof child !== "string" || !placeholder(child)) && child !== null) fail("privacy_rejected");
      visit(child, depth + 1, encodingDepth);
    }
  };
  const rawFiles = value && typeof value === "object" && "files" in value ? record(value.files) : {};
  for (const path of Object.keys(rawFiles)) {
    if (/(?:^|[/\\])(?:\.env|auth\.json|credentials(?:\.json)?|\.npmrc|\.netrc|id_rsa|id_ed25519|sessions\.json|history\.jsonl)$/i.test(path)
      || /\.(?:pem|key|p12|pfx|png|jpe?g|gif|webp|pdf|zip|gz|zst|exe|dll|so|wasm)$/i.test(path)) fail("privacy_rejected");
  }
  visit(inspectionValue, 0);
  scan(joined, 0, false);
}

function user(value: unknown): MarketplaceUser {
  const object = fields(value, ["userId", "username", "displayName", "reviewer"]);
  const displayName = text(object.displayName, 320);
  inspectMarketplacePrivacy(displayName);
  return { userId: submissionId(object.userId), username: text(object.username, 48), displayName, reviewer: bool(object.reviewer) };
}
function page<Item>(value: unknown, decode: (item: unknown) => Item): MarketplacePageResult<Item> {
  const object = fields(value, ["items", "limit", "offset", "hasMore"]);
  const limit = integer(object.limit, 100);
  if (!limit || !Array.isArray(object.items) || object.items.length > limit) fail();
  return { items: object.items.map(decode), limit, offset: integer(object.offset, 100_000), hasMore: bool(object.hasMore) };
}
function signingKeyId(value: unknown): string {
  const id = text(value, 64);
  if (!/^[A-Za-z0-9_.-]+$/.test(id)) fail();
  return id;
}
function signature(value: unknown): string {
  const result = text(value, 128);
  if (!/^[a-f0-9]{128}$/.test(result)) fail();
  return result;
}
function catalogEntry(value: unknown): MarketplaceCatalogEntry {
  const object = fields(value, ["id", "name", "description", "authorDisplayName", "version", "kind", "clients", "digest", "signature", "signingKeyId", "publishedAt"]);
  const result: MarketplaceCatalogEntry = { id: pluginId(object.id), name: text(object.name, 512), description: text(object.description, 16 * 1024),
    authorDisplayName: text(object.authorDisplayName, 320), version: version(object.version), kind: choice(object.kind, ["mcp", "skill", "provider"]),
    clients: strings(object.clients, 32, pluginClients.length).map(client => choice(client, pluginClients)), digest: digest(object.digest),
    signature: signature(object.signature), signingKeyId: signingKeyId(object.signingKeyId), publishedAt: integer(object.publishedAt) };
  inspectMarketplacePrivacy({ name: result.name, description: result.description, author: result.authorDisplayName });
  return result;
}
function submissionSummary(value: unknown): MarketplaceSubmissionSummary {
  const object = fields(value, ["submissionId", "pluginId", "name", "version", "digest", "status", "authorDisplayName", "createdAt", "updatedAt", "reviewNote", "previousPublishedVersion", "permissionChanges", "permissions", "clients"]);
  const result: MarketplaceSubmissionSummary = { submissionId: submissionId(object.submissionId), pluginId: pluginId(object.pluginId), name: text(object.name, 512),
    version: version(object.version), digest: digest(object.digest), status: choice(object.status, submissionStatuses), authorDisplayName: text(object.authorDisplayName, 320),
    createdAt: integer(object.createdAt), updatedAt: integer(object.updatedAt), reviewNote: nullableText(object.reviewNote),
    previousPublishedVersion: object.previousPublishedVersion === null ? null : version(object.previousPublishedVersion), permissionChanges: permissions(object.permissionChanges),
    permissions: permissions(object.permissions), clients: strings(object.clients, 32, pluginClients.length).map(client => choice(client, pluginClients)) };
  inspectMarketplacePrivacy({ name: result.name, author: result.authorDisplayName, note: result.reviewNote, permissions: result.permissions });
  return result;
}
async function submission(value: unknown, secrets: readonly string[]): Promise<MarketplaceSubmission> {
  const object = fields(value, ["submissionId", "status", "package", "digest", "authorDisplayName", "createdAt", "updatedAt", "reviewNote", "previousPublishedVersion", "permissionChanges"]);
  inspectMarketplacePrivacy(object.package, secrets);
  const packageValue = typedPackage(object.package);
  const actualDigest = digest(object.digest);
  if (await marketplacePackageDigest(packageValue) !== actualDigest || packageValue.manifest.author !== object.authorDisplayName) fail();
  const note = nullableText(object.reviewNote);
  inspectMarketplacePrivacy(note, secrets);
  return { submissionId: submissionId(object.submissionId), status: choice(object.status, submissionStatuses), package: packageValue, digest: actualDigest,
    authorDisplayName: text(object.authorDisplayName, 320), createdAt: integer(object.createdAt), updatedAt: integer(object.updatedAt), reviewNote: note,
    previousPublishedVersion: object.previousPublishedVersion === null ? null : version(object.previousPublishedVersion), permissionChanges: permissions(object.permissionChanges) };
}
async function published(value: unknown): Promise<PublishedPlugin> {
  const object = fields(value, ["package", "digest", "signature", "signingKeyId"]);
  inspectMarketplacePrivacy(object.package);
  const packageValue = typedPackage(object.package);
  const actualDigest = digest(object.digest);
  if (await marketplacePackageDigest(packageValue) !== actualDigest) fail();
  return { package: packageValue, digest: actualDigest, signature: signature(object.signature), signingKeyId: signingKeyId(object.signingKeyId) };
}
function withdrawalNotice(value: unknown, id: string, releaseVersion: string): PluginWithdrawal {
  const object = fields(value, ["id", "version", "digest", "reason", "signingKeyId", "signature"]);
  if (object.id !== id || object.version !== releaseVersion) fail();
  return { id: pluginId(object.id), version: version(object.version), digest: digest(object.digest), reason: choice(object.reason, ["security", "privacy", "withdrawn"]), signingKeyId: signingKeyId(object.signingKeyId), signature: signature(object.signature) };
}
function expected(request: MarketplaceVersionRequest): MarketplaceVersionRequest { return { version: version(request.version), digest: digest(request.digest) }; }
function listQuery(query: MarketplaceListQuery): string {
  const result = new URLSearchParams();
  if (query.limit !== undefined) { if (!query.limit) fail("invalid_request"); result.set("limit", String(integer(query.limit, 100))); }
  if (query.offset !== undefined) result.set("offset", String(integer(query.offset, 100_000)));
  if (query.status !== undefined) result.set("status", choice(query.status, submissionStatuses));
  return result.size ? `?${result}` : "";
}
function localError(error: unknown): MarketplaceServiceError {
  if (error instanceof MarketplaceServiceError) return error;
  if (error instanceof PluginServiceError && error.code === "plugin_untrusted_key") return new MarketplaceServiceError("untrusted_key");
  if (error instanceof PluginServiceError && error.code === "plugin_withdrawn") return new MarketplaceServiceError("withdrawn");
  return new MarketplaceServiceError("local_install");
}
function freeze<Value>(value: Value): Value {
  if (value && typeof value === "object") { for (const child of Object.values(value)) freeze(child); Object.freeze(value); }
  return value;
}

function auditEvent(value: unknown): MarketplaceAuditEvent {
  const object = fields(value, ["sequence", "actorDisplayName", "submissionId", "version", "digest", "action", "fromStatus", "toStatus", "note", "createdAt"]);
  const result = { sequence: integer(object.sequence), actorDisplayName: text(object.actorDisplayName, 320),
    submissionId: object.submissionId === null ? null : submissionId(object.submissionId), version: object.version === null ? null : version(object.version),
    digest: object.digest === null ? null : digest(object.digest), action: text(object.action, 96), fromStatus: nullableText(object.fromStatus, 32),
    toStatus: nullableText(object.toStatus, 32), note: nullableText(object.note), createdAt: integer(object.createdAt) };
  inspectMarketplacePrivacy({ actor: result.actorDisplayName, note: result.note });
  return result;
}

function reportRecord(value: unknown): MarketplaceReport {
  const object = fields(value, ["reportId", "pluginId", "version", "reporterDisplayName", "reason", "note", "status", "resolutionNote", "createdAt", "resolvedAt"]);
  const result: MarketplaceReport = { reportId: submissionId(object.reportId), pluginId: pluginId(object.pluginId), version: version(object.version),
    reporterDisplayName: text(object.reporterDisplayName, 320), reason: choice(object.reason, ["security", "privacy", "malware", "copyright", "other"]),
    note: text(object.note), status: choice(object.status, ["open", "resolved", "dismissed"]), resolutionNote: nullableText(object.resolutionNote),
    createdAt: integer(object.createdAt), resolvedAt: object.resolvedAt === null ? null : integer(object.resolvedAt) };
  inspectMarketplacePrivacy({ reporter: result.reporterDisplayName, note: result.note, resolution: result.resolutionNote });
  return result;
}

export class MarketplaceClient {
  readonly origin: string;
  #fetch: typeof globalThis.fetch;
  #now: () => number;
  #timeout: number;
  #token: string | null = null;
  #session: MarketplaceSession | null = null;
  #generation = 0;
  #disposed = false;
  #expiryTimer: ReturnType<typeof setTimeout> | undefined;
  #listeners = new Set<(session: MarketplaceSession | null) => void>();
  #requests = new Set<AbortController>();
  #plans = new WeakMap<MarketplaceInstallPlan, number>();

  constructor(origin: string, options: MarketplaceClientOptions = {}) {
    this.origin = normalizeMarketplaceOrigin(origin, options.allowDevelopmentHttp);
    this.#now = options.now ?? (() => Date.now());
    this.#timeout = options.requestTimeoutMs ?? 25_000;
    if (!Number.isSafeInteger(this.#timeout) || this.#timeout < 100 || this.#timeout > 60_000) fail("invalid_request");
    this.#fetch = options.fetch ?? createMarketplaceFetch({ origin: this.origin, allowDevelopmentHttp: options.allowDevelopmentHttp === true, timeoutMs: this.#timeout });
  }

  get session(): MarketplaceSession | null {
    if (this.#session && this.#session.expiresAt * 1000 <= this.#now()) this.#clearSession();
    return this.#session;
  }

  subscribe(listener: (session: MarketplaceSession | null) => void): () => void {
    if (this.#disposed) fail("session_changed");
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  }

  dispose(): void {
    this.#disposed = true;
    this.#clearSession();
    this.#listeners.clear();
  }

  #clearSession(): void {
    this.#generation++;
    this.#token = null;
    this.#session = null;
    clearTimeout(this.#expiryTimer);
    this.#expiryTimer = undefined;
    for (const controller of this.#requests) controller.abort();
    for (const listener of this.#listeners) { try { listener(null); } catch {} }
  }

  #guard(generation: number): void {
    if (this.#disposed || this.#generation !== generation) fail("session_changed");
  }

  #secrets(): string[] {
    const session = this.session;
    if (!session || !this.#token) fail("authentication_required");
    return [this.#token, session.user.userId];
  }

  #note(value = ""): string {
    const note = text(value);
    inspectMarketplacePrivacy(note, this.#secrets());
    return note;
  }

  async #request(path: string, method: "GET" | "POST", body: unknown, authenticated: boolean, tokenOverride?: string): Promise<unknown> {
    if (this.#disposed) fail("session_changed");
    if (authenticated && !tokenOverride) this.#secrets();
    const generation = this.#generation;
    const token = tokenOverride ?? (authenticated ? this.#token : null);
    const controller = new AbortController();
    this.#requests.add(controller);
    const timer = setTimeout(() => controller.abort(), this.#timeout);
    try {
      const encoded = body === undefined ? undefined : JSON.stringify(body);
      if (encoded && encoder.encode(encoded).length > maxEnvelopeBytes) fail("invalid_request");
      const response = await this.#fetch(`${this.origin}${path}`, { method, body: encoded, signal: controller.signal,
        headers: { Accept: "application/json", ...(encoded ? { "Content-Type": "application/json" } : {}), ...(token ? { Authorization: `Bearer ${token}` } : {}) },
        credentials: "omit", redirect: "error", cache: "no-store", referrerPolicy: "no-referrer" });
      this.#guard(generation);
      if (response.redirected || response.url && new URL(response.url).origin !== this.origin) fail("invalid_response");
      if (response.status === 401 && authenticated) { this.#clearSession(); fail("authentication_required"); }
      if (response.status === 204 && response.ok) return undefined;
      if (!/^application\/json(?:\s*;|$)/i.test(response.headers.get("content-type") ?? "")) fail("invalid_response");
      const advertised = response.headers.get("content-length");
      if (advertised && (!/^[0-9]+$/.test(advertised) || Number(advertised) > maxEnvelopeBytes)) fail("invalid_response");
      let source = "";
      let received = 0;
      const decoder = new TextDecoder("utf-8", { fatal: true });
      if (response.body) {
        const reader = response.body.getReader();
        try {
          for (;;) {
            const chunk = await reader.read();
            if (chunk.done) break;
            received += chunk.value.byteLength;
            if (received > maxEnvelopeBytes) { controller.abort(); fail("invalid_response"); }
            source += decoder.decode(chunk.value, { stream: true });
          }
          source += decoder.decode();
        } finally { reader.releaseLock(); }
      } else source = await response.text();
      this.#guard(generation);
      let result: unknown;
      try { result = parseStrictMarketplaceJson(source, maxEnvelopeBytes); } catch { fail("invalid_response"); }
      if (!response.ok) {
        const code = record(record(result).error).code;
        if (typeof code !== "string") fail("invalid_response");
        throw new MarketplaceServiceError(code === "service_unavailable" || code === "invalid_configuration" ? "unavailable" : code);
      }
      return result;
    } catch (error) {
      if (error instanceof MarketplaceServiceError) throw error;
      this.#guard(generation);
      if (error instanceof MarketplaceTransportError) throw new MarketplaceServiceError(error.code);
      throw new MarketplaceServiceError("network");
    } finally { clearTimeout(timer); this.#requests.delete(controller); }
  }

  async #perform<Result>(path: string, authenticated: boolean, decode: (value: unknown) => Result | Promise<Result>, body?: unknown): Promise<Result> {
    if (authenticated) this.#secrets();
    const generation = this.#generation;
    const value = await this.#request(path, body === undefined ? "GET" : "POST", body, authenticated);
    const result = await decode(value);
    this.#guard(generation);
    return freeze(result);
  }

  async #authenticate(username: string, password: string, displayName?: string): Promise<MarketplaceSession> {
    if (!/^[A-Za-z0-9_-]{3,32}$/.test(username) || encoder.encode(password).length < 12 || encoder.encode(password).length > 128
      || /[\u0000-\u001f\u007f]/u.test(password)) fail("invalid_request");
    if (displayName !== undefined) { if (!text(displayName, 160).trim()) fail("invalid_request"); inspectMarketplacePrivacy(displayName); }
    this.#clearSession();
    const generation = this.#generation;
    const result = await this.#request(displayName === undefined ? "/v1/auth/login" : "/v1/auth/register", "POST",
      { username, password, ...(displayName === undefined ? {} : { displayName }) }, false);
    const object = fields(result, ["token", "tokenType", "expiresAt", "user"]);
    const token = digest(object.token);
    if (object.tokenType !== "Bearer") fail("invalid_response");
    const expiresAt = integer(object.expiresAt);
    const sessionUser = user(object.user);
    if (expiresAt * 1000 <= this.#now() || expiresAt * 1000 > this.#now() + 604_801_000 || sessionUser.username !== username.toLowerCase()) fail("invalid_response");
    this.#guard(generation);
    this.#token = token;
    this.#session = freeze({ expiresAt, user: sessionUser });
    this.#expiryTimer = setTimeout(() => this.#clearSession(), expiresAt * 1000 - this.#now());
    for (const listener of this.#listeners) { try { listener(this.#session); } catch {} }
    return this.#session;
  }

  register(username: string, displayName: string, password: string): Promise<MarketplaceSession> { return this.#authenticate(username, password, displayName); }
  login(username: string, password: string): Promise<MarketplaceSession> { return this.#authenticate(username, password); }

  async logout(): Promise<void> {
    const token = this.#token;
    this.#clearSession();
    if (token && !this.#disposed) await this.#request("/v1/auth/logout", "POST", {}, false, token);
  }

  async refreshIdentity(): Promise<MarketplaceSession> {
    const sessionUser = await this.#perform("/v1/auth/me", true, user);
    if (!this.#session) fail("authentication_required");
    this.#session = freeze({ ...this.#session, user: sessionUser });
    for (const listener of this.#listeners) { try { listener(this.#session); } catch {} }
    return this.#session;
  }

  catalog(query: MarketplaceCatalogQuery = {}): Promise<MarketplacePageResult<MarketplaceCatalogEntry>> {
    const params = new URLSearchParams(listQuery({ limit: query.limit, offset: query.offset }).slice(1));
    if (query.client !== undefined) params.set("client", choice(query.client, pluginClients));
    if (query.kind !== undefined) params.set("kind", choice(query.kind, ["mcp", "skill", "provider"]));
    if (query.q !== undefined) { const search = text(query.q, 160); inspectMarketplacePrivacy(search); params.set("q", search); }
    return this.#perform(`/v1/plugins${params.size ? `?${params}` : ""}`, false, value => page(value, catalogEntry));
  }

  versions(id: string, query: MarketplaceListQuery = {}): Promise<MarketplacePageResult<MarketplaceCatalogEntry>> {
    if (query.status !== undefined) fail("invalid_request");
    return this.#perform(`/v1/plugins/${pluginId(id)}/versions${listQuery(query)}`, false, value => page(value, catalogEntry));
  }

  async download(id: string, releaseVersion: string): Promise<PublishedPlugin> {
    const result = await this.#perform(`/v1/plugins/${pluginId(id)}/versions/${version(releaseVersion)}`, false, published);
    if (result.package.manifest.id !== id || result.package.manifest.version !== releaseVersion) fail("invalid_response");
    return result;
  }

  async withdrawal(id: string, releaseVersion: string): Promise<PluginWithdrawal | null> {
    try { return await this.#perform(`/v1/plugins/${pluginId(id)}/versions/${version(releaseVersion)}/withdrawal`, false, value => withdrawalNotice(value, id, releaseVersion)); }
    catch (error) { if (error instanceof MarketplaceServiceError && error.code === "not_found") return null; throw error; }
  }

  async submit(source: string | PluginPackage, replacesSubmissionId?: string): Promise<MarketplaceSubmission> {
    const secrets = this.#secrets();
    let sourceText: string;
    try { sourceText = typeof source === "string" ? source : JSON.stringify(source); } catch { return fail("invalid_package"); }
    const packageValue = parseMarketplacePackage(sourceText, secrets);
    if (packageValue.manifest.author !== this.session?.user.displayName) fail("invalid_request");
    const canonical = canonicalMarketplacePackage(packageValue);
    const result = await this.#perform("/v1/submissions", true, value => submission(value, secrets), {
      package: packageValue, ...(replacesSubmissionId === undefined ? {} : { replacesSubmissionId: submissionId(replacesSubmissionId) }),
    });
    if (canonicalMarketplacePackage(result.package) !== canonical || result.status !== "submitted") fail("invalid_response");
    return result;
  }

  submissions(query: MarketplaceListQuery = {}): Promise<MarketplacePageResult<MarketplaceSubmissionSummary>> {
    return this.#perform(`/v1/submissions${listQuery(query)}`, true, value => page(value, submissionSummary));
  }

  reviewQueue(query: MarketplaceListQuery = {}): Promise<MarketplacePageResult<MarketplaceSubmissionSummary>> {
    return this.#perform(`/v1/review/submissions${listQuery(query)}`, true, value => page(value, submissionSummary));
  }

  submission(id: string): Promise<MarketplaceSubmission> {
    const secrets = this.#secrets();
    return this.#perform(`/v1/submissions/${submissionId(id)}`, true, value => submission(value, secrets));
  }

  decide(id: string, request: MarketplaceDecisionRequest): Promise<MarketplaceSubmission> {
    const secrets = this.#secrets();
    return this.#perform(`/v1/review/submissions/${submissionId(id)}/decision`, true, value => submission(value, secrets), {
      ...expected(request), decision: choice(request.decision, ["approve", "request_changes", "reject"]), note: this.#note(request.note),
    });
  }

  publish(id: string, request: MarketplaceVersionRequest): Promise<PublishedPlugin> {
    return this.#perform(`/v1/submissions/${submissionId(id)}/publish`, true, published, expected(request));
  }

  withdraw(id: string, request: MarketplaceWithdrawalRequest): Promise<MarketplaceSubmission> {
    const secrets = this.#secrets();
    return this.#perform(`/v1/submissions/${submissionId(id)}/withdraw`, true, value => submission(value, secrets), {
      ...expected(request), reason: choice(request.reason, ["security", "privacy", "withdrawn"]), note: this.#note(request.note),
    });
  }

  submissionAudit(id: string, query: MarketplaceListQuery = {}): Promise<MarketplacePageResult<MarketplaceAuditEvent>> {
    return this.#perform(`/v1/submissions/${submissionId(id)}/audit${listQuery(query)}`, true, value => page(value, auditEvent));
  }

  reviewAudit(query: MarketplaceListQuery = {}): Promise<MarketplacePageResult<MarketplaceAuditEvent>> {
    return this.#perform(`/v1/review/audit${listQuery(query)}`, true, value => page(value, auditEvent));
  }

  report(id: string, releaseVersion: string, reason: MarketplaceReport["reason"], note = ""): Promise<MarketplaceReport> {
    return this.#perform(`/v1/plugins/${pluginId(id)}/versions/${version(releaseVersion)}/reports`, true, reportRecord,
      { reason: choice(reason, ["security", "privacy", "malware", "copyright", "other"]), note: this.#note(note) });
  }

  reports(query: { limit?: number; offset?: number; status?: MarketplaceReport["status"] } = {}): Promise<MarketplacePageResult<MarketplaceReport>> {
    const params = new URLSearchParams(listQuery({ limit: query.limit, offset: query.offset }).slice(1));
    if (query.status) params.set("status", choice(query.status, ["open", "resolved", "dismissed"]));
    return this.#perform(`/v1/review/reports${params.size ? `?${params}` : ""}`, true, value => page(value, reportRecord));
  }

  resolveReport(id: string, resolution: "resolved" | "dismissed", note = ""): Promise<MarketplaceReport> {
    return this.#perform(`/v1/review/reports/${submissionId(id)}/resolve`, true, reportRecord,
      { resolution: choice(resolution, ["resolved", "dismissed"]), note: this.#note(note) });
  }

  async prepareInstall(id: string, releaseVersion: string): Promise<MarketplaceInstallPlan> {
    const generation = this.#generation;
    try {
      const release = await this.download(id, releaseVersion);
      const preview = await previewPluginInstall(JSON.stringify(release));
      this.#guard(generation);
      if (preview.validation.digest !== release.digest || preview.signingKeyId !== release.signingKeyId
        || !hexDigest.test(preview.signingKeyFingerprint) || preview.expiresAt * 1000 <= this.#now()
        || JSON.stringify(manifest(preview.validation.manifest)) !== JSON.stringify(release.package.manifest)) fail("invalid_response");
      const plan = freeze({ origin: this.origin, published: release, preview });
      this.#plans.set(plan, generation);
      return plan;
    } catch (error) {
      if (error instanceof MarketplaceServiceError && error.code === "not_found") {
        const notice = await this.withdrawal(id, releaseVersion);
        if (notice) { try { await applyPluginWithdrawal(JSON.stringify(notice)); } catch (cause) { throw localError(cause); } fail("withdrawn"); }
      }
      throw localError(error);
    }
  }

  async #checkWithdrawal(plan: MarketplaceInstallPlan): Promise<void> {
    const notice = await this.withdrawal(plan.published.package.manifest.id, plan.published.package.manifest.version);
    if (!notice) return;
    if (notice.digest !== plan.published.digest) fail("invalid_response");
    try { await applyPluginWithdrawal(JSON.stringify(notice)); } catch (error) { throw localError(error); }
    fail("withdrawn");
  }

  async confirmInstall(plan: MarketplaceInstallPlan, options: MarketplaceInstallOptions): Promise<InstalledPlugin> {
    const generation = this.#plans.get(plan);
    if (generation === undefined || plan.origin !== this.origin) fail("confirmation");
    this.#guard(generation);
    if (!options.confirmed || !Array.isArray(options.clients) || !options.clients.length || new Set(options.clients).size !== options.clients.length
      || options.clients.some(client => !plan.published.package.manifest.clients.includes(client)) || plan.preview.expiresAt * 1000 <= this.#now()
      || options.enable !== undefined && typeof options.enable !== "boolean") fail("confirmation");
    this.#plans.delete(plan);
    try {
      await this.#checkWithdrawal(plan);
      const current = await this.download(plan.published.package.manifest.id, plan.published.package.manifest.version);
      if (current.digest !== plan.published.digest || current.signature !== plan.published.signature || current.signingKeyId !== plan.published.signingKeyId
        || canonicalMarketplacePackage(current.package) !== canonicalMarketplacePackage(plan.published.package)) fail("conflict");
      await this.#checkWithdrawal(plan);
      this.#guard(generation);
      if (plan.preview.expiresAt * 1000 <= this.#now()) fail("confirmation");
      const installed = await confirmPluginInstall({ previewId: plan.preview.previewId, digest: plan.published.digest,
        acceptedPermissions: plan.published.package.manifest.permissions, clients: [...options.clients], confirmed: true, enable: options.enable ?? false });
      await this.#checkWithdrawal(plan);
      this.#guard(generation);
      return installed;
    } catch (error) { throw localError(error); }
  }
}
