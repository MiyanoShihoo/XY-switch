import { addAccount, updateAccount, replaceAccountCredentials, chooseAccountImport, importAccount, AccountServiceError, accountErrorText } from "../../lib/accounts-service";
import { loadProviders } from "../../lib/provider-service";
import type { AccountInfo, SelectedAccountImport, UpdateAccountRequest } from "../../types/accounts";
import { accountDestinationSnapshot } from "./account-presentation";

export const ACCOUNT_QUERY_KEYS = {
  list: ["accounts", "list"] as const,
  providers: ["accounts", "providers"] as const,
  policy: (providerId: string) => ["accounts", "policy", providerId] as const,
};

export interface AccountProviderSummary { id: string; name: string; enabled: boolean; endpoint: string | null }
export interface CredentialField { value: string }
export interface ImportSelectionRef { current: SelectedAccountImport | null }
export type ApiKeyForm =
  | { mode: "add"; alias: string; providerId: string; approvedDestination: string }
  | { mode: "replace"; id: string; expectedRevision: number; approvedDestination: string };

type ApiKeyCredential = { kind: "apiKey"; apiKey: string };
const credentialRequests = new WeakMap<CredentialField, Set<ApiKeyCredential>>();

export function clearCredentialField(field: CredentialField | null): void {
  if (!field) return;
  field.value = "";
  for (const credentials of credentialRequests.get(field) ?? []) credentials.apiKey = "";
}

export function requireAccountAction(confirmed: boolean, isActive: () => boolean): void {
  if (confirmed !== true) throw new AccountServiceError("account_authorization_required");
  if (!isActive()) throw new AccountServiceError("account_cancelled");
}

export async function loadAccountProviders(): Promise<AccountProviderSummary[]> {
  try {
    return (await loadProviders()).map(provider => ({ id: provider.id, name: provider.name, enabled: provider.enabled,
      endpoint: accountDestinationSnapshot(provider.endpoint) }));
  } catch (error) {
    throw new Error(accountErrorText(error));
  }
}

export function captureAccountDestination(provider: AccountProviderSummary | undefined): string {
  const destination = accountDestinationSnapshot(provider?.endpoint);
  if (!destination) throw new AccountServiceError("account_destination_authorization_required");
  return destination;
}

export async function submitAccountSettings(form: Omit<UpdateAccountRequest, "confirmed" | "approvedDestination"> & { approvedDestination: string }, confirmed: boolean, isActive: () => boolean): Promise<AccountInfo> {
  requireAccountAction(confirmed, isActive);
  if (!accountDestinationSnapshot(form.approvedDestination)) throw new AccountServiceError("account_destination_authorization_required");
  return updateAccount({ ...form, confirmed: true });
}

export async function submitApiKey(form: ApiKeyForm, field: CredentialField | null, confirmed: boolean, isActive: () => boolean): Promise<AccountInfo> {
  let credentials: ApiKeyCredential | undefined;
  try {
    requireAccountAction(confirmed, isActive);
    if (!accountDestinationSnapshot(form.approvedDestination)) throw new AccountServiceError("account_destination_authorization_required");
    if (!field) throw new AccountServiceError("account_invalid_input");
    credentials = { kind: "apiKey", apiKey: field.value };
    if (!credentials.apiKey) throw new AccountServiceError("account_invalid_input");
    const requests = credentialRequests.get(field) ?? new Set<ApiKeyCredential>();
    requests.add(credentials);
    credentialRequests.set(field, requests);
    if (form.mode === "add") {
      return await addAccount({ alias: form.alias, providerId: form.providerId, approvedDestination: form.approvedDestination, credentials, availableModels: [], allowRefresh: false, confirmed: true });
    }
    return await replaceAccountCredentials({ id: form.id, expectedRevision: form.expectedRevision, approvedDestination: form.approvedDestination, credentials, allowRefresh: false, confirmed: true });
  } catch (error) {
    if (error instanceof AccountServiceError) throw error;
    throw new Error(accountErrorText(error));
  } finally {
    clearCredentialField(field);
    if (field && credentials) {
      const requests = credentialRequests.get(field);
      requests?.delete(credentials);
      if (!requests?.size) credentialRequests.delete(field);
    }
    if (credentials) credentials.apiKey = "";
  }
}

export async function selectAccountImport(confirmed: boolean, isActive: () => boolean): Promise<SelectedAccountImport | null> {
  requireAccountAction(confirmed, isActive);
  const selected = await chooseAccountImport();
  requireAccountAction(confirmed, isActive);
  return selected;
}

export async function submitSelectedImport(form: { alias: string; providerId: string; approvedDestination: string; allowRefresh: boolean }, selection: ImportSelectionRef, confirmed: boolean, isActive: () => boolean): Promise<AccountInfo> {
  requireAccountAction(confirmed, isActive);
  if (!accountDestinationSnapshot(form.approvedDestination)) throw new AccountServiceError("account_destination_authorization_required");
  const selected = selection.current;
  if (!selected || !Number.isSafeInteger(selected.expiresAt) || selected.expiresAt <= Math.floor(Date.now() / 1000)) {
    selection.current = null;
    throw new AccountServiceError("account_import_selection_expired");
  }
  selection.current = null;
  return importAccount({ sourceId: selected.sourceId, alias: form.alias, providerId: form.providerId, approvedDestination: form.approvedDestination, allowRefresh: form.allowRefresh, confirmed: true });
}
