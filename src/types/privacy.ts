export interface PrivacySettings {
  redactPersonalInformation: boolean;
  blockUninspectedAttachments: boolean;
}

export interface PrivacyReport {
  secretRedactions: number;
  personalRedactions: number;
  protectedFields: number;
  uninspectedAttachments: number;
}

export type PrivacyBlockCode = "privacy_policy_unavailable" | "privacy_sensitive_structure" | "privacy_attachment_uninspected" | "privacy_scan_limit" | "privacy_scan_failed"
  | "privacy_response_sensitive_structure" | "privacy_response_attachment_uninspected" | "privacy_response_scan_limit" | "privacy_response_scan_failed";

export interface PrivacyInspection {
  redacted: unknown;
  report: PrivacyReport;
  blockedCode: PrivacyBlockCode | null;
}
