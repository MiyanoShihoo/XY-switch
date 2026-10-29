import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type { AccessPolicy, AccessTokenInfo, BudgetPolicy, BudgetStatus, ConfigVersion, GovernanceEvent, GovernanceOverview, IssuedAccessToken, MonitorPolicy, MountBackup, MountClient, MountPreview, MountRoot, SyntheticRun, VersionPreview } from "../types/governance";

export const defaultAccessPolicy: AccessPolicy = { requireToken: false, requestsPerMinute: 120, maxInFlight: 8 };
export const defaultBudgetPolicy: BudgetPolicy = { enabled: false, dailyTokens: 100000, dailyMicrousd: 1000000, inputReservation: 4096, maxOutputTokens: 512 };
export const defaultMonitorPolicy: MonitorPolicy = { enabled: false, providerId: "", model: "", mode: "chat", intervalMinutes: 30, maxRunsPerDay: 10 };
export const governanceNative = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
async function read<T>(command: string, fallback: T): Promise<T> {
  return governanceNative() ? invoke<T>(command) : structuredClone(fallback);
}
async function write<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (!governanceNative()) throw new Error(t("浏览器预览不能保存治理设置、发起收费验收或写入外部文件。"));
  return invoke<T>(command, args);
}
export const loadAccessPolicy = () => read<AccessPolicy>("get_access_policy", defaultAccessPolicy);
export const saveAccessPolicy = (policy: AccessPolicy, confirmed: boolean) => write<void>("save_access_policy", { policy, confirmed });
export const listAccessTokens = () => read<AccessTokenInfo[]>("list_access_tokens", []);
// Deliberately not a React Query mutation: the one-time secret must never enter a query/mutation cache.
export const issueAccessToken = (label: string, scope: "proxy" | "monitor", days: number) => write<IssuedAccessToken>("create_access_token", { label, scope, days });
export const revokeAccessToken = (id: string, confirmed: boolean) => write<void>("revoke_access_token", { id, confirmed });
export const loadHardBudget = () => read<BudgetStatus>("get_hard_budget", { policy: defaultBudgetPolicy, dayUtc: Math.floor(Date.now() / 86400000), reservedTokens: 0, reservedMicrousd: 0, attempts: 0 });
export const saveHardBudget = (policy: BudgetPolicy, confirmed: boolean) => write<void>("save_hard_budget", { policy, confirmed });
export const loadMonitorPolicy = () => read<MonitorPolicy>("get_synthetic_monitor", defaultMonitorPolicy);
export const saveMonitorPolicy = (policy: MonitorPolicy, confirmation: string) => write<void>("save_synthetic_monitor", { policy, confirmation });
export const runSyntheticCheck = (policy: MonitorPolicy, confirmation: string) => write<SyntheticRun>("run_synthetic_check", { policy, confirmation });
export const listSyntheticRuns = () => read<SyntheticRun[]>("list_synthetic_runs", []);
export const loadGovernanceOverview = () => read<GovernanceOverview | null>("get_governance_status", null);
export const loadGovernanceEvents = () => read<GovernanceEvent[]>("governance_events", []);
export const createConfigVersion = (label: string) => write<ConfigVersion>("create_config_version", { label });
export const listConfigVersions = () => read<ConfigVersion[]>("list_config_versions", []);
export const previewConfigRollback = (id: string) => write<VersionPreview>("preview_config_rollback", { id });
export const applyConfigRollback = (preview: VersionPreview, confirmation: string) => write<ConfigVersion>("apply_config_rollback", { id: preview.versionId, expectedCurrentHash: preview.expectedCurrentHash, targetHash: preview.targetHash, confirmation });
export const deleteConfigVersion = (id: string, confirmation: string) => write<void>("delete_config_version", { id, confirmation });
export const chooseMountRoot = () => write<MountRoot | null>("choose_mount_root", {});
export const previewExplicitMount = (rootId: string, client: MountClient, promptIds: string[], skillIds: string[]) => write<MountPreview>("preview_explicit_mount", { rootId, client, promptIds, skillIds });
export const previewMountRollback = (rootId: string, backupId: string) => write<MountPreview>("preview_mount_rollback", { rootId, backupId });
export const applyExplicitMount = (id: string, confirmation: string) => write<MountBackup>("apply_explicit_mount", { id, confirmation });
export const listMountBackups = () => read<MountBackup[]>("list_mount_backups", []);
export const deleteMountBackup = (id: string, confirmation: string) => write<void>("delete_mount_backup", { id, confirmation });
