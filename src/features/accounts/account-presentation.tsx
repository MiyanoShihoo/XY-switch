import { t, useLanguage, type Language } from "../../i18n/language";
import type { AccountInfo, AccountStatus, AuthorizationStatus, QuotaSnapshot, QuotaSource, QuotaWindow } from "../../types/accounts";

export function safeAccountLabel(value: unknown, fallback = t("未命名账号")): string {
  if (typeof value !== "string" || !value || value.trim() !== value || [...value].length > 80
    || /[\u0000-\u001f\u007f@]/.test(value)
    || /(?:\bBearer\s+|\b(?:sk|sess|rk)-[\w-]{8,}|\beyJ[\w-]+\.[\w-]+|https?:\/\/)/i.test(value)) return fallback;
  return value;
}

export function validAccountAlias(value: string): boolean {
  return safeAccountLabel(value, "") === value && value.length > 0;
}

export function accountDestinationSnapshot(value: unknown): string | null {
  if (typeof value !== "string" || !value || value.length > 4096 || value.trim() !== value
    || /[\s\u0000-\u001f\u007f-\u009f@]/.test(value)) return null;
  try {
    const url = new URL(value);
    const decoded = decodeURIComponent(value);
    if (!["http:", "https:"].includes(url.protocol) || !url.hostname || url.username || url.password
      || value.includes("?") || value.includes("#") || /[@\u0000-\u001f\u007f-\u009f]/.test(decoded)
      || /(?:\bBearer\s+|\b(?:sk|sess|rk)-[\w-]{8,}|\beyJ[\w-]+\.[\w-]+)/i.test(decoded)) return null;
    return value;
  } catch { return null; }
}

export function accountDestinationLabel(value: unknown): string {
  const snapshot = accountDestinationSnapshot(value);
  return snapshot ? new URL(snapshot).origin : t("请求地址不可用于账号授权");
}

export function nativeAccountScopeMessage(): string {
  return t("绑定用于同步原生账号与额度，登录仍由原生客户端管理。当前这些引用尚不参与网关的 HTTP 账号轮换。");
}

export function accountStatusLabel(status: AccountStatus, kind?: AccountInfo["kind"]): string {
  switch (status) {
    case "available": return kind === "nativeManaged" ? t("原生状态已同步") : t("可参与选择");
    case "refreshing": return kind === "nativeManaged" ? t("正在同步原生状态") : t("正在刷新凭据");
    case "refreshRequired": return t("需要刷新凭据");
    case "nativeCheckRequired": return t("原生状态需要重新同步");
    case "loginRequired": return t("需要重新授权");
    case "destinationApprovalRequired": return t("请求地址待批准");
    case "quotaLimited": return t("可信额度已耗尽");
    case "coolingDown": return t("冷却中");
    case "disabled": return t("已停用");
    case "revoked": return t("本地引用已撤销");
    default: return t("状态未知");
  }
}

export function authorizationLabel(status: AuthorizationStatus): string {
  switch (status) {
    case "authorized": return t("已有授权证据");
    case "loginRequired": return t("需要重新授权");
    default: return t("尚未验证授权");
  }
}

export function accountKindLabel(kind: AccountInfo["kind"]): string {
  if (kind === "apiKey") return t("API key");
  if (kind === "oauth") return t("OAuth 授权");
  return t("原生客户端管理");
}

export function quotaSourceLabel(source: QuotaSource): string {
  switch (source) {
    case "providerResponse": return t("供应商响应");
    case "providerAccountApi": return t("供应商账号接口");
    case "nativeClient": return t("原生客户端");
    default: return t("未知来源");
  }
}

export function accountTime(value: number | null, language: Language): string {
  if (value === null || !Number.isSafeInteger(value) || value < 0) return t("未知");
  const date = new Date(value * 1000);
  return Number.isNaN(date.getTime()) ? t("未知") : date.toLocaleString(language === "en" ? "en-US" : "zh-CN");
}

export function knownQuotaWindows(snapshot: QuotaSnapshot | null, timestamp: number): QuotaWindow[] {
  if (!snapshot || !["providerResponse", "providerAccountApi", "nativeClient"].includes(snapshot.source)
    || !Number.isSafeInteger(snapshot.observedAt) || !Number.isSafeInteger(snapshot.validUntil)
    || snapshot.observedAt < timestamp - 86400 || snapshot.observedAt > timestamp + 5
    || snapshot.validUntil <= timestamp || snapshot.validUntil <= snapshot.observedAt
    || snapshot.validUntil - snapshot.observedAt > 86400 || !Array.isArray(snapshot.windows)
    || snapshot.windows.length === 0 || snapshot.windows.length > 16) return [];
  const invalid = snapshot.windows.some(window =>
    !["requests", "tokens", "percent"].includes(window.unit)
    || (window.remaining !== null && (!Number.isFinite(window.remaining) || window.remaining < 0))
    || (window.limit !== null && (!Number.isFinite(window.limit) || window.limit <= 0))
    || (window.remaining !== null && window.limit !== null && window.remaining > window.limit)
    || (window.unit === "percent" && ((window.remaining !== null && window.remaining > 100) || (window.limit !== null && window.limit > 100)))
    || (window.resetsAt !== null && (!Number.isSafeInteger(window.resetsAt) || window.resetsAt <= snapshot.observedAt)));
  return invalid ? [] : snapshot.windows.filter(window => window.resetsAt === null || window.resetsAt > timestamp);
}

export function accountCanRefresh(account: AccountInfo, timestamp: number): boolean {
  return account.kind === "oauth" && account.destinationApproved === true && account.canRefresh && account.allowRefresh && account.enabled
    && !["revoked", "refreshing", "loginRequired", "destinationApprovalRequired"].includes(account.status)
    && (account.cooldownUntil === null || account.cooldownUntil <= timestamp);
}

export function localRevocationMessage(result: { localReferenceRevoked: boolean; credentialsRemoved: boolean; account: { cleanupPending: boolean } }): string {
  if (!result.localReferenceRevoked) return t("本地撤销尚未完成，请刷新列表。" );
  if (!result.credentialsRemoved || result.account.cleanupPending) return t("本地引用已撤销；安全存储清理待完成，可重试本地撤销。" );
  return t("本地引用已撤销，已清理本应用凭据；供应商授权与客户端登录未退出。" );
}

export function AccountQuotaView({ quota, timestamp }: { quota: QuotaSnapshot | null; timestamp: number }) {
  const language = useLanguage();
  const windows = knownQuotaWindows(quota, timestamp);
  if (!windows.length) return <p className="accounts-quota-unknown">{t("额度未知；未收到有效证据，不能视为零额度。")}</p>;
  return <div className="accounts-quota">
    <p>{t("额度来源：{source}", { source: quotaSourceLabel(quota!.source) })}</p>
    {windows.map((window, index) => <div key={index} className="accounts-quota__window">
      <strong>{t("额度窗口 {index}", { index: index + 1 })}</strong>
      <span>{t("剩余 {remaining} / 上限 {limit}", { remaining: window.remaining ?? t("未知"), limit: window.limit ?? t("未知") })} · {window.unit === "requests" ? t("请求数") : window.unit === "tokens" ? t("令牌数") : t("百分比")}</span>
      <span>{t("重置时间：{time}", { time: accountTime(window.resetsAt, language) })}</span>
    </div>)}
    <p>{t("证据有效至：{time}", { time: accountTime(quota!.validUntil, language) })}</p>
  </div>;
}
