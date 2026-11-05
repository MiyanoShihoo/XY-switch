import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  applyPluginWithdrawal, configureTrustedPluginKey, confirmPluginInstall, disablePlugin, enablePluginClient,
  executePluginTool, listInstalledPlugins, listTrustedPluginKeys, maxPluginEnvelopeBytes, PluginServiceError,
  pluginErrorText, pluginMappingsForClient, pluginQueryKeys, pluginSkillsForClient, pluginStatusText,
  pluginToolsForClient, previewInstalledPlugin, previewPluginInstall, previewPluginRollback, previewPluginTool,
  previewTrustedPluginKey, readPluginDocument, revokeTrustedPluginKey, uninstallPlugin, validatePluginPackage,
} from "./lib/plugins-service";
import { PluginsPage } from "./features/plugins/PluginsPage";
import { InstallReviewDialog, ToolReviewDialog, TrustedKeyDialog, WithdrawalReviewDialog } from "./features/plugins/PluginReviewDialogs";
import {
  confirmInstallReview, confirmKeyReview, confirmToolReview, confirmWithdrawalReview, readWithdrawalReview,
  type InstallChoices, type ToolDraft, type ToolReview, type TrustedKeyReview,
} from "./features/plugins/review";
import {
  emptyPluginPermissions, type InstalledPlugin, type InstallPreview, type PluginManifest,
  type PluginWithdrawal, type TrustedKeyInput,
} from "./types/plugins";

const digest = "a".repeat(64);
const argumentsDigest = "b".repeat(64);
const fingerprint = "c".repeat(64);
const previewId = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const approvedTime = 1_800_000_000_000;

function manifestFixture(): PluginManifest {
  return {
    schemaVersion: 1, id: "example.reader", name: "Approved Reader", description: "Read reviewed text.",
    author: "Synthetic publisher", version: "1.0.0", minimumHostVersion: "0.0.0", kind: "mcp", entry: "tools.json",
    clients: ["codex-desktop", "codex-cli"], permissions: { ...emptyPluginPermissions(), readPaths: ["package:docs/"] }, dependencies: [],
  };
}

function installFixture(): InstallPreview {
  return {
    previewId, expiresAt: approvedTime + 120_000,
    validation: { digest, canonicalSize: 1024, fileCount: 2, manifest: manifestFixture(), runtimeSupported: true, warnings: [] },
    signingKeyId: "example.publisher", signingKeyFingerprint: fingerprint, previousVersion: "0.9.0",
    permissionChanges: [{ client: "codex-desktop", added: { ...emptyPluginPermissions(), readPaths: ["package:docs/"] } }],
  };
}

function choicesFixture(): InstallChoices {
  return { digest, clients: ["codex-desktop"], accepted: true, enable: false };
}

function keyFixture(): TrustedKeyReview {
  const input = { id: "example.publisher", label: "Synthetic publisher", publicKey: "d".repeat(64) };
  return { input, preview: { ...input, fingerprint, enabled: true } };
}

function toolFixture(): ToolReview {
  const draft: ToolDraft = { pluginId: "example.reader", version: "1.0.0", pluginDigest: digest, client: "codex-desktop",
    tool: "read", argumentsJson: '{"path":"docs/approved.md"}' };
  return { draft, preview: { previewId, expiresAt: approvedTime + 120_000, pluginId: draft.pluginId, version: draft.version,
    pluginDigest: digest, client: draft.client, tool: draft.tool, argumentsDigest,
    requiredPermissions: { ...emptyPluginPermissions(), readPaths: ["package:docs/approved.md"] }, effect: "Read reviewed package text." } };
}

function withdrawalFixture(): PluginWithdrawal {
  return { id: "example.reader", version: "0.9.0", digest, reason: "privacy", signingKeyId: "example.publisher", signature: "e".repeat(128) };
}

function installedFixture(): InstalledPlugin {
  return { id: "example.reader", manifest: manifestFixture(), activeVersion: "1.0.0", digest, installed: true,
    enabledClients: ["codex-desktop"], verification: "verified", warnings: [], versions: [
      { version: "1.0.0", digest, installedAt: approvedTime, signingKeyId: "example.publisher", verification: "verified", withdrawn: false },
      { version: "0.9.0", digest: "f".repeat(64), installedAt: approvedTime - 1000, signingKeyId: "example.publisher", verification: "plugin_withdrawn", withdrawn: true },
    ] };
}

describe("local plugin native command contracts", () => {
  beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); });
  afterEach(() => { vi.unstubAllGlobals(); });

  it("passes the original signed JSON to preview and only an exact review to installation", async () => {
    const raw = '{"package":{"manifest":{},"files":{}},"digest":"reviewed"}';
    const preview = installFixture();
    invoke.mockResolvedValueOnce(preview).mockResolvedValueOnce(installedFixture());
    expect(await previewPluginInstall(raw)).toEqual(preview);
    expect(invoke).toHaveBeenLastCalledWith("plugins_preview_install", { publishedJson: raw });
    const confirmation = confirmInstallReview(preview, choicesFixture(), approvedTime);
    expect(await confirmPluginInstall(confirmation)).toEqual(installedFixture());
    expect(invoke).toHaveBeenLastCalledWith("plugins_confirm_install", { confirmationJson: JSON.stringify(confirmation) });
    expect(JSON.parse(invoke.mock.lastCall![1].confirmationJson)).not.toHaveProperty("package");
  });

  it("requires explicit trust and keeps the host fingerprint in its configuration command", async () => {
    const review = keyFixture();
    invoke.mockResolvedValue(review.preview);
    expect(await previewTrustedPluginKey(review.input)).toEqual(review.preview);
    expect(invoke).toHaveBeenLastCalledWith("plugins_preview_trusted_key", { keyJson: JSON.stringify(review.input) });
    expect(() => configureTrustedPluginKey(review.input, fingerprint, false)).toThrow(PluginServiceError);
    expect(() => configureTrustedPluginKey(review.input, "C".repeat(64), true)).toThrow(PluginServiceError);
    const key = confirmKeyReview(review, review.input, fingerprint, true);
    await configureTrustedPluginKey(key, fingerprint, true);
    expect(invoke).toHaveBeenLastCalledWith("plugins_configure_trusted_key", { keyJson: JSON.stringify(key), fingerprint, confirmed: true });
    await listTrustedPluginKeys();
    expect(invoke).toHaveBeenLastCalledWith("plugins_list_trusted_keys");
    await revokeTrustedPluginKey(key.id);
    expect(invoke).toHaveBeenLastCalledWith("plugins_revoke_trusted_key", { id: key.id });
  });

  it("uses real lifecycle commands with explicit clients and immutable rollback versions", async () => {
    invoke.mockResolvedValue(installedFixture());
    await listInstalledPlugins();
    expect(invoke).toHaveBeenLastCalledWith("plugins_list_installed");
    await previewInstalledPlugin("example.reader");
    expect(invoke).toHaveBeenLastCalledWith("plugins_preview_installed", { id: "example.reader" });
    await disablePlugin("example.reader", "codex-cli");
    expect(invoke).toHaveBeenLastCalledWith("plugins_disable", { id: "example.reader", client: "codex-cli" });
    await disablePlugin("example.reader");
    expect(invoke).toHaveBeenLastCalledWith("plugins_disable", { id: "example.reader", client: null });
    await enablePluginClient("example.reader", "codex-desktop", digest, true);
    expect(invoke).toHaveBeenLastCalledWith("plugins_enable_client", { id: "example.reader", client: "codex-desktop", digest, confirmed: true });
    await previewPluginRollback("example.reader", "0.9.0");
    expect(invoke).toHaveBeenLastCalledWith("plugins_preview_rollback", { id: "example.reader", version: "0.9.0" });
    await uninstallPlugin("example.reader", true);
    expect(invoke).toHaveBeenLastCalledWith("plugins_uninstall", { id: "example.reader", confirmed: true });
    const withdrawalJson = JSON.stringify(withdrawalFixture());
    await applyPluginWithdrawal(withdrawalJson);
    expect(invoke).toHaveBeenLastCalledWith("plugins_apply_withdrawal", { withdrawalJson });
  });

  it("sends raw argument JSON for strict host parsing and confirms without resending arguments", async () => {
    const storage = { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal("localStorage", storage);
    invoke.mockResolvedValue([]);
    await pluginToolsForClient("codex-cli");
    expect(invoke).toHaveBeenLastCalledWith("plugins_tools", { client: "codex-cli" });
    await pluginSkillsForClient("claude-desktop");
    expect(invoke).toHaveBeenLastCalledWith("plugins_skills", { client: "claude-desktop" });
    await pluginMappingsForClient("opencode-cli");
    expect(invoke).toHaveBeenLastCalledWith("plugins_provider_mappings", { client: "opencode-cli" });
    const raw = '{"path":"first.md","path":"second.md"}';
    await previewPluginTool("example.reader", "codex-desktop", "read", raw);
    expect(invoke).toHaveBeenLastCalledWith("plugins_preview_tool", { id: "example.reader", client: "codex-desktop", tool: "read", argumentsJson: raw });
    const review = toolFixture();
    const confirmation = confirmToolReview(review, review.draft, argumentsDigest, true, approvedTime);
    await executePluginTool("codex-desktop", confirmation);
    expect(invoke).toHaveBeenLastCalledWith("plugins_execute_tool", { client: "codex-desktop", confirmationJson: JSON.stringify(confirmation) });
    expect(JSON.parse(invoke.mock.lastCall![1].confirmationJson)).not.toHaveProperty("arguments");
    expect(storage.setItem).not.toHaveBeenCalled();
    expect(storage.getItem).not.toHaveBeenCalled();
    expect(storage.removeItem).not.toHaveBeenCalled();
  });

  it("hides native secrets and unknown errors while preserving recognized static failures", async () => {
    const syntheticSecret = "synthetic-private-fixture-value";
    invoke.mockRejectedValue({ code: "plugin_privacy", message: syntheticSecret, arguments: syntheticSecret });
    const failure = await previewPluginInstall("{}").catch(cause => cause);
    expect(failure).toBeInstanceOf(PluginServiceError);
    expect(failure.code).toBe("plugin_privacy");
    expect(JSON.stringify(failure)).not.toContain(syntheticSecret);
    expect(pluginErrorText(failure)).toContain("隐私");
    for (const cause of [new Error(syntheticSecret), syntheticSecret, { code: syntheticSecret, message: syntheticSecret }, { code: "__proto__" }]) {
      invoke.mockRejectedValue(cause);
      await expect(listInstalledPlugins()).rejects.toMatchObject({ code: "plugin_operation_failed" });
      expect(pluginErrorText(cause)).not.toContain(syntheticSecret);
    }
    expect(pluginStatusText(syntheticSecret)).not.toContain(syntheticSecret);
  });

  it("rejects missing consent and UTF-8 byte limit violations before IPC", () => {
    const review = toolFixture();
    expect(() => executePluginTool("codex-desktop", { ...confirmToolReview(review, review.draft, argumentsDigest, true, approvedTime), confirmed: false })).toThrow(PluginServiceError);
    expect(() => uninstallPlugin("example.reader", false)).toThrow(PluginServiceError);
    expect(() => enablePluginClient("example.reader", "codex-desktop", digest, false)).toThrow(PluginServiceError);
    expect(() => confirmPluginInstall({ ...confirmInstallReview(installFixture(), choicesFixture(), approvedTime), clients: [] })).toThrow(PluginServiceError);
    expect(() => validatePluginPackage(" \n ")).toThrow(PluginServiceError);
    expect(() => previewPluginInstall("x".repeat(maxPluginEnvelopeBytes + 1))).toThrow(PluginServiceError);
    expect(() => previewPluginInstall("🦆".repeat(Math.floor(maxPluginEnvelopeBytes / 4) + 1))).toThrow(PluginServiceError);
    expect(() => previewPluginTool("example.reader", "codex-desktop", "read", "🦆".repeat(16_385))).toThrow(PluginServiceError);
    expect(() => applyPluginWithdrawal("x".repeat(4097))).toThrow(PluginServiceError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("reads bounded UTF-8 files and rejects invalid encoding or dishonest size metadata", async () => {
    const bytes = new TextEncoder().encode('{"reviewed":"文本"}');
    expect(await readPluginDocument({ size: bytes.length, arrayBuffer: async () => bytes.buffer })).toBe('{"reviewed":"文本"}');
    const read = vi.fn();
    await expect(readPluginDocument({ size: maxPluginEnvelopeBytes + 1, arrayBuffer: read })).rejects.toMatchObject({ code: "plugin_package_size" });
    expect(read).not.toHaveBeenCalled();
    await expect(readPluginDocument({ size: 1, arrayBuffer: async () => new ArrayBuffer(maxPluginEnvelopeBytes + 1) })).rejects.toMatchObject({ code: "plugin_package_size" });
    await expect(readPluginDocument({ size: 2, arrayBuffer: async () => new Uint8Array([0xc3, 0x28]).buffer })).rejects.toMatchObject({ code: "plugin_schema" });
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("exact plugin review confirmations", () => {
  beforeEach(() => { vi.resetAllMocks(); });

  it("binds trust to independently checked fingerprint and unchanged publisher input", () => {
    const review = keyFixture();
    expect(confirmKeyReview(review, review.input, fingerprint, true)).toEqual(review.input);
    for (const changed of [{ ...review.input, id: "other.publisher" }, { ...review.input, label: "Changed label" }, { ...review.input, publicKey: "f".repeat(64) }]) {
      expect(() => confirmKeyReview(review, changed, fingerprint, true)).toThrow(PluginServiceError);
    }
    expect(() => confirmKeyReview(review, review.input, fingerprint, false)).toThrow(PluginServiceError);
    expect(() => confirmKeyReview(review, review.input, digest, true)).toThrow(PluginServiceError);
    expect(() => confirmKeyReview({ ...review, preview: { ...review.preview, publicKey: "f".repeat(64) } }, review.input, fingerprint, true)).toThrow(PluginServiceError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("requires current digest, full permissions and explicitly selected supported clients", () => {
    const preview = installFixture();
    const choices = choicesFixture();
    const confirmation = confirmInstallReview(preview, choices, approvedTime);
    expect(confirmation.acceptedPermissions).toEqual(preview.validation.manifest.permissions);
    expect(confirmation.acceptedPermissions).not.toBe(preview.validation.manifest.permissions);
    expect(confirmation.acceptedPermissions.readPaths).not.toBe(preview.validation.manifest.permissions.readPaths);
    expect(confirmation.enable).toBe(false);
    for (const invalid of [{ ...choices, digest: "f".repeat(64) }, { ...choices, digest: digest.toUpperCase() }, { ...choices, accepted: false },
      { ...choices, clients: [] }, { ...choices, clients: ["codex-desktop", "codex-desktop"] as InstallChoices["clients"] },
      { ...choices, clients: ["claude-desktop"] as InstallChoices["clients"] }]) {
      expect(() => confirmInstallReview(preview, invalid, approvedTime)).toThrow(PluginServiceError);
    }
    expect(() => confirmInstallReview(preview, choices, preview.expiresAt)).toThrow("过期");
    expect(() => confirmInstallReview({ ...preview, expiresAt: Number.NaN }, choices, approvedTime)).toThrow("过期");
  });

  it("keeps unsupported capabilities disabled even when a package can be stored", () => {
    const preview = installFixture();
    preview.validation.runtimeSupported = false;
    preview.validation.manifest.permissions.process = true;
    expect(confirmInstallReview(preview, choicesFixture(), approvedTime).enable).toBe(false);
    expect(() => confirmInstallReview(preview, { ...choicesFixture(), enable: true }, approvedTime)).toThrow(PluginServiceError);
  });

  it("rejects changed arguments, client, tool, version, package digest and expired reviews", () => {
    const review = toolFixture();
    const confirmation = confirmToolReview(review, review.draft, argumentsDigest, true, approvedTime);
    expect(confirmation).toEqual({ previewId, pluginDigest: digest, argumentsDigest, acceptedPermissions: review.preview.requiredPermissions, confirmed: true });
    expect(confirmation.acceptedPermissions.readPaths).not.toBe(review.preview.requiredPermissions.readPaths);
    for (const change of [{ argumentsJson: '{"path":"other.md"}' }, { client: "codex-cli" as const }, { tool: "other" },
      { version: "1.1.0" }, { pluginId: "other.reader" }, { pluginDigest: "f".repeat(64) }]) {
      expect(() => confirmToolReview(review, { ...review.draft, ...change }, argumentsDigest, true, approvedTime)).toThrow(PluginServiceError);
    }
    expect(() => confirmToolReview(review, review.draft, digest, true, approvedTime)).toThrow(PluginServiceError);
    expect(() => confirmToolReview(review, review.draft, argumentsDigest, false, approvedTime)).toThrow(PluginServiceError);
    expect(() => confirmToolReview(review, review.draft, argumentsDigest, true, review.preview.expiresAt)).toThrow("过期");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps withdrawal content immutable until confirmation and rejects unsupported fields", () => {
    const raw = JSON.stringify(withdrawalFixture());
    const review = readWithdrawalReview(raw);
    expect(confirmWithdrawalReview(review, raw, true)).toBe(raw);
    expect(() => confirmWithdrawalReview(review, raw + " ", true)).toThrow(PluginServiceError);
    expect(() => confirmWithdrawalReview(review, raw, false)).toThrow(PluginServiceError);
    for (const invalid of ["null", "[]", JSON.stringify({ ...withdrawalFixture(), publicKey: "d".repeat(64) }),
      JSON.stringify({ ...withdrawalFixture(), reason: "other" }), JSON.stringify({ ...withdrawalFixture(), signature: "invalid" })]) {
      expect(() => readWithdrawalReview(invalid)).toThrow(PluginServiceError);
    }
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("production local plugin panel and review dialogs", () => {
  beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", {}); });
  afterEach(() => { vi.unstubAllGlobals(); });

  it("renders the embeddable page and rejects installation and execution outside the desktop", async () => {
    await expect(previewPluginInstall("{}")).rejects.toMatchObject({ code: "plugin_desktop_required" });
    await expect(listInstalledPlugins()).rejects.toMatchObject({ code: "plugin_desktop_required" });
    const client = new QueryClient();
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(PluginsPage)));
    expect(html).toContain("本地插件安装、信任配置与执行需要桌面应用");
    expect(html).toContain("导入签名插件包");
    expect(html).toContain("签名撤回通知");
    expect(html).toContain('disabled=""');
    expect(invoke).not.toHaveBeenCalled();
    client.clear();
  });

  it("renders installed client grants and withdrawn immutable history using actual components", () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const client = new QueryClient();
    client.setQueryData(pluginQueryKeys.installed, [installedFixture()]);
    client.setQueryData(pluginQueryKeys.keys, [keyFixture().preview]);
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(PluginsPage)));
    for (const text of ["Approved Reader", "Codex Desktop", "Codex CLI", "已启用", "已停用", "已撤回", "审核授权", "停用全部客户端", "卸载插件", "审核回滚", "吊销信任"]) {
      expect(html).toContain(text);
    }
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>审核回滚<\/button>/);
    expect(invoke).not.toHaveBeenCalled();
    client.clear();
  });

  it("renders independent trust and full install permission review with confirmation initially disabled", () => {
    const key = keyFixture();
    const keyHtml = renderToStaticMarkup(createElement(TrustedKeyDialog, { input: key.input, review: key, busy: false, error: "",
      onChange: vi.fn<(input: TrustedKeyInput) => void>(), onPreview: vi.fn(), onConfirm: vi.fn(), onClose: vi.fn() }));
    expect(keyHtml).toContain("插件包内的公钥不能建立信任");
    expect(keyHtml).toContain(fingerprint);
    expect(keyHtml).toMatch(/<button[^>]*disabled=""[^>]*>确认信任此公钥<\/button>/);
    const preview = installFixture();
    preview.expiresAt = Date.now() + 120_000;
    const html = renderToStaticMarkup(createElement(InstallReviewDialog, { preview, busy: false, error: "", onConfirm: vi.fn(), onClose: vi.fn() }));
    expect(html).toContain("各客户端新增权限");
    expect(html).toContain("package:docs/");
    expect(html).toContain("输入本次插件包的精确摘要");
    expect(html).toContain("默认保持停用");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>确认保存并授权<\/button>/);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("escapes reviewed text and prevents expired tool and unconfirmed withdrawal actions", () => {
    const review = toolFixture();
    review.preview.expiresAt = Date.now() - 1;
    review.draft.argumentsJson = '{"message":"<script>fixture</script>"}';
    const html = renderToStaticMarkup(createElement(ToolReviewDialog, { review, operation: "read_text", busy: false, error: "", onConfirm: vi.fn(), onClose: vi.fn() }));
    expect(html).toContain("&lt;script&gt;fixture&lt;/script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).toContain("预览已过期");
    expect(html).toContain("package:docs/approved.md");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>确认并执行这一次调用<\/button>/);
    const notice = readWithdrawalReview(JSON.stringify(withdrawalFixture()));
    const withdrawalHtml = renderToStaticMarkup(createElement(WithdrawalReviewDialog, { review: notice, busy: false, error: "", onConfirm: vi.fn(), onClose: vi.fn() }));
    expect(withdrawalHtml).toContain("签名将在确认后由宿主验证");
    expect(withdrawalHtml).toMatch(/<button[^>]*disabled=""[^>]*>确认验证并应用撤回<\/button>/);
    expect(invoke).not.toHaveBeenCalled();
  });
});
