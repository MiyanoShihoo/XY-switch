import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ClientConnectionsPage, clearSensitiveAction } from "./features/clients/ClientConnectionsPage";
import { NativeCodeForm, OpenCodeLoginForm, currentInputs, visible } from "./features/clients/OpenCodeLoginForm";
import { NativeAccountBindingDialog } from "./features/clients/NativeAccountBindingDialog";
import { NativeAccountConfirmDialog } from "./features/clients/NativeAccountConfirmDialog";
import { CLIENT_CONNECTIONS_QUERY_KEY, captureNativeBinding, currentClientConnections, markNativeLoginStarted, nativeCandidateAvailable, nativeClientActionIssue, nativeLoginPending, runNativeAccountOperation, submitNativeBinding } from "./features/clients/native-account-actions";
import { ACCOUNT_QUERY_KEYS } from "./features/accounts/account-actions";
import { bindNativeAccount, cancelClientLogin, clientAuthorizationUrl, completeClientLogin, disconnectClientProvider, loadClientConnections, loadClientLoginMethods, loadClientSettings, logoutClientAccount, pollClientLogin, readClientAccount, refreshNativeAccount, saveClientSettings, startClientLogin, startOpenCodeLogin } from "./lib/clients-service";
import type { AccountInfo } from "./types/accounts";
import type { ClientConnection, ClientSettings, NativeAccountSnapshot, NativeLoginMethod, NativeLoginProgress } from "./types/clients";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const settings: ClientSettings = { opencodeConfig: null, opencodeSchema: null, claudeDesktopProfile: null, codexBinary: null, opencodeBinary: null };
const method: NativeLoginMethod = {
  providerId: "openai", providerLabel: "OpenAI", methodId: "oauth/native", label: "Native OAuth",
  prompts: [
    { kind: "select", key: "mode", message: "Account type", options: [{ label: "Personal", value: "personal" }, { label: "Enterprise", value: "enterprise" }], when: null },
    { kind: "text", key: "tenant", message: "Tenant", options: [], when: { key: "mode", op: "eq", value: "enterprise" } },
  ],
};
const progress: NativeLoginProgress = { clientId: "opencode-cli", status: "pending", verificationUrl: "https://auth.example.test/authorize?state=synthetic", userCode: null, providerId: "openai", instructions: "Continue in the native browser", requiresCode: true };
const snapshot: NativeAccountSnapshot = { clientId: "opencode-cli", authMode: "connected_providers", plan: null, checkedAt: 1, quota: null, quotas: [], providers: [{ providerId: "openai", label: "OpenAI", connectionId: "cred_owned", disconnectable: true }],
  poolCandidates: [{ id: "opaque-native-candidate", label: "原生订阅账号", expiresAt: Math.floor(Date.now() / 1000) + 3600, boundAccountId: null }] };
const bindingProvider = { id: "destination-provider", name: "目标供应商", enabled: true, endpoint: "https://provider.example.test:8443/subscription/v1/" };
const bindingForm = { candidateId: snapshot.poolCandidates[0].id, alias: "工作订阅", providerId: bindingProvider.id };
const bindingRequest = { ...bindingForm, clientId: "opencode-cli" as const, approvedDestination: bindingProvider.endpoint, confirmed: true };
const queryClients: QueryClient[] = [];

function operations(confirmed: boolean, inputs: Record<string, string>) {
  return [
    () => saveClientSettings(settings, confirmed),
    () => readClientAccount("opencode-cli", confirmed),
    () => readClientAccount("codex-desktop", confirmed, true),
    () => startClientLogin("codex-cli", confirmed),
    () => pollClientLogin("opencode-cli", confirmed),
    () => cancelClientLogin("opencode-cli", confirmed),
    () => logoutClientAccount("codex-desktop", confirmed),
    () => loadClientLoginMethods("opencode-cli", confirmed),
    () => startOpenCodeLogin("opencode-cli", method, inputs, confirmed),
    () => completeClientLogin("opencode-cli", "synthetic-code", confirmed),
    () => disconnectClientProvider("opencode-cli", "openai", "cred_owned", confirmed),
    () => bindNativeAccount({ ...bindingRequest, confirmed }),
    () => refreshNativeAccount("native-bound-account", confirmed),
  ];
}

function row(overrides: Partial<ClientConnection> = {}): ClientConnection {
  return { id: "opencode-cli", label: "OpenCode CLI", configOwner: "opencode", configPaths: [], configDetected: false,
    takeoverTarget: "opencode", mcpTarget: "opencode", loginMethod: "opencode_oauth", loginInstructions: "Synthetic native instruction",
    inferenceMode: "chat_gateway", nativeVerification: "pending", setupError: null, configurationPending: false, loginStatus: null, account: snapshot, ...overrides };
}

function bindingClient(rows: ClientConnection[] = [row()], accounts: AccountInfo[] = []): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity, gcTime: Infinity } } });
  queryClients.push(client);
  client.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, rows);
  client.setQueryData(ACCOUNT_QUERY_KEYS.list, accounts);
  client.setQueryData(ACCOUNT_QUERY_KEYS.providers, [bindingProvider]);
  client.setQueryData(["clients", "settings"], settings);
  client.setQueryData(["takeovers"], []);
  return client;
}

function boundAccount(): AccountInfo {
  return { id: "native-bound-account", alias: "工作订阅", providerId: bindingProvider.id, kind: "nativeManaged", client: "openCodeCli", source: "nativeClient",
    status: "available", authorizationStatus: "authorized", destinationApproved: true, enabled: true, revision: 1, credentialGeneration: 0, expiresAt: null,
    cooldownUntil: null, availableModels: [], quota: null, allowRefresh: false, canRefresh: false, cleanupPending: false,
    lastValidatedAt: null, lastUsedAt: null, createdAt: 1, updatedAt: 1 };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.stubGlobal("navigator", { language: "zh-CN", languages: ["zh-CN"] });
});
afterEach(() => {
  for (const client of queryClients.splice(0)) client.clear();
  vi.unstubAllGlobals();
});

describe("client authorization and preview isolation", () => {
  it("rejects every account mutation or private read before reaching the native bridge", async () => {
    const inputs = { mode: "enterprise", tenant: "synthetic-private-input" };
    for (const operation of operations(false, inputs)) await expect(operation()).rejects.toThrow("明确授权");
    expect(invoke).not.toHaveBeenCalled();
    expect(inputs).toEqual({});
  });

  it("covers all desktop and terminal entries without invoking native accounts in a browser", async () => {
    vi.stubGlobal("window", {});
    const entries = await loadClientConnections();
    expect(entries.map(entry => entry.id)).toEqual(["codex-desktop", "codex-cli", "claude-desktop", "claude-desktop-3p", "claude-code", "opencode-desktop", "opencode-cli", "gemini-cli"]);
    expect(entries.filter(entry => entry.configOwner === "codex")).toHaveLength(2);
    expect(entries.filter(entry => entry.configOwner === "opencode")).toHaveLength(2);
    expect(entries.every(entry => entry.account === null && entry.nativeVerification === "preview" && !entry.configDetected)).toBe(true);
    expect(entries.find(entry => entry.id === "claude-desktop")?.takeoverTarget).toBeNull();
    expect(await loadClientSettings()).toEqual(settings);
    const inputs = { mode: "enterprise", tenant: "synthetic-private-input" };
    for (const operation of operations(true, inputs)) await expect(operation()).rejects.toThrow("浏览器预览");
    expect(invoke).not.toHaveBeenCalled();
    expect(inputs).toEqual({});
  });

  it("renders explicit preview status and keeps account controls disabled", async () => {
    vi.stubGlobal("window", {});
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity, gcTime: Infinity } } });
    queryClients.push(client);
    client.setQueryData(["clients", "connections"], await loadClientConnections());
    client.setQueryData(["clients", "settings"], settings);
    client.setQueryData(["takeovers"], []);
    const markup = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(ClientConnectionsPage)));
    for (const label of ["Codex Desktop", "Claude Desktop", "OpenCode Desktop", "Codex CLI", "Claude Code", "OpenCode CLI", "Gemini CLI"]) expect(markup).toContain(label);
    expect(markup).toContain("浏览器预览仅展示入口");
    expect(markup).toContain("OpenCode 桌面与终端共用所选配置");
    const signInControls = [...markup.matchAll(/<button\b([^>]*)>提供商授权登录<\/button>/g)];
    expect(signInControls).toHaveLength(2);
    expect(signInControls.every(control => control[1].includes('disabled=""'))).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("native client command contracts", () => {
  it("dispatches approved account, login, cancellation and exact connection operations", async () => {
    invoke.mockResolvedValue(progress);
    await loadClientConnections(); expect(invoke).toHaveBeenLastCalledWith("list_client_connections", {});
    await loadClientSettings(); expect(invoke).toHaveBeenLastCalledWith("get_client_settings", {});
    await saveClientSettings(settings, true); expect(invoke).toHaveBeenLastCalledWith("save_client_settings", { settings });
    await readClientAccount("opencode-desktop", true, true); expect(invoke).toHaveBeenLastCalledWith("read_client_account", { clientId: "opencode-desktop", confirmed: true, refresh: true });
    await startClientLogin("codex-desktop", true); expect(invoke).toHaveBeenLastCalledWith("start_client_login", { clientId: "codex-desktop", confirmed: true });
    await loadClientLoginMethods("opencode-desktop", true); expect(invoke).toHaveBeenLastCalledWith("list_client_login_methods", { clientId: "opencode-desktop", confirmed: true });
    await pollClientLogin("opencode-desktop", true); expect(invoke).toHaveBeenLastCalledWith("poll_client_login", { clientId: "opencode-desktop", confirmed: true });
    await cancelClientLogin("opencode-desktop", true); expect(invoke).toHaveBeenLastCalledWith("cancel_client_login", { clientId: "opencode-desktop", confirmed: true });
    await logoutClientAccount("codex-desktop", true); expect(invoke).toHaveBeenLastCalledWith("logout_client_account", { clientId: "codex-desktop", confirmed: true });
    await disconnectClientProvider("opencode-desktop", "openai", "cred_owned", true); expect(invoke).toHaveBeenLastCalledWith("disconnect_client_provider", { clientId: "opencode-desktop", confirmed: true, providerId: "openai", connectionId: "cred_owned" });
  });

  it("submits provider inputs once and clears request references after completion", async () => {
    let dispatched: unknown;
    invoke.mockImplementation(async (_command: string, args: unknown) => { dispatched = structuredClone(args); return progress; });
    const inputs = { mode: "enterprise", tenant: "synthetic-private-input" };
    expect(await startOpenCodeLogin("opencode-cli", method, inputs, true)).toEqual(progress);
    expect(dispatched).toEqual({ clientId: "opencode-cli", confirmed: true, providerId: "openai", methodId: "oauth/native", inputs: { mode: "enterprise", tenant: "synthetic-private-input" } });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(inputs).toEqual({});
    expect(invoke.mock.calls[0][1].inputs).toEqual({});
  });

  it("submits the authorized code once and clears its bridge request on success", async () => {
    let dispatched: unknown;
    invoke.mockImplementation(async (_command: string, args: unknown) => { dispatched = structuredClone(args); return progress; });
    expect(await completeClientLogin("opencode-cli", "synthetic-authorization-code", true)).toEqual(progress);
    expect(dispatched).toEqual({ clientId: "opencode-cli", confirmed: true, code: "synthetic-authorization-code" });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0][1].code).toBe("");
  });

  it("hides arbitrary bridge errors and clears failed inputs and code requests", async () => {
    const privateError = "person@example.test Bearer synthetic-private-secret C:/synthetic/private-auth.json";
    invoke.mockRejectedValue(new Error(privateError));
    const inputs = { mode: "enterprise", tenant: "synthetic-private-input" };
    for (const operation of [() => startOpenCodeLogin("opencode-cli", method, inputs, true), () => completeClientLogin("opencode-cli", "synthetic-code", true), () => loadClientConnections()]) {
      let caught: unknown;
      try { await operation(); } catch (error) { caught = error; }
      expect(caught).toBeInstanceOf(Error);
      expect(String(caught)).toContain("客户端操作未完成");
      expect(String(caught)).not.toContain(privateError);
      expect(String(caught)).not.toContain("synthetic-private");
    }
    expect(inputs).toEqual({});
    expect(invoke.mock.calls[1][1].code).toBe("");
  });

  it("releases pending action input references when the dialog or page closes", async () => {
    let finish: (value: NativeLoginProgress) => void = () => { throw new Error("request did not start"); };
    let dispatched: unknown;
    invoke.mockImplementation((_command: string, args: unknown) => {
      dispatched = structuredClone(args);
      return new Promise<NativeLoginProgress>(resolve => { finish = resolve; });
    });
    const inputs = { mode: "enterprise", tenant: "synthetic-private-input" };
    const action = { kind: "opencode-start" as const, row: row(), method, inputs };
    const pending = startOpenCodeLogin("opencode-cli", method, inputs, true);
    clearSensitiveAction(action);
    expect(inputs).toEqual({});
    expect(dispatched).toMatchObject({ inputs: { tenant: "synthetic-private-input" } });
    finish(progress);
    await pending;
    const codeAction = { kind: "complete" as const, row: row(), code: "synthetic-code" };
    clearSensitiveAction(codeAction);
    expect(codeAction.code).toBe("");
    clearSensitiveAction(null);
  });
});

describe("native authorization links and conditional forms", () => {
  it("permits HTTPS provider links and restricts Codex links to official authorization hosts", () => {
    expect(clientAuthorizationUrl("https://auth.openai.com/device", "codex-desktop")).toBe("https://auth.openai.com/device");
    expect(clientAuthorizationUrl("https://chatgpt.com/activate", "codex-cli")).toBe("https://chatgpt.com/activate");
    expect(clientAuthorizationUrl("https://provider.example.test/native", "opencode-cli")).toBe("https://provider.example.test/native");
    for (const url of ["http://auth.openai.com/device", "javascript:alert(1)", "https://user:secret@auth.openai.com/device", "https://auth.openai.com:4444/device", "https://auth.openai.com.evil.test/device", "https://provider.example.test/native"]) expect(clientAuthorizationUrl(url, "codex-cli")).toBeNull();
    for (const url of [null, "", "http://provider.example.test/native", "https://user:secret@provider.example.test", "https://provider.example.test:4444", "x".repeat(8193)]) expect(clientAuthorizationUrl(url, "opencode-cli")).toBeNull();
  });

  it("keeps only fields visible for the selected native method and removes hidden inputs", () => {
    expect(currentInputs(method, {})).toEqual({ mode: "personal" });
    expect(currentInputs(method, { mode: "enterprise", tenant: "synthetic-tenant", extra: "synthetic-extra" })).toEqual({ mode: "enterprise", tenant: "synthetic-tenant" });
    const changed = currentInputs(method, { mode: "personal", tenant: "synthetic-hidden" });
    expect(changed).toEqual({ mode: "personal" });
    expect(visible(method.prompts[1], changed)).toBe(false);
    expect(Object.getPrototypeOf(changed)).toBeNull();
    const inherited = Object.create({ tenant: "synthetic-inherited" }) as Record<string, string>;
    inherited.mode = "enterprise";
    expect(currentInputs(method, inherited).tenant).toBe("");
    expect(visible({ ...method.prompts[1], when: { key: "mode", op: "neq", value: "personal" } }, { mode: "enterprise" })).toBe(true);
    expect(visible(method.prompts[1], {})).toBe(false);
  });

  it("uses private bounded form fields and disables empty code submission", () => {
    const enterpriseMethod = { ...method, prompts: [method.prompts[1]].map(prompt => ({ ...prompt, when: null })) };
    const form = renderToStaticMarkup(createElement(OpenCodeLoginForm, { methods: [enterpriseMethod], disabled: false, onStart: vi.fn(), onCancel: vi.fn() }));
    expect(form).toContain('type="password"');
    expect(form).toContain('autoComplete="off"');
    expect(form).toContain('maxLength="4096"');
    expect(form).toContain("确认或关闭表单后清除");
    const code = renderToStaticMarkup(createElement(NativeCodeForm, { disabled: false, onSubmit: vi.fn() }));
    expect(code).toContain('type="password"');
    expect(code).toContain('maxLength="16384"');
    expect(code).toContain('disabled=""');
    const empty = renderToStaticMarkup(createElement(OpenCodeLoginForm, { methods: [], disabled: false, onStart: vi.fn(), onCancel: vi.fn() }));
    expect(empty).toContain("没有可用的 OAuth 登录方式");
    expect(empty).not.toContain("检查并授权登录");
  });
});

describe("native account binding and shared login state", () => {
  it("uses only the opaque candidate and the exact approved destination, and refreshes with id", async () => {
    const saved = boundAccount();
    invoke.mockResolvedValue(saved);
    expect(await bindNativeAccount(bindingRequest)).toEqual(saved);
    expect(invoke).toHaveBeenLastCalledWith("bind_native_account", { request: bindingRequest });
    expect(Object.keys(invoke.mock.calls[0][1].request).sort()).toEqual(["alias", "approvedDestination", "candidateId", "clientId", "confirmed", "providerId"]);
    expect(await refreshNativeAccount(saved.id, true)).toEqual(saved);
    expect(invoke).toHaveBeenLastCalledWith("refresh_native_account", { id: saved.id, confirmed: true });
    expect(invoke.mock.calls[1][1]).not.toHaveProperty("accountId");
  });

  it("rejects truthy values that are not explicit authorization before IPC", async () => {
    for (const confirmed of ["true", 1, null, undefined]) {
      await expect(bindNativeAccount({ ...bindingRequest, confirmed: confirmed as unknown as boolean })).rejects.toThrow("明确授权");
      await expect(refreshNativeAccount("native-bound-account", confirmed as unknown as boolean)).rejects.toThrow("明确授权");
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("disables an open native confirmation when login becomes pending while allowing cancellation", () => {
    const html = renderToStaticMarkup(createElement(NativeAccountConfirmDialog, {
      title: "原生账号同步", message: "已明确授权的原生账号操作", confirmLabel: "授权并继续", disabled: true,
      error: "原生登录进行中", onCancel: vi.fn(), onConfirm: vi.fn(),
    }));
    expect(html).toContain('role="alertdialog"');
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>授权并继续<\/button>/);
    expect(html).toMatch(/<button(?![^>]*disabled)[^>]*>取消<\/button>/);
    expect(html).toContain("原生登录进行中");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows full destination approval in a separate dialog and keeps native identity private", () => {
    const nativeRow = row({ account: { ...snapshot, poolCandidates: [{ ...snapshot.poolCandidates[0], label: "private-person@example.test" }] } });
    const client = bindingClient([nativeRow]);
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(NativeAccountBindingDialog, {
      row: nativeRow, rows: [nativeRow], login: null, accountsReady: true, onClose: vi.fn(), onBound: vi.fn(),
    })));
    expect(html).toContain('role="dialog"');
    expect(html).toContain('id="native-binding-alias"');
    expect(html).toContain('id="native-binding-provider"');
    expect(html).toContain(bindingProvider.endpoint);
    expect(html).toContain("完整目的地");
    expect(html).toContain('type="checkbox"');
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>授权并绑定<\/button>/);
    expect(html).toContain("登录仍由原生客户端管理");
    expect(html).toContain("尚不参与网关的 HTTP 账号轮换");
    expect(html).not.toContain("private-person@example.test");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects expired or bound candidates, unsafe aliases and invalid providers", () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const candidate = snapshot.poolCandidates[0];
    expect(nativeCandidateAvailable(candidate, timestamp)).toBe(true);
    for (const change of [{ expiresAt: timestamp }, { expiresAt: Number.NaN }, { boundAccountId: "already-bound" }, { id: "" }]) {
      const changed = { ...candidate, ...change };
      expect(nativeCandidateAvailable(changed, timestamp)).toBe(false);
      expect(() => captureNativeBinding(row({ account: { ...snapshot, poolCandidates: [changed] } }), bindingForm, [bindingProvider], timestamp)).toThrow();
    }
    for (const alias of ["", "private@example.test", "sk-synthetic-private-key", "https://private.example.test", " padded "]) {
      expect(() => captureNativeBinding(row(), { ...bindingForm, alias }, [bindingProvider], timestamp)).toThrow("别名");
    }
    for (const providers of [[], [{ ...bindingProvider, enabled: false }], [{ ...bindingProvider, endpoint: "https://private:secret@provider.example.test/v1" }]]) {
      expect(() => captureNativeBinding(row(), bindingForm, providers, timestamp)).toThrow();
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rechecks the candidate and exact destination after approval, including changed URL paths", async () => {
    const client = bindingClient();
    const approval = captureNativeBinding(row(), bindingForm, [bindingProvider], Math.floor(Date.now() / 1000));
    await expect(submitNativeBinding(client, approval, false, () => true)).rejects.toThrow("明确授权");
    await expect(submitNativeBinding(client, approval, true, () => false)).rejects.toThrow("已取消");
    client.setQueryData(ACCOUNT_QUERY_KEYS.providers, [{ ...bindingProvider, endpoint: "https://provider.example.test:8443/changed/v1/" }]);
    await expect(submitNativeBinding(client, approval, true, () => true)).rejects.toThrow("目的地已改变");
    client.setQueryData(ACCOUNT_QUERY_KEYS.providers, [bindingProvider]);
    for (const candidates of [[], [{ ...snapshot.poolCandidates[0], expiresAt: 1 }], [{ ...snapshot.poolCandidates[0], boundAccountId: "other" }]]) {
      client.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, [row({ account: { ...snapshot, poolCandidates: candidates } })]);
      await expect(submitNativeBinding(client, approval, true, () => true)).rejects.toThrow("重新授权读取");
    }
    client.setQueryData(CLIENT_CONNECTIONS_QUERY_KEY, [row()]);
    await expect(submitNativeBinding(client, approval, true, () => true, progress)).rejects.toThrow("原生登录进行中");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("invalidates native and account metadata on binding success and failure without exposing backend errors", async () => {
    const client = bindingClient();
    const invalidation = vi.spyOn(client, "invalidateQueries");
    const approval = captureNativeBinding(row(), bindingForm, [bindingProvider], Math.floor(Date.now() / 1000));
    invoke.mockResolvedValueOnce(boundAccount());
    expect(await submitNativeBinding(client, approval, true, () => true)).toEqual(boundAccount());
    expect(client.getQueryData(ACCOUNT_QUERY_KEYS.list)).toEqual([boundAccount()]);
    expect(invalidation).toHaveBeenCalledWith({ queryKey: ACCOUNT_QUERY_KEYS.list });
    expect(invalidation).toHaveBeenCalledWith({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY });
    invalidation.mockClear();
    invoke.mockRejectedValueOnce(new Error("private@example.test Bearer synthetic-secret"));
    await expect(submitNativeBinding(client, approval, true, () => true)).rejects.toThrow("客户端操作未完成");
    expect(invalidation).toHaveBeenCalledWith({ queryKey: ACCOUNT_QUERY_KEYS.list });
    expect(invalidation).toHaveBeenCalledWith({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY });
  });

  it("invalidates the account list after every native read, login, logout and disconnect, even on failure", async () => {
    const client = bindingClient();
    const invalidation = vi.spyOn(client, "invalidateQueries");
    for (const fails of [false, true]) {
      for (const operation of operations(true, { mode: "personal" })) {
        invalidation.mockClear();
        if (fails) invoke.mockRejectedValueOnce(new Error("synthetic-native-failure")); else invoke.mockResolvedValueOnce(progress);
        const pending = runNativeAccountOperation(client, async () => { await operation(); });
        if (fails) await expect(pending).rejects.toThrow("客户端操作未完成"); else await pending;
        expect(invalidation).toHaveBeenCalledWith({ queryKey: ACCOUNT_QUERY_KEYS.list });
        expect(invalidation).toHaveBeenCalledWith({ queryKey: CLIENT_CONNECTIONS_QUERY_KEY });
      }
    }
  });

  it.each(["codex", "opencode"] as const)("blocks reads, binding and refresh for both %s entries while shared login is pending", async owner => {
    const desktop = row({ id: owner === "codex" ? "codex-desktop" : "opencode-desktop", configOwner: owner, loginMethod: owner === "codex" ? "codex_device_code" : "opencode_oauth" });
    const terminal = { ...desktop, id: owner === "codex" ? "codex-cli" as const : "opencode-cli" as const, loginStatus: "pending" };
    const rows = [desktop, terminal];
    const client = bindingClient(rows);
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(ClientConnectionsPage)));
    for (const label of owner === "codex" ? ["读取账号", "绑定原生账号", "刷新登录状态"] : ["读取账号", "绑定原生账号"]) {
      const controls = [...html.matchAll(new RegExp('<button\\b([^>]*)>' + label + '<\\/button>', 'g'))];
      expect(controls).toHaveLength(2);
      expect(controls.every(control => control[1].includes('disabled=""'))).toBe(true);
    }
    expect(nativeLoginPending(desktop.id, rows)).toBe(true);
    expect(nativeClientActionIssue(desktop.id, rows)).toContain("原生登录进行中");
    expect(nativeLoginPending(owner === "codex" ? "opencode-cli" : "codex-cli", rows)).toBe(false);
    const approval = captureNativeBinding(desktop, bindingForm, [bindingProvider], Math.floor(Date.now() / 1000));
    await expect(submitNativeBinding(client, approval, true, () => true)).rejects.toThrow("原生登录进行中");
    const idle = rows.map(item => ({ ...item, loginStatus: null }));
    expect(nativeClientActionIssue(desktop.id, idle)).toBe("");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("clears both shared snapshots when login starts and displays already bound metadata without claiming gateway access", () => {
    const bound = { ...snapshot.poolCandidates[0], boundAccountId: boundAccount().id };
    const terminal = row({ account: { ...snapshot, poolCandidates: [bound] } });
    const desktop = { ...terminal, id: "opencode-desktop" as const };
    const client = bindingClient([terminal, desktop], [boundAccount()]);
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(ClientConnectionsPage)));
    expect(html).toContain("已绑定 · 工作订阅 · 原生状态已同步");
    expect(html).not.toContain("可参与选择");
    expect(html).not.toContain('id="native-binding-alias"');
    markNativeLoginStarted(client, "opencode-desktop");
    expect(currentClientConnections(client)?.every(item => item.account === null && item.loginStatus === "pending")).toBe(true);
  });
});
