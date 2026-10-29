import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";
import type { UsageFilter } from "../types/usage";
import type { ModelFingerprint, PassiveAuditReport, Pretokenizer, ReferenceAnalysisRequest, ReferenceReport, ShadowAuditReport, ShadowAuditSetting, ShadowAuditStatus } from "../types/model-audit";
import { normalizeReferenceReport } from "./model-audit";

function requireDesktop(): void {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error(t("请在 XY Switch 桌面应用中分析本地审计记录。网页预览没有本地数据。"));
  }
}

export async function analyzeModelAudit(filter: UsageFilter): Promise<PassiveAuditReport> {
  requireDesktop();
  return invoke<PassiveAuditReport>("analyze_model_audit", { filter });
}

export async function loadModelReference(path: string): Promise<ModelFingerprint> {
  requireDesktop();
  return invoke<ModelFingerprint>("load_model_reference", { path });
}

export async function generateModelReference(request: { tokenizerPath: string; model: string; pretokenizer?: Pretokenizer }): Promise<ModelFingerprint> {
  requireDesktop();
  return invoke<ModelFingerprint>("generate_model_reference", { request });
}

export async function analyzeModelReference(request: ReferenceAnalysisRequest): Promise<ReferenceReport> {
  requireDesktop();
  return normalizeReferenceReport(await invoke<unknown>("analyze_model_reference", { request }));
}

export async function getShadowAuditSettings(): Promise<ShadowAuditStatus[]> {
  requireDesktop();
  return invoke<ShadowAuditStatus[]>("get_shadow_audit_settings");
}

export async function saveShadowAuditSetting(request: ShadowAuditSetting): Promise<ShadowAuditStatus> {
  requireDesktop();
  return invoke<ShadowAuditStatus>("save_shadow_audit_setting", { request });
}

export async function analyzeShadowAudit(filter: UsageFilter): Promise<ShadowAuditReport> {
  requireDesktop();
  return invoke<ShadowAuditReport>("analyze_shadow_audit", { filter });
}
