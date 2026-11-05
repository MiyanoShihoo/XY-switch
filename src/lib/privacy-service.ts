import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type { PrivacyInspection, PrivacySettings } from "../types/privacy";

export const defaultPrivacySettings: PrivacySettings = {
  redactPersonalInformation: true,
  blockUninspectedAttachments: true,
};
export const maxPrivacyPreviewBytes = 256 * 1024;

function requireDesktop() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error(t("隐私策略和本地检查需要桌面应用。"));
  }
}

export async function loadPrivacySettings(): Promise<PrivacySettings> {
  requireDesktop();
  return invoke<PrivacySettings>("get_privacy_settings");
}

export async function savePrivacySettings(settings: PrivacySettings): Promise<PrivacySettings> {
  requireDesktop();
  return invoke<PrivacySettings>("save_privacy_settings", { settings });
}

export async function previewPrivacy(text: string, settings: PrivacySettings): Promise<PrivacyInspection> {
  requireDesktop();
  if (!text.trim()) throw new Error(t("请输入需要检查的文本或 JSON。"));
  if (new TextEncoder().encode(text).length > maxPrivacyPreviewBytes) {
    throw new Error(t("本地预览最多检查 256 KiB 文本。"));
  }
  return invoke<PrivacyInspection>("preview_privacy", { text, settings });
}

export function privacyBlockMessage(code: string): string {
  const messages: Record<string, string> = {
    privacy_policy_unavailable: "无法读取隐私策略，发送已阻止。",
    privacy_sensitive_structure: "可执行字段或结构字段含有敏感信息，发送已阻止。",
    privacy_attachment_uninspected: "附件内容无法检查，发送已阻止。",
    privacy_scan_limit: "内容超过隐私检查限制，发送已阻止。",
    privacy_scan_failed: "隐私检查失败，发送已阻止。",
    privacy_response_sensitive_structure: "响应的工具参数或结构字段含有敏感信息，已停止返回。",
    privacy_response_attachment_uninspected: "响应附件内容无法检查，已停止返回。",
    privacy_response_scan_limit: "响应超过隐私检查限制，已停止返回。",
    privacy_response_scan_failed: "响应隐私检查失败，已停止返回。",
  };
  return t(messages[code] ?? (code.startsWith("privacy_response_") ? "响应隐私检查失败，已停止返回。" : "隐私检查失败，发送已阻止。"));
}
