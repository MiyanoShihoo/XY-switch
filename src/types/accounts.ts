export type AccountKind = "apiKey" | "oauth" | "nativeManaged";
export type AccountClient = "codexDesktop" | "codexCli" | "claudeDesktop" | "claudeCode" | "openCodeDesktop" | "openCodeCli" | "geminiCli";
export type AccountStatus = "available" | "refreshing" | "refreshRequired" | "nativeCheckRequired" | "loginRequired" | "quotaLimited" | "coolingDown" | "disabled" | "revoked" | "destinationApprovalRequired";
export type AuthorizationStatus = "unverified" | "authorized" | "loginRequired";
export type AccountSource = "manual" | "selectedFile" | "nativeClient";
export type QuotaSource = "unknown" | "providerResponse" | "providerAccountApi" | "nativeClient";
export type QuotaUnit = "requests" | "tokens" | "percent";

export interface QuotaWindow {
  id: string;
  unit: QuotaUnit;
  remaining: number | null;
  limit: number | null;
  resetsAt: number | null;
}

export interface QuotaSnapshot {
  source: QuotaSource;
  observedAt: number;
  validUntil: number;
  windows: QuotaWindow[];
}

export interface AccountInfo {
  id: string;
  alias: string;
  providerId: string;
  kind: AccountKind;
  client: AccountClient | null;
  source: AccountSource;
  status: AccountStatus;
  authorizationStatus: AuthorizationStatus;
  destinationApproved: boolean;
  enabled: boolean;
  revision: number;
  credentialGeneration: number;
  expiresAt: number | null;
  cooldownUntil: number | null;
  availableModels: string[];
  quota: QuotaSnapshot | null;
  allowRefresh: boolean;
  canRefresh: boolean;
  cleanupPending: boolean;
  lastValidatedAt: number | null;
  lastUsedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface AccountPolicy {
  mode: "fixed" | "rotate";
  fixedAccountId: string | null;
  affinity: boolean;
  failureCooldownSeconds: number;
  rateLimitCooldownSeconds: number;
  maxCooldownSeconds: number;
}

export type AccountCredentialInput =
  | { kind: "apiKey"; apiKey: string }
  | { kind: "oauth"; accessToken: string; refreshToken: string | null; expiresAt: number | null }
  | { kind: "nativeManaged"; client: AccountClient };

export interface AddAccountRequest {
  alias: string;
  providerId: string;
  approvedDestination: string;
  credentials: AccountCredentialInput;
  availableModels: string[];
  allowRefresh: boolean;
  confirmed: boolean;
}

export interface UpdateAccountRequest {
  id: string;
  expectedRevision: number;
  alias: string;
  providerId: string;
  approvedDestination?: string | null;
  enabled: boolean;
  availableModels: string[];
  confirmed: boolean;
}

export interface ReplaceCredentialRequest {
  id: string;
  expectedRevision: number;
  approvedDestination?: string | null;
  credentials: AccountCredentialInput;
  allowRefresh: boolean;
  confirmed: boolean;
}

export interface SelectedAccountImport { sourceId: string; expiresAt: number }
export interface ImportAccountRequest {
  sourceId: string;
  alias: string;
  providerId: string;
  approvedDestination: string;
  allowRefresh: boolean;
  confirmed: boolean;
}

export interface LocalRevocation {
  account: AccountInfo;
  localReferenceRevoked: boolean;
  credentialsRemoved: boolean;
  scope: "localOnly";
}

export interface AccountProviderOption { id: string; name: string; endpoint?: string }
