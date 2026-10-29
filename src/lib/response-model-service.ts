import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";
import type { ResponseModelProbe } from "../types/response-model";

export async function probeProviderResponseModel(providerId: string, model: string, anomalyId?: string): Promise<ResponseModelProbe> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error(t("请在 XY Switch 桌面应用中探测上游响应。网页预览不会发送探测请求。"));
  }
  if (!anomalyId) throw new Error(t("请先选择一条被动异常响应；不能绕过验证开关和预算直接探测。"));
  return invoke<ResponseModelProbe>("probe_provider_response_model", { request: { providerId, model, anomalyId } });
}
