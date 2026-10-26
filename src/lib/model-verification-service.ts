import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_VERIFICATION_SETTINGS, VERIFICATION_TOKEN_BUDGET_RANGE, type VerificationSettings, type VerificationStatus } from "../types/model-verification";
const desktop = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export async function getVerificationStatus(providerId: string, model: string): Promise<VerificationStatus> {
  if (!desktop()) return { settings: { ...DEFAULT_VERIFICATION_SETTINGS, aliases: [] }, recent: [], observedResponses: 0,
    probeRequestsToday: 0, probeTokensToday: 0, budgetUsedToday: 0, capabilityConfirmed: false, droppedObservations: 0, storageError: false };
  return invoke("model_verification_status", { providerId, model });
}
export async function saveVerificationSettings(settings: VerificationSettings): Promise<VerificationSettings> {
  if (!desktop()) throw new Error(t("请在桌面应用中设置模型验证；网页预览不会开启或发送复核。"));
  const budget = settings.softTokenBudgetPerDay;
  if (!Number.isInteger(budget) || budget < VERIFICATION_TOKEN_BUDGET_RANGE.min || budget > VERIFICATION_TOKEN_BUDGET_RANGE.max) {
    throw new Error(t("每日软 Token 预算必须为 0–10000 的整数；0 表示不允许收费复核。"));
  }
  return invoke("set_model_verification_settings", { settings });
}
export async function confirmVerificationCost(providerId: string, model: string, confirmed: boolean): Promise<void> {
  if (!desktop()) throw new Error(t("请在桌面应用中确认此路由的费用边界。"));
  return invoke("confirm_model_verification_cost", { providerId, model, confirmed });
}
