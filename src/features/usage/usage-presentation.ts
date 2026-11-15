import { localeTag } from "../../i18n/language";
import type { UsageLogEntry } from "../../types/usage";
import { forwardingStatusLabel } from "./RequestDiagnosticsPanel";
import { formatTokens, formatUsd } from "./usage-format";
import { t } from "../../i18n/language";

export const costLabel = (value: string | number | null | undefined) => {
  const formatted = formatUsd(value);
  return formatted === "—" ? t("未知") : formatted;
};
export const partialLabel = (value: string, partial: boolean) => partial ? t("{value}（已知部分）", { value }) : value;
export const knownTokenValue = (value: number | null | undefined, knownCount = 0) => value != null && Number.isFinite(value) && Number.isFinite(knownCount) && knownCount > 0 ? value : null;
export const tokenLabel = (value: number | null | undefined, knownCount = 0, totalCount = 1) => {
  const known = knownTokenValue(value, knownCount);
  return known === null ? t("未知") : partialLabel(formatTokens(known), knownCount < totalCount);
};
export const inputOutputTokens = (row: Pick<UsageLogEntry, "inputTokens" | "outputTokens" | "tokenCoverage">) => {
  const input = knownTokenValue(row.inputTokens, row.tokenCoverage?.input);
  const output = knownTokenValue(row.outputTokens, row.tokenCoverage?.output);
  return {
    value: input === null && output === null ? null : (input ?? 0) + (output ?? 0),
    knownCount: (input === null ? 0 : row.tokenCoverage!.input) + (output === null ? 0 : row.tokenCoverage!.output),
  };
};
export const aggregateCostLabel = (value: string | number | null | undefined, unknownCount?: number, totalCount = 0) => {
  const formatted = formatUsd(value);
  if (formatted === "—" || (totalCount > 0 && unknownCount != null && unknownCount >= totalCount)) return t("未知");
  return partialLabel(formatted, unknownCount == null ? totalCount > 0 : unknownCount > 0);
};
export const isSessionRow = (row: UsageLogEntry) => row.source === "harness_session" || row.source === "codex_session" || row.providerId.startsWith("local:");
export const durationLabel = (value: number) => value > 0 ? Math.round(value).toLocaleString(localeTag()) + " ms" : "—";
export const statusLabel = (row: UsageLogEntry) => isSessionRow(row) ? (row.statusCode === 200 ? t("已完成") : t("中断 / 失败")) : forwardingStatusLabel(row);
