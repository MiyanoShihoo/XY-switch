import type { PluginClient, PluginKind, PluginPackage, PluginPermissions, PublishedPlugin } from "./plugins";

export type SubmissionStatus = "submitted" | "changes_requested" | "rejected" | "approved" | "published" | "withdrawn";
export const submissionStatuses: readonly SubmissionStatus[] = ["submitted", "changes_requested", "rejected", "approved", "published", "withdrawn"];
export type ReviewDecision = "approve" | "request_changes" | "reject";
export type WithdrawalReason = "security" | "privacy" | "withdrawn";

export interface MarketplaceUser {
  userId: string;
  username: string;
  displayName: string;
  reviewer: boolean;
}

export interface MarketplaceSession {
  expiresAt: number;
  user: MarketplaceUser;
}

export interface MarketplacePageResult<Item> {
  items: Item[];
  limit: number;
  offset: number;
  hasMore: boolean;
}

export interface MarketplaceListQuery {
  limit?: number;
  offset?: number;
  status?: SubmissionStatus;
}

export interface MarketplaceCatalogQuery {
  limit?: number;
  offset?: number;
  client?: PluginClient;
  kind?: PluginKind;
  q?: string;
}

export interface MarketplaceCatalogEntry {
  id: string;
  name: string;
  description: string;
  authorDisplayName: string;
  version: string;
  kind: PluginKind;
  clients: PluginClient[];
  digest: string;
  signature: string;
  signingKeyId: string;
  publishedAt: number;
}

export interface MarketplaceSubmissionSummary {
  submissionId: string;
  pluginId: string;
  name: string;
  version: string;
  digest: string;
  status: SubmissionStatus;
  authorDisplayName: string;
  createdAt: number;
  updatedAt: number;
  reviewNote: string | null;
  previousPublishedVersion: string | null;
  permissionChanges: PluginPermissions;
  permissions: PluginPermissions;
  clients: PluginClient[];
}

export interface MarketplaceSubmission {
  submissionId: string;
  status: SubmissionStatus;
  package: PluginPackage;
  digest: string;
  authorDisplayName: string;
  createdAt: number;
  updatedAt: number;
  reviewNote: string | null;
  previousPublishedVersion: string | null;
  permissionChanges: PluginPermissions;
}

export interface MarketplaceVersionRequest {
  version: string;
  digest: string;
}

export interface MarketplaceDecisionRequest extends MarketplaceVersionRequest {
  decision: ReviewDecision;
  note?: string;
}

export interface MarketplaceWithdrawalRequest extends MarketplaceVersionRequest {
  reason: WithdrawalReason;
  note?: string;
}

export interface MarketplaceInstallPlan {
  origin: string;
  published: PublishedPlugin;
  preview: import("./plugins").InstallPreview;
}

export interface MarketplaceReport {
  reportId: string;
  pluginId: string;
  version: string;
  reporterDisplayName: string;
  reason: "security" | "privacy" | "malware" | "copyright" | "other";
  note: string;
  status: "open" | "resolved" | "dismissed";
  resolutionNote: string | null;
  createdAt: number;
  resolvedAt: number | null;
}

export interface MarketplaceAuditEvent {
  sequence: number;
  actorDisplayName: string;
  submissionId: string | null;
  version: string | null;
  digest: string | null;
  action: string;
  fromStatus: string | null;
  toStatus: string | null;
  note: string | null;
  createdAt: number;
}

export interface MarketplaceClientOptions {
  allowDevelopmentHttp?: boolean;
  fetch?: typeof globalThis.fetch;
  now?: () => number;
  requestTimeoutMs?: number;
}

export interface MarketplaceInstallOptions {
  clients: PluginClient[];
  confirmed: boolean;
  enable?: boolean;
}
