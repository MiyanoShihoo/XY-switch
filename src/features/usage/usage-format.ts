import { localeTag } from "../../i18n/language";
import type { UsageFilter } from "../../types/usage";

export function formatTokens(value: number): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat(localeTag(), { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

export function formatUsd(value: string | number | null | undefined): string {
  if (value === null || value === undefined || (typeof value === "string" && value.trim() === "")) return "—";
  const number = Number(value);
  return Number.isFinite(number) ? "$" + number.toFixed(4) : "—";
}

export function normalizeUsageFilter(filter: UsageFilter): UsageFilter {
  return Object.fromEntries(Object.entries(filter).map(([key, value]) => [key, typeof value === "string" ? value.trim() : value]).filter(([, value]) => value !== "" && value !== undefined));
}

export function lastUsagePage(total: number, pageSize: number): number {
  const count = Number.isFinite(total) ? Math.max(0, total) : 0;
  const size = Number.isFinite(pageSize) ? Math.max(1, Math.floor(pageSize)) : 1;
  return Math.max(0, Math.ceil(count / size) - 1);
}
