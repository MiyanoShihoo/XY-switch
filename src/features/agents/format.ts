export const count = (value: number | null | undefined, locale: string) => value == null || !Number.isFinite(value) ? "—" : new Intl.NumberFormat(locale).format(value);
export const compactCount = (value: number, locale: string) => new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(value);
export const money = (value: number | null, locale: string) => value == null ? "—" : new Intl.NumberFormat(locale, { style: "currency", currency: "USD", maximumFractionDigits: 6 }).format(value);
export const isValidTimestamp = (value: number | null | undefined): value is number => value != null && Number.isFinite(value) && Math.abs(value) <= 8_640_000_000_000_000;
export const isoTimestamp = (value: number | null | undefined) => isValidTimestamp(value) ? new Date(value).toISOString() : undefined;
export const calendarDate = (value: number | null | undefined, locale: string) => isValidTimestamp(value) ? new Intl.DateTimeFormat(locale, { month: "2-digit", day: "2-digit" }).format(value) : "—";
export function timestamp(value: number | null | undefined, locale: string, short = false) {
  if (!isValidTimestamp(value)) return "—";
  return new Intl.DateTimeFormat(locale, short
    ? { hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" }
    : { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23", timeZoneName: "short" }).format(value);
}
export function elapsed(start: number | null, end: number | null, now: number) {
  if (!isValidTimestamp(start) || !isValidTimestamp(end ?? now)) return "—";
  const seconds = Math.max(0, Math.floor(((end ?? now) - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m`;
  return `${Math.floor(seconds / 86400)}d ${Math.floor(seconds % 86400 / 3600)}h`;
}
export function localDateInput(value: number | null) {
  if (!isValidTimestamp(value)) return "";
  const date = new Date(value);
  const localTime = value - date.getTimezoneOffset() * 60000;
  if (!isValidTimestamp(localTime) || date.getFullYear() < 1 || date.getFullYear() > 9999) return "";
  return new Date(localTime).toISOString().slice(0, 19);
}
export function parseDateInput(value: string) {
  if (!value) return undefined;
  const parsed = new Date(value).getTime();
  return isValidTimestamp(parsed) ? parsed : undefined;
}
export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : typeof error === "string" ? error : "Unknown observation error";
}
