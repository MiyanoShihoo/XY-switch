import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AccountsPage, AccountPolicyFields, accountPolicyValid } from "./features/accounts/AccountsPage";
import { ACCOUNT_QUERY_KEYS, captureAccountDestination, clearCredentialField, loadAccountProviders, selectAccountImport, submitAccountSettings, submitApiKey, submitSelectedImport } from "./features/accounts/account-actions";
import { AccountQuotaView, accountCanRefresh, accountDestinationLabel, accountDestinationSnapshot, accountStatusLabel, knownQuotaWindows, localRevocationMessage, safeAccountLabel, validAccountAlias } from "./features/accounts/account-presentation";
import { AccountServiceError, DEFAULT_ACCOUNT_POLICY, accountErrorText, addAccount, chooseAccountImport, getAccountPolicy, importAccount, listAccounts, refreshAccount, replaceAccountCredentials, revokeAccount, saveAccountPolicy, updateAccount } from "./lib/accounts-service";
import type { AccountInfo, AccountPolicy, QuotaSnapshot, SelectedAccountImport } from "./types/accounts";
import type { ClientConnection } from "./types/clients";
import { CLIENT_CONNECTIONS_QUERY_KEY, nativeAccountCanSync, nativeAccountSyncCopy, submitNativeAccountSync } from "./features/clients/native-account-actions";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const queryClients: QueryClient[] = [];
const providerId = "synthetic-provider";
const approvedDestination = "https://provider.example.test/v1";

function account(overrides: Partial<AccountInfo> = {}): AccountInfo {
  const timestamp = Math.floor(Date.now() / 1000);
  return {
    id: "synthetic-account", alias: "工作账号", providerId, kind: "apiKey", client: null, source: "manual",
    status: "available", authorizationStatus: "unverified", destinationApproved: true, enabled: true, revision: 4, credentialGeneration: 2,
    expiresAt: null, cooldownUntil: null, availableModels: [], quota: null, allowRefresh: false, canRefresh: false,
    cleanupPending: false, lastValidatedAt: null, lastUsedAt: null, createdAt: timestamp, updatedAt: timestamp,
    ...overrides,
  };
}

function evidence(timestamp: number, remaining = 0): QuotaSnapshot {
  return { source: "providerResponse", observedAt: timestamp, validUntil: timestamp + 300,
    windows: [{ id: "private-window-id", unit: "requests", remaining, limit: 100, resetsAt: timestamp + 200 }] };
}

function accountClient(accounts: AccountInfo[] = [], connections?: ClientConnection[]): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity, gcTime: Infinity } } });
  queryClients.push(client);
  client.setQueryData(ACCOUNT_QUERY_KEYS.list, accounts);
  client.setQueryData(ACCOUNT_QUERY_KEYS.providers, [{ id: providerId, name: "测试供应商", enabled: true, endpoint: approvedDestination }]);
  client.setQueryData(ACCOUNT_QUERY_KEYS.policy(providerId), { ...DEFAULT_ACCOUNT_POLICY });
  if (connections) client.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, connections);
  return client;
}

function renderPage(accounts: AccountInfo[] = [], connections?: ClientConnection[]): string {
  const client = accountClient(accounts, connections);
  return renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(AccountsPage)));
}

function nativeConnection(overrides: Partial<ClientConnection> = {}): ClientConnection {
  return { id: "codex-cli", label: "Codex CLI", configOwner: "codex", configPaths: [], configDetected: true,
    takeoverTarget: "codex", mcpTarget: "codex", loginMethod: "codex_device_code", loginInstructions: "",
    inferenceMode: "responses_gateway", nativeVerification: "pending", setupError: null, configurationPending: false,
    loginStatus: null, account: null, ...overrides };
}

function nativeAccount(overrides: Partial<AccountInfo> = {}): AccountInfo {
  return account({ kind: "nativeManaged", client: "codexCli", source: "nativeClient", status: "nativeCheckRequired", ...overrides });
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
});
afterEach(() => {
  for (const client of queryClients.splice(0)) client.clear();
  vi.unstubAllGlobals();
});

describe("account native services and explicit authorization", () => {
  it("blocks native operations in browser preview and renders disabled controls without fake success", async () => {
    vi.stubGlobal("window", {});
    await expect(listAccounts()).rejects.toThrow("桌面应用");
    await expect(chooseAccountImport()).rejects.toThrow("桌面应用");
    await expect(getAccountPolicy(providerId)).rejects.toThrow("桌面应用");
    await expect(refreshAccount("synthetic-account", true)).rejects.toThrow("桌面应用");
    const html = renderPage();
    expect(html).toContain("账号池操作需要桌面应用");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>添加 API key<\/button>/);
    expect(html).not.toContain("账号已保存");
    expect(html).not.toContain("凭据刷新已完成");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects every unconfirmed sensitive write before IPC", async () => {
    const current = account();
    const credentials = { kind: "apiKey" as const, apiKey: "synthetic-key-only" };
    const operations = [
      () => addAccount({ alias: current.alias, providerId, approvedDestination, credentials, availableModels: [], allowRefresh: false, confirmed: false }),
      () => updateAccount({ id: current.id, expectedRevision: current.revision, alias: current.alias, providerId, enabled: true, availableModels: [], confirmed: false }),
      () => replaceAccountCredentials({ id: current.id, expectedRevision: current.revision, credentials, allowRefresh: false, confirmed: false }),
      () => importAccount({ sourceId: "synthetic-selection", alias: current.alias, providerId, approvedDestination, allowRefresh: false, confirmed: false }),
      () => saveAccountPolicy(providerId, { ...DEFAULT_ACCOUNT_POLICY }, false),
      () => refreshAccount(current.id, false),
      () => revokeAccount(current.id, false),
    ];
    for (const operation of operations) await expect(operation()).rejects.toMatchObject({ code: "account_authorization_required" });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("uses the registered metadata, revision, policy, refresh and local-revocation commands", async () => {
    const current = account();
    const policy: AccountPolicy = { ...DEFAULT_ACCOUNT_POLICY, mode: "fixed", fixedAccountId: current.id, affinity: false };
    const receipt = { account: account({ status: "revoked" }), localReferenceRevoked: true, credentialsRemoved: true, scope: "localOnly" };
    invoke.mockImplementation(async (command: string) => {
      if (command === "list_accounts") return [current];
      if (command === "get_account_policy" || command === "save_account_policy") return policy;
      if (command === "revoke_account") return receipt;
      return current;
    });
    expect(await listAccounts()).toEqual([current]);
    expect(invoke).toHaveBeenLastCalledWith("list_accounts", {});
    expect(await getAccountPolicy(providerId)).toEqual(policy);
    expect(invoke).toHaveBeenLastCalledWith("get_account_policy", { providerId });
    const update = { id: current.id, expectedRevision: 4, alias: "备用账号", providerId, approvedDestination, enabled: false, availableModels: [], confirmed: true };
    expect(await updateAccount(update)).toEqual(current);
    expect(invoke).toHaveBeenLastCalledWith("update_account", { request: update });
    expect(await saveAccountPolicy(providerId, policy, true)).toEqual(policy);
    expect(invoke).toHaveBeenLastCalledWith("save_account_policy", { providerId, policy, confirmed: true });
    expect(await refreshAccount(current.id, true)).toEqual(current);
    expect(invoke).toHaveBeenLastCalledWith("refresh_account", { id: current.id, confirmed: true });
    expect(await revokeAccount(current.id, true)).toEqual(receipt);
    expect(invoke).toHaveBeenLastCalledWith("revoke_account", { id: current.id, confirmed: true });
  });

  it("propagates missing refresh capability and sanitizes arbitrary bridge errors", async () => {
    invoke.mockRejectedValueOnce("account_refresh_not_configured");
    await expect(refreshAccount("synthetic-account", true)).rejects.toMatchObject({ code: "account_refresh_not_configured" });
    const sensitive = "person@example.test Bearer synthetic-secret C:\synthetic\auth.json";
    invoke.mockRejectedValueOnce(new Error(sensitive));
    let caught: unknown;
    try { await listAccounts(); } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(AccountServiceError);
    expect(accountErrorText(caught)).toContain("账号池操作失败");
    expect(String(caught)).not.toContain(sensitive);
    expect(accountErrorText(new Error(sensitive))).not.toContain("synthetic-secret");
    expect(accountErrorText("account_login_required")).toContain("重新授权");
  });

  it("retains only provider summaries instead of copying provider credentials into the account query", async () => {
    invoke.mockResolvedValue([{ id: providerId, name: "测试供应商", enabled: true, providerType: "custom", appGroup: "Codex",
      endpoint: "https://provider.example.test/v1", model: "synthetic-model", apiKey: "synthetic-provider-private", advancedConfig: {} }]);
    const summaries = await loadAccountProviders();
    expect(summaries).toEqual([{ id: providerId, name: "测试供应商", enabled: true, endpoint: approvedDestination }]);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("list_providers");
    expect(JSON.stringify(summaries)).not.toContain("synthetic-provider-private");
    expect(Object.keys(summaries[0]).sort()).toEqual(["enabled", "endpoint", "id", "name"]);
  });
});

describe("memory-only API key forms", () => {
  it("sends the explicitly authorized key once and clears both input and request memory on completion", async () => {
    const field = { value: "synthetic-form-key" }; let dispatched: unknown;
    const storage = { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal("localStorage", storage);
    invoke.mockImplementation(async (_command: string, args: unknown) => { dispatched = structuredClone(args); return account(); });
    const saved = await submitApiKey({ mode: "add", alias: "工作账号", providerId, approvedDestination }, field, true, () => true);
    expect(saved.id).toBe("synthetic-account");
    expect(invoke.mock.calls[0][0]).toBe("add_account");
    expect(dispatched).toEqual({ request: { alias: "工作账号", providerId, approvedDestination, credentials: { kind: "apiKey", apiKey: "synthetic-form-key" }, availableModels: [], allowRefresh: false, confirmed: true } });
    expect(field.value).toBe("");
    expect(invoke.mock.calls[0][1].request.credentials.apiKey).toBe("");
    expect(storage.getItem).not.toHaveBeenCalled(); expect(storage.setItem).not.toHaveBeenCalled(); expect(storage.removeItem).not.toHaveBeenCalled();
  });

  it("replaces a key with the expected account revision and clears memory after a rejected native write", async () => {
    const field = { value: "synthetic-replacement-key" }; let dispatched: unknown;
    invoke.mockImplementation(async (_command: string, args: unknown) => { dispatched = structuredClone(args); throw "account_revision_conflict"; });
    await expect(submitApiKey({ mode: "replace", id: "synthetic-account", expectedRevision: 4, approvedDestination }, field, true, () => true)).rejects.toMatchObject({ code: "account_revision_conflict" });
    expect(invoke.mock.calls[0][0]).toBe("replace_account_credentials");
    expect(dispatched).toEqual({ request: { id: "synthetic-account", expectedRevision: 4, approvedDestination, credentials: { kind: "apiKey", apiKey: "synthetic-replacement-key" }, allowRefresh: false, confirmed: true } });
    expect(field.value).toBe(""); expect(invoke.mock.calls[0][1].request.credentials.apiKey).toBe("");
  });

  it("blocks unconfirmed, departed and empty forms, and always removes the input value", async () => {
    for (const [confirmed, active, expected] of [[false, true, "account_authorization_required"], [true, false, "account_cancelled"], [true, true, "account_invalid_input"]] as const) {
      const field = { value: expected === "account_invalid_input" ? "" : "synthetic-unsubmitted-key" };
      await expect(submitApiKey({ mode: "add", alias: "工作账号", providerId, approvedDestination }, field, confirmed, () => active)).rejects.toMatchObject({ code: expected });
      expect(field.value).toBe("");
    }
    expect(invoke).not.toHaveBeenCalled();
    const field = { value: "synthetic-leaving-key" }; clearCredentialField(field); clearCredentialField(null);
    expect(field.value).toBe("");
  });

  it("clears the pending request object immediately when its form leaves the page", async () => {
    const field = { value: "synthetic-pending-form-key" };
    let finish: (saved: AccountInfo) => void = () => {};
    invoke.mockImplementation(() => new Promise<AccountInfo>(resolve => { finish = resolve; }));
    const saving = submitApiKey({ mode: "add", alias: "工作账号", providerId, approvedDestination }, field, true, () => true);
    expect(invoke.mock.calls[0][1].request.credentials.apiKey).toBe("synthetic-pending-form-key");
    clearCredentialField(field);
    expect(field.value).toBe("");
    expect(invoke.mock.calls[0][1].request.credentials.apiKey).toBe("");
    finish(account());
    await saving;
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(field.value).toBe("");
  });
});

describe("approved provider destinations", () => {
  it("keeps the approved destination snapshot when the provider changes before submission", async () => {
    const provider = { id: providerId, name: "测试供应商", enabled: true, endpoint: approvedDestination };
    const form = { mode: "add" as const, alias: "工作账号", providerId, approvedDestination: captureAccountDestination(provider) };
    provider.endpoint = "https://changed.example.test/v2";
    const field = { value: "synthetic-destination-key" };
    let dispatched: unknown;
    invoke.mockImplementation(async (_command: string, args: unknown) => {
      dispatched = structuredClone(args);
      throw "account_destination_authorization_required";
    });
    await expect(submitApiKey(form, field, true, () => true)).rejects.toMatchObject({ code: "account_destination_authorization_required" });
    expect(dispatched).toMatchObject({ request: { providerId, approvedDestination, credentials: { apiKey: "synthetic-destination-key" } } });
    expect(JSON.stringify(dispatched)).not.toContain(provider.endpoint);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(field.value).toBe("");
    expect(invoke.mock.calls[0][1].request.credentials.apiKey).toBe("");
  });

  it("rejects sensitive or ambiguous destinations before IPC and removes the API key input", async () => {
    const unsafe = [
      "https://user:password@provider.example.test/v1", "https://provider.example.test/person@example.test",
      "https://provider.example.test/person%40example.test", "https://provider.example.test/sk-synthetic-secret-12345678",
      "https://provider.example.test/v1?api_key=synthetic-private", "https://provider.example.test/v1#synthetic-private",
      "https://provider.example.test/v1\n", "https://provider.example.test/v1%0A", "https://provider.example.test/v1\u0085",
      "https://provider.example.test/v1%", "file:///synthetic/auth.json", " https://provider.example.test/v1",
    ];
    for (const destination of unsafe) {
      expect(accountDestinationSnapshot(destination)).toBeNull();
      expect(accountDestinationLabel(destination)).toBe("请求地址不可用于账号授权");
      expect(() => captureAccountDestination({ id: providerId, name: "测试供应商", enabled: true, endpoint: destination })).toThrow(AccountServiceError);
      const field = { value: "synthetic-unsubmitted-key" };
      await expect(submitApiKey({ mode: "add", alias: "工作账号", providerId, approvedDestination: destination }, field, true, () => true)).rejects.toMatchObject({ code: "account_destination_authorization_required" });
      expect(field.value).toBe("");
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not retain sensitive endpoint values in provider query summaries", async () => {
    const endpoints = ["https://provider.example.test/v1?token=synthetic-hidden", "https://person@example.test/v1", "https://provider.example.test/sk-synthetic-hidden-12345678"];
    invoke.mockResolvedValue(endpoints.map((endpoint, index) => ({ id: `${providerId}-${index}`, name: "测试供应商", enabled: true, endpoint, apiKey: "synthetic-provider-private" })));
    const summaries = await loadAccountProviders();
    expect(summaries).toHaveLength(3);
    expect(summaries.every(provider => provider.endpoint === null)).toBe(true);
    for (const sensitive of [...endpoints, "synthetic-provider-private", "synthetic-hidden"]) expect(JSON.stringify(summaries)).not.toContain(sensitive);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("list_providers");
  });

  it("shows destination reapproval and submits only the explicitly confirmed settings revision", async () => {
    const current = account({ kind: "oauth", destinationApproved: false, status: "destinationApprovalRequired", canRefresh: true, allowRefresh: true });
    const html = renderPage([current]);
    expect(html).toContain("请求地址待批准");
    expect(html).toContain("重新批准请求地址");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>刷新凭据<\/button>/);
    expect(accountCanRefresh(current, Math.floor(Date.now() / 1000))).toBe(false);
    const form = { id: current.id, expectedRevision: current.revision, alias: current.alias, providerId,
      approvedDestination: "https://changed.example.test/v2", enabled: true, availableModels: [] };
    await expect(submitAccountSettings(form, false, () => true)).rejects.toMatchObject({ code: "account_authorization_required" });
    await expect(submitAccountSettings(form, true, () => false)).rejects.toMatchObject({ code: "account_cancelled" });
    await expect(submitAccountSettings({ ...form, approvedDestination: "https://user@example.test/v1" }, true, () => true)).rejects.toMatchObject({ code: "account_destination_authorization_required" });
    expect(invoke).not.toHaveBeenCalled();
    invoke.mockResolvedValue(account({ kind: "oauth", revision: 5, credentialGeneration: 3, destinationApproved: true }));
    expect((await submitAccountSettings(form, true, () => true)).credentialGeneration).toBe(3);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("update_account", { request: { ...form, confirmed: true } });
  });
});

describe("native-managed account synchronization", () => {
  it("presents synchronized state without claiming HTTP rotation or subscription inference", () => {
    const codex = nativeAccount({ status: "available" });
    const opencode = nativeAccount({ id: "synthetic-opencode", client: "openCodeCli" });
    const html = renderPage([codex, opencode], [nativeConnection(), nativeConnection({ id: "opencode-cli", configOwner: "opencode" })]);
    expect(accountStatusLabel("available", "nativeManaged")).toBe("原生状态已同步");
    expect(accountStatusLabel("available", "apiKey")).toBe("可参与选择");
    expect(html).toContain("原生状态需要重新同步");
    expect(html).toContain("原生状态已同步");
    expect(html).toContain("登录仍由原生客户端管理");
    expect(html).toContain("尚不参与网关的 HTTP 账号轮换");
    expect(html).not.toContain('data-status="available">可参与选择');
    expect(html).toMatch(/<button(?![^>]*disabled)[^>]*>通过 Codex 刷新<\/button>/);
    expect(html).toMatch(/<button(?![^>]*disabled)[^>]*>同步 OpenCode 状态<\/button>/);
    expect(html).not.toContain("刷新凭据</button>");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("uses client-specific confirmation and does not require OAuth refresh capability", () => {
    for (const client of ["codexDesktop", "codexCli", "openCodeDesktop", "openCodeCli"] as const) {
      const current = nativeAccount({ client, allowRefresh: false, canRefresh: false });
      const codex = client === "codexDesktop" || client === "codexCli";
      const desktop = client === "codexDesktop" || client === "openCodeDesktop";
      const id = codex ? desktop ? "codex-desktop" : "codex-cli" : desktop ? "opencode-desktop" : "opencode-cli";
      expect(nativeAccountCanSync(current, [nativeConnection({ id })])).toBe(true);
      expect(accountCanRefresh(current, Math.floor(Date.now() / 1000))).toBe(false);
      const copy = nativeAccountSyncCopy(current);
      expect(copy.confirmLabel).toBe(codex ? "授权原生刷新" : "授权同步元数据");
      expect(copy.message).toContain(codex ? "Codex 原生客户端刷新其管理的凭据" : "此操作不会强制刷新 token");
      expect(copy.message).toContain("尚不参与网关的 HTTP 账号轮换");
    }
  });

  it("blocks shared-owner pending login in the account page and rechecks it before IPC", async () => {
    for (const client of ["codexCli", "openCodeCli"] as const) {
      const current = nativeAccount({ client });
      const codex = client === "codexCli";
      const connections = [nativeConnection({ id: codex ? "codex-cli" : "opencode-cli" }),
        nativeConnection({ id: codex ? "codex-desktop" : "opencode-desktop", loginStatus: "pending" })];
      const cache = accountClient([current], connections);
      const html = renderToStaticMarkup(createElement(QueryClientProvider, { client: cache }, createElement(AccountsPage)));
      expect(html).toContain("原生登录进行中");
      expect(html).toMatch(codex ? /<button[^>]*disabled=""[^>]*>通过 Codex 刷新<\/button>/ : /<button[^>]*disabled=""[^>]*>同步 OpenCode 状态<\/button>/);
      expect(nativeAccountCanSync(current, connections)).toBe(false);
      await expect(submitNativeAccountSync(cache, current.id, true, () => true)).rejects.toThrow("原生账号暂不能同步");
      cache.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, connections.map(row => ({ ...row, loginStatus: "cancelled" })));
      expect(nativeAccountCanSync(current, cache.getQueryData(CLIENT_CONNECTIONS_QUERY_KEY))).toBe(true);
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects unavailable, disabled, revoked or unapproved native state before IPC", async () => {
    const current = nativeAccount();
    const cache = accountClient([current], [nativeConnection()]);
    for (const change of [{ enabled: false }, { destinationApproved: false }, { status: "disabled" }, { status: "revoked" },
      { status: "refreshing" }, { status: "destinationApprovalRequired" }, { kind: "oauth" }, { client: null }] as Partial<AccountInfo>[]) {
      cache.setQueryData(ACCOUNT_QUERY_KEYS.list, [{ ...current, ...change }]);
      await expect(submitNativeAccountSync(cache, current.id, true, () => true)).rejects.toThrow("原生账号暂不能同步");
    }
    cache.setQueryData(ACCOUNT_QUERY_KEYS.list, [current]);
    for (const rows of [[], [nativeConnection({ setupError: "unavailable" })], [nativeConnection({ configurationPending: true })]]) {
      cache.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, rows);
      await expect(submitNativeAccountSync(cache, current.id, true, () => true)).rejects.toThrow("原生账号暂不能同步");
    }
    cache.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, [nativeConnection()]);
    cache.getQueryCache().find({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY })!.setState({ status: "error", error: new Error("unavailable") });
    await expect(submitNativeAccountSync(cache, current.id, true, () => true)).rejects.toThrow("原生账号暂不能同步");
    expect(nativeAccountCanSync(current, undefined)).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("requires explicit approval, uses the id contract and invalidates metadata after success", async () => {
    const current = nativeAccount();
    const cache = accountClient([current], [nativeConnection()]);
    const invalidation = vi.spyOn(cache, "invalidateQueries");
    await expect(submitNativeAccountSync(cache, current.id, false, () => true)).rejects.toMatchObject({ code: "account_authorization_required" });
    await expect(submitNativeAccountSync(cache, current.id, true, () => false)).rejects.toMatchObject({ code: "account_cancelled" });
    expect(invoke).not.toHaveBeenCalled();
    const updated = nativeAccount({ status: "available", revision: current.revision + 1 });
    invoke.mockResolvedValueOnce(updated);
    expect(await submitNativeAccountSync(cache, current.id, true, () => true)).toEqual(updated);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("refresh_native_account", { id: current.id, confirmed: true });
    expect(cache.getQueryData(ACCOUNT_QUERY_KEYS.list)).toEqual([updated]);
    expect(invalidation).toHaveBeenCalledWith({ queryKey: ACCOUNT_QUERY_KEYS.list });
    expect(invalidation).toHaveBeenCalledWith({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY });
  });

  it("invalidates account and client metadata after a failed sync without exposing secrets", async () => {
    const current = nativeAccount();
    const cache = accountClient([current], [nativeConnection()]);
    const invalidation = vi.spyOn(cache, "invalidateQueries");
    invoke.mockRejectedValueOnce(new Error("private@example.test Bearer synthetic-private"));
    await expect(submitNativeAccountSync(cache, current.id, true, () => true)).rejects.toThrow("客户端操作未完成");
    expect(cache.getQueryData(ACCOUNT_QUERY_KEYS.list)).toEqual([current]);
    expect(invalidation).toHaveBeenCalledWith({ queryKey: ACCOUNT_QUERY_KEYS.list });
    expect(invalidation).toHaveBeenCalledWith({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY });
  });

  it("excludes native references from fixed HTTP account choices and policy validation", () => {
    const native = nativeAccount({ id: "native-reference-only", alias: "原生引用" });
    const current = account();
    const fixed: AccountPolicy = { ...DEFAULT_ACCOUNT_POLICY, mode: "fixed", fixedAccountId: native.id };
    expect(accountPolicyValid(fixed, [current, native])).toBe(false);
    expect(accountPolicyValid({ ...fixed, fixedAccountId: current.id }, [current, native])).toBe(true);
    const html = renderToStaticMarkup(createElement(AccountPolicyFields, { policy: fixed, accounts: [current, native], disabled: false, onChange: () => {} }));
    expect(html).not.toContain(native.id);
    expect(html).not.toContain(native.alias);
    expect(html).toContain(current.id);
  });
});

describe("selected-file import", () => {
  it("requires confirmation before opening a chooser and rejects a selection arriving after departure", async () => {
    await expect(selectAccountImport(false, () => true)).rejects.toMatchObject({ code: "account_authorization_required" });
    expect(invoke).not.toHaveBeenCalled();
    let finish: (selection: SelectedAccountImport) => void = () => {};
    let active = true;
    invoke.mockImplementation(() => new Promise<SelectedAccountImport>(resolve => { finish = resolve; }));
    const choosing = selectAccountImport(true, () => active);
    active = false; finish({ sourceId: "synthetic-picked-source", expiresAt: Math.floor(Date.now() / 1000) + 300 });
    await expect(choosing).rejects.toMatchObject({ code: "account_cancelled" });
    expect(invoke).toHaveBeenCalledExactlyOnceWith("choose_account_import", {});
  });

  it("consumes the opaque selection before IPC so concurrent or failed imports cannot reuse it", async () => {
    const selection = { current: { sourceId: "synthetic-picked-source", expiresAt: Math.floor(Date.now() / 1000) + 300 } as SelectedAccountImport | null };
    const form = { alias: "导入账号", providerId, approvedDestination, allowRefresh: true };
    let fail: (reason: string) => void = () => {};
    invoke.mockImplementation(() => new Promise<never>((_resolve, reject) => { fail = reject; }));
    const importing = submitSelectedImport(form, selection, true, () => true);
    expect(selection.current).toBeNull();
    await expect(submitSelectedImport(form, selection, true, () => true)).rejects.toMatchObject({ code: "account_import_selection_expired" });
    fail("account_import_invalid_file");
    await expect(importing).rejects.toMatchObject({ code: "account_import_invalid_file" });
    expect(invoke).toHaveBeenCalledExactlyOnceWith("import_account", { request: { sourceId: "synthetic-picked-source", ...form, confirmed: true } });
    expect(selection.current).toBeNull();
  });

  it("imports only the current selected source and blocks expired tickets before IPC", async () => {
    const selected = { sourceId: "synthetic-current-source", expiresAt: Math.floor(Date.now() / 1000) + 300 };
    invoke.mockResolvedValueOnce(selected);
    const selection = { current: await selectAccountImport(true, () => true) };
    const imported = account({ kind: "oauth", source: "selectedFile" }); invoke.mockResolvedValueOnce(imported);
    expect(await submitSelectedImport({ alias: "导入账号", providerId, approvedDestination, allowRefresh: false }, selection, true, () => true)).toEqual(imported);
    expect(invoke).toHaveBeenLastCalledWith("import_account", { request: { sourceId: selected.sourceId, alias: "导入账号", providerId, approvedDestination, allowRefresh: false, confirmed: true } });
    expect(selection.current).toBeNull();
    invoke.mockClear();
    const expired = { current: { sourceId: "synthetic-expired-source", expiresAt: Math.floor(Date.now() / 1000) - 1 } as SelectedAccountImport | null };
    await expect(submitSelectedImport({ alias: "导入账号", providerId, approvedDestination, allowRefresh: false }, expired, true, () => true)).rejects.toMatchObject({ code: "account_import_selection_expired" });
    expect(expired.current).toBeNull(); expect(invoke).not.toHaveBeenCalled();
  });
});

describe("account metadata, quota and integration boundaries", () => {
  it("renders only metadata and hides emails, token-looking aliases, quota IDs and native references", () => {
    const token = "sk-synthetic-hidden-12345678";
    const current = Object.assign(account({ alias: "synthetic-person@example.test", kind: "nativeManaged", client: "codexDesktop", source: "nativeClient", quota: evidence(Math.floor(Date.now() / 1000), 5) }), {
      email: "synthetic-person@example.test", accessToken: token, refreshToken: "synthetic-hidden-refresh", nativeReference: "synthetic-private-client-reference",
    });
    const html = renderPage([current]);
    expect(html).toContain("未命名账号");
    expect(html).toContain("原生客户端管理");
    expect(html).toContain("尚未验证授权");
    for (const sensitive of [current.email, token, current.refreshToken, current.nativeReference, "private-window-id"]) expect(html).not.toContain(sensitive);
    expect(safeAccountLabel(token)).toBe("未命名账号");
    expect(safeAccountLabel("Bearer synthetic-hidden")).toBe("未命名账号");
    expect(validAccountAlias(current.email)).toBe(false);
    expect(validAccountAlias("工作账号")).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps missing, untrusted, stale and invalid quota evidence unknown", () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const valid = evidence(timestamp);
    const snapshots: (QuotaSnapshot | null)[] = [null,
      { ...valid, source: "unknown" }, { ...valid, validUntil: timestamp - 1 }, { ...valid, observedAt: timestamp + 6 },
      { ...valid, windows: [{ ...valid.windows[0], remaining: -1 }] },
      { ...valid, windows: [{ ...valid.windows[0], remaining: Number.NaN }] },
      { ...valid, windows: [{ ...valid.windows[0], remaining: 101 }] },
      { ...valid, windows: [{ ...valid.windows[0], resetsAt: timestamp - 1 }] },
    ];
    for (const quota of snapshots) {
      expect(knownQuotaWindows(quota, timestamp)).toEqual([]);
      const html = renderToStaticMarkup(createElement(AccountQuotaView, { quota, timestamp }));
      expect(html).toContain("额度未知");
      expect(html).not.toContain("剩余 0");
      expect(html).not.toContain("private-window-id");
    }
  });

  it("displays trusted zero quota and its source separately from expired windows", () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const quota = evidence(timestamp);
    const html = renderToStaticMarkup(createElement(AccountQuotaView, { quota, timestamp }));
    expect(html).toContain("额度来源：供应商响应");
    expect(html).toContain("剩余 0 / 上限 100");
    expect(html).toContain("证据有效至");
    expect(html).not.toContain("额度未知");
    const expiredWindow = { ...quota, observedAt: timestamp - 10, windows: [{ ...quota.windows[0], resetsAt: timestamp - 1 }] };
    expect(knownQuotaWindows(expiredWindow, timestamp)).toEqual([]);
  });

  it("enables refresh only for a registered, authorized OAuth callback outside cooldown", () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const oauth = account({ kind: "oauth", allowRefresh: true, canRefresh: true, status: "refreshRequired" });
    expect(accountCanRefresh(oauth, timestamp)).toBe(true);
    for (const change of [{ kind: "nativeManaged" }, { kind: "apiKey" }, { canRefresh: false }, { allowRefresh: false }, { enabled: false }, { destinationApproved: false },
      { status: "refreshing" }, { status: "revoked" }, { status: "loginRequired" }, { status: "destinationApprovalRequired" }, { cooldownUntil: timestamp + 60 }] as Partial<AccountInfo>[]) {
      expect(accountCanRefresh({ ...oauth, ...change }, timestamp)).toBe(false);
    }
    expect(accountCanRefresh({ ...oauth, cooldownUntil: timestamp }, timestamp)).toBe(true);
    const html = renderPage([account({ kind: "oauth", status: "loginRequired" }), account({ id: "synthetic-native", kind: "nativeManaged", source: "nativeClient" })]);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>刷新凭据<\/button>/);
    expect(html).toContain("此账号尚无可用的授权刷新接口");
    expect(html).toContain("请在对应原生客户端完成登录或更新授权");
    expect(html).toContain("PKCE 登录和 SIWC 订阅推理尚待接入");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("requires a listed fixed binding and bounded cooldowns, and presents affinity for rotation", () => {
    const current = account();
    const fixed: AccountPolicy = { ...DEFAULT_ACCOUNT_POLICY, mode: "fixed", fixedAccountId: current.id };
    expect(accountPolicyValid(fixed, [current])).toBe(true);
    expect(accountPolicyValid(fixed, [])).toBe(false);
    expect(accountPolicyValid({ ...fixed, fixedAccountId: "other-account" }, [current])).toBe(false);
    expect(accountPolicyValid({ ...DEFAULT_ACCOUNT_POLICY }, [current])).toBe(true);
    expect(accountPolicyValid({ ...DEFAULT_ACCOUNT_POLICY, fixedAccountId: current.id }, [current])).toBe(false);
    for (const failureCooldownSeconds of [0, -1, 1.5, Number.NaN, 86401]) expect(accountPolicyValid({ ...DEFAULT_ACCOUNT_POLICY, failureCooldownSeconds }, [current])).toBe(false);
    expect(accountPolicyValid({ ...DEFAULT_ACCOUNT_POLICY, rateLimitCooldownSeconds: 61, maxCooldownSeconds: 60 }, [current])).toBe(false);
    const fixedHtml = renderToStaticMarkup(createElement(AccountPolicyFields, { policy: fixed, accounts: [current], disabled: false, onChange: () => {} }));
    expect(fixedHtml).toContain("固定使用的账号");
    expect(fixedHtml).toContain('value="synthetic-account" selected=""');
    const rotationHtml = renderToStaticMarkup(createElement(AccountPolicyFields, { policy: { ...DEFAULT_ACCOUNT_POLICY }, accounts: [current], disabled: false, onChange: () => {} }));
    expect(rotationHtml).not.toContain("固定使用的账号");
    expect(rotationHtml).toContain("保持会话账号亲和");
  });

  it("reports local revocation and pending cleanup accurately for active and revoked accounts", () => {
    expect(localRevocationMessage({ localReferenceRevoked: false, credentialsRemoved: false, account: { cleanupPending: true } })).toContain("本地撤销尚未完成");
    expect(localRevocationMessage({ localReferenceRevoked: true, credentialsRemoved: false, account: { cleanupPending: true } })).toContain("可重试本地撤销");
    expect(localRevocationMessage({ localReferenceRevoked: true, credentialsRemoved: true, account: { cleanupPending: false } })).toContain("供应商授权与客户端登录未退出");
    const activeHtml = renderPage([account({ cleanupPending: true })]);
    expect(activeHtml).toContain("当前账号引用仍有效");
    expect(activeHtml).not.toContain("本地引用保持撤销");
    expect(activeHtml).toMatch(/<button[^>]*>撤销本地引用<\/button>/);
    const revokedHtml = renderPage([account({ status: "revoked", cleanupPending: true })]);
    expect(revokedHtml).toContain("本地引用保持撤销");
    expect(revokedHtml).toMatch(/<button[^>]*>重试本地清理<\/button>/);
    expect(revokedHtml).not.toContain("当前账号引用仍有效");
  });
});
