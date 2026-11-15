import type { AccountInfo, AccountPolicy } from "../../types/accounts";

export function accountPolicyCandidates(accounts: AccountInfo[], providerId: string): AccountInfo[] {
  return providerId ? accounts.filter(account => account.providerId === providerId && account.kind !== "nativeManaged" && account.enabled && account.status !== "revoked") : [];
}

export function accountParticipation(account: AccountInfo, providerId: string, policy?: AccountPolicy): string {
  if (!providerId || account.providerId !== providerId || account.kind === "nativeManaged" || !account.enabled || account.status === "revoked") return "不参与选择";
  if (!policy) return "策略尚未读取";
  if (policy.mode === "rotate") return "轮换候选";
  return policy.fixedAccountId === account.id ? "固定选择" : "未被固定选择";
}
