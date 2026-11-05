import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MarketplacePage } from "./features/marketplace/MarketplacePage";
import { MarketplaceClient, MarketplaceServiceError, canonicalMarketplacePackage, inspectMarketplacePrivacy, marketplaceErrorText, marketplacePackageDigest, normalizeMarketplaceOrigin, parseMarketplacePackage, parseStrictMarketplaceJson } from "./lib/marketplace-service";
import type { InstallPreview, InstalledPlugin, PluginPackage, PublishedPlugin } from "./types/plugins";
import type { MarketplaceSubmission } from "./types/marketplace";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const clock = 1_800_000_000_000;
const authorId = "01000000-0000-4000-8000-000000000001";
const submissionId = "02000000-0000-4000-8000-000000000001";
const token = "7a".repeat(32);
const nativeRequestId = "03000000-0000-4000-8000-000000000001";
const clients: MarketplaceClient[] = [];
const emptyPermissions = () => ({ network: [], readPaths: [], writePaths: [], process: false, credentials: [] });
const emptyPage = () => ({ items: [], limit: 20, offset: 0, hasMore: false });
const auth = (overrides: Record<string, unknown> = {}) => ({ token, tokenType: "Bearer", expiresAt: clock / 1000 + 3600,
  user: { userId: authorId, username: "ordinary_author", displayName: "Example Author", reviewer: false }, ...overrides });
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const missing = () => json({ error: { code: "not_found", message: "synthetic private server detail" } }, 404);

function packageFixture(): PluginPackage {
  return { manifest: { schemaVersion: 1, id: "shared-example", name: "Shared example skill", description: "A small reviewed text package",
    author: "Example Author", version: "1.0.0", minimumHostVersion: "1.0.0", kind: "skill", entry: "SKILL.md",
    clients: ["codex-desktop", "claude-desktop", "opencode-desktop"], permissions: emptyPermissions(), dependencies: [] },
    files: { "SKILL.md": "# Shared skill\nSummarize the text selected by the user.\n" } };
}

async function publishedFixture(): Promise<PublishedPlugin> {
  const packageValue = packageFixture();
  return { package: packageValue, digest: await marketplacePackageDigest(packageValue), signature: "a1".repeat(64), signingKeyId: "synthetic-publisher" };
}

async function submissionFixture(packageValue = packageFixture()): Promise<MarketplaceSubmission> {
  return { submissionId, status: "submitted", package: packageValue, digest: await marketplacePackageDigest(packageValue),
    authorDisplayName: packageValue.manifest.author, createdAt: clock / 1000, updatedAt: clock / 1000, reviewNote: null,
    previousPublishedVersion: null, permissionChanges: emptyPermissions() };
}

function previewFixture(published: PublishedPlugin): InstallPreview {
  return { previewId: "synthetic-preview", expiresAt: clock / 1000 + 300, signingKeyId: published.signingKeyId,
    signingKeyFingerprint: "b2".repeat(32), previousVersion: null, permissionChanges: [], validation: { digest: published.digest,
      manifest: published.package.manifest, canonicalSize: canonicalMarketplacePackage(published.package).length,
      fileCount: Object.keys(published.package.files).length, runtimeSupported: true, warnings: [] } };
}

function installedFixture(published: PublishedPlugin): InstalledPlugin {
  return { id: published.package.manifest.id, manifest: published.package.manifest, activeVersion: published.package.manifest.version,
    digest: published.digest, installed: true, enabledClients: [], versions: [], verification: "verified", warnings: [] };
}

function setup(handler: (path: string, init: RequestInit) => Response | Promise<Response>, now = () => clock) {
  const fetcher = vi.fn<typeof fetch>(async (input, init = {}) => {
    const url = new URL(String(input));
    return handler(`${url.pathname}${url.search}`, init);
  });
  const client = new MarketplaceClient("https://market.example.test", { fetch: fetcher, now, requestTimeoutMs: 5000 });
  clients.push(client);
  return { client, fetcher };
}

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>(settle => { resolve = settle; });
  return { promise, resolve };
}

beforeEach(() => {
  invoke.mockReset();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
});
afterEach(() => {
  for (const client of clients.splice(0)) client.dispose();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("market transport and memory identity", () => {
  it("accepts HTTPS and explicit exact loopback HTTP only", () => {
    expect(normalizeMarketplaceOrigin("https://market.example.test/")).toBe("https://market.example.test");
    for (const host of ["localhost", "127.0.0.1", "[::1]"]) {
      expect(normalizeMarketplaceOrigin(`http://${host}:47831`, true)).toBe(`http://${host}:47831`);
      expect(() => normalizeMarketplaceOrigin(`http://${host}:47831`)).toThrow(MarketplaceServiceError);
    }
    for (const origin of ["http://market.example.test", "http://127.1", "http://2130706433", "http://0x7f000001", "http://localhost.example.test",
      "https://user:password@market.example.test", "https://market.example.test/v1", "https://market.example.test?secret=value", "https://market.example.test#fragment", "https://market.example.test\\evil"]) {
      expect(() => normalizeMarketplaceOrigin(origin, true)).toThrow(MarketplaceServiceError);
    }
  });

  it("uses private in-memory bearer state and never authenticates public browsing", async () => {
    const storage = { setItem: vi.fn(), getItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal("localStorage", storage); vi.stubGlobal("sessionStorage", storage);
    const { client, fetcher } = setup(path => path === "/v1/auth/login" ? json(auth()) : path.endsWith("/withdrawal") ? missing() : json(emptyPage()));
    const listener = vi.fn(); client.subscribe(listener);
    const session = await client.login("ordinary_author", "synthetic market password");
    expect(JSON.stringify(client)).not.toContain(token);
    expect(JSON.stringify(session)).not.toContain(token);
    expect(Object.isFrozen(session.user)).toBe(true);
    await client.catalog(); await client.versions("shared-example"); await client.withdrawal("shared-example", "1.0.0");
    for (const [, init] of fetcher.mock.calls.slice(1)) {
      expect(new Headers(init?.headers).has("authorization")).toBe(false);
      expect(init).toMatchObject({ credentials: "omit", redirect: "error", cache: "no-store", referrerPolicy: "no-referrer" });
    }
    await client.submissions();
    expect(new Headers(fetcher.mock.calls.at(-1)?.[1]?.headers).get("authorization")).toBe(`Bearer ${token}`);
    expect(storage.setItem).not.toHaveBeenCalled();
    await client.logout();
    expect(client.session).toBeNull();
    expect(listener).toHaveBeenLastCalledWith(null);
  });

  it("blocks anonymous drafts, submissions, audit and review locally", async () => {
    const { client, fetcher } = setup(() => json(emptyPage()));
    const operations: Array<() => Promise<unknown>> = [() => client.submissions(), () => client.submission(submissionId), () => client.reviewQueue(),
      () => client.submissionAudit(submissionId), () => client.reviewAudit(), () => client.reports()];
    for (const operation of operations) {
      await expect(Promise.resolve().then(operation)).rejects.toMatchObject({ code: "authentication_required" });
    }
    await expect(client.submit(packageFixture())).rejects.toMatchObject({ code: "authentication_required" });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("clears identity on expiry and 401 without showing server secrets", async () => {
    let currentTime = clock;
    const { client } = setup(path => path === "/v1/auth/login" ? json(auth()) : json({ error: { code: "forbidden", message: "Bearer synthetic-response-secret" } }, 401), () => currentTime);
    await client.login("ordinary_author", "synthetic market password");
    await expect(client.submissions()).rejects.toMatchObject({ code: "authentication_required" });
    expect(client.session).toBeNull();
    await client.login("ordinary_author", "synthetic market password");
    currentTime += 3_600_001;
    expect(client.session).toBeNull();
    await expect(client.submit(packageFixture())).rejects.toMatchObject({ code: "authentication_required" });
    expect(marketplaceErrorText(new Error("Bearer synthetic-response-secret"))).not.toContain("synthetic-response-secret");
  });

  it("cannot restore a late login after logout or disposal", async () => {
    for (const action of ["logout", "dispose"] as const) {
      const pending = deferred<Response>();
      const { client } = setup(() => pending.promise);
      const login = client.login("ordinary_author", "synthetic market password");
      const assertion = expect(login).rejects.toMatchObject({ code: "session_changed" });
      if (action === "logout") await client.logout(); else client.dispose();
      pending.resolve(json(auth()));
      await assertion;
      expect(client.session).toBeNull();
    }
  });
});

describe("default desktop market transport", () => {
  const nativeResponse = (body: unknown, status = 200) => ({ status, contentType: status === 204 ? null : "application/json",
    body: status === 204 ? "" : JSON.stringify(body) });
  const desktopClient = (requestTimeoutMs = 5000) => {
    const client = new MarketplaceClient("https://market.example.test/", { now: () => clock, requestTimeoutMs });
    clients.push(client);
    return client;
  };

  it("uses native IPC for public browsing, authenticated calls, and logout", async () => {
    const browserFetch = vi.fn(); vi.stubGlobal("fetch", browserFetch);
    invoke.mockImplementation(async (command, argumentsValue) => {
      if (command === "marketplace_begin_request") return nativeRequestId;
      if (command === "marketplace_cancel_request") return;
      const path = argumentsValue.request.path;
      return path === "/v1/auth/login" ? nativeResponse(auth()) : path === "/v1/auth/logout" ? nativeResponse(null, 204) : nativeResponse(emptyPage());
    });
    const client = desktopClient();
    await client.catalog();
    await client.login("ordinary_author", "synthetic market password");
    await client.submissions();
    await client.logout();
    const beginnings = invoke.mock.calls.filter(([command]) => command === "marketplace_begin_request");
    expect(beginnings).toHaveLength(4);
    expect(beginnings[0][1]).toEqual({ options: { origin: "https://market.example.test", allowDevelopmentHttp: false, timeoutMs: 5000 } });
    const requests = invoke.mock.calls.filter(([command]) => command === "marketplace_request").map(([, argumentsValue]) => argumentsValue);
    expect(requests[0]).toMatchObject({ requestId: nativeRequestId, request: { method: "GET", body: null, bearer: null } });
    expect(requests[1].request).toMatchObject({ method: "POST", bearer: null });
    expect(JSON.parse(requests[1].request.body)).toMatchObject({ username: "ordinary_author", password: "synthetic market password" });
    expect(requests[2].request).toMatchObject({ method: "GET", bearer: token });
    expect(requests[3].request).toMatchObject({ method: "POST", bearer: token });
    expect(client.session).toBeNull();
    expect(browserFetch).not.toHaveBeenCalled();
  });

  it("returns immediately on disposal during begin and cancels the late ID without dispatch", async () => {
    const registration = deferred<unknown>();
    invoke.mockImplementation((command) => command === "marketplace_begin_request" ? registration.promise : Promise.resolve());
    const client = desktopClient();
    const login = client.login("ordinary_author", "synthetic market password");
    const assertion = expect(login).rejects.toMatchObject({ code: "session_changed" });
    client.dispose();
    await assertion;
    expect(invoke.mock.calls.some(([command]) => command === "marketplace_request")).toBe(false);
    registration.resolve(nativeRequestId);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("marketplace_cancel_request", { requestId: nativeRequestId }));
    expect(invoke.mock.calls.filter(([command]) => command === "marketplace_cancel_request")).toHaveLength(1);
    expect(invoke.mock.calls.some(([command]) => command === "marketplace_request")).toBe(false);
    expect(client.session).toBeNull();
  });

  it("cancels an active native request and ignores its late authentication result", async () => {
    const reply = deferred<unknown>();
    invoke.mockImplementation((command) => command === "marketplace_begin_request" ? Promise.resolve(nativeRequestId)
      : command === "marketplace_request" ? reply.promise : Promise.resolve());
    const client = desktopClient();
    const login = client.login("ordinary_author", "synthetic market password");
    const assertion = expect(login).rejects.toMatchObject({ code: "session_changed" });
    await vi.waitFor(() => expect(invoke.mock.calls.some(([command]) => command === "marketplace_request")).toBe(true));
    client.dispose();
    await assertion;
    reply.resolve(nativeResponse(auth()));
    await Promise.resolve();
    expect(invoke.mock.calls.filter(([command]) => command === "marketplace_cancel_request")).toEqual([
      ["marketplace_cancel_request", { requestId: nativeRequestId }],
    ]);
    expect(client.session).toBeNull();
  });

  it("applies the frontend deadline while native registration is pending", async () => {
    vi.useFakeTimers();
    const registration = deferred<unknown>();
    invoke.mockImplementation((command) => command === "marketplace_begin_request" ? registration.promise : Promise.resolve());
    const client = desktopClient(100);
    const request = client.catalog();
    const assertion = expect(request).rejects.toMatchObject({ code: "network" });
    await vi.advanceTimersByTimeAsync(100);
    await assertion;
    registration.resolve(nativeRequestId);
    await Promise.resolve(); await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith("marketplace_cancel_request", { requestId: nativeRequestId });
    expect(invoke.mock.calls.some(([command]) => command === "marketplace_request")).toBe(false);
  });

  it("rejects malformed native responses and cleans up only safe request IDs", async () => {
    for (const reply of [null, { ...nativeResponse(emptyPage()), status: 302 }, { ...nativeResponse(emptyPage()), contentType: "text/html" },
      { ...nativeResponse(emptyPage()), privateDetail: "PRIVATE_DETAIL" }, { status: 204, contentType: null, body: "PRIVATE_BODY" },
      { status: 200, contentType: "application/json", body: "x".repeat(2 * 1024 * 1024 + 64 * 1024 + 1) }]) {
      invoke.mockImplementation(async command => command === "marketplace_begin_request" ? nativeRequestId : reply);
      await expect(desktopClient().catalog()).rejects.toMatchObject({ code: "invalid_response", message: new MarketplaceServiceError("invalid_response").message });
    }
    invoke.mockReset();
    invoke.mockResolvedValue("PRIVATE_INVALID_ID");
    await expect(desktopClient().catalog()).rejects.toMatchObject({ code: "invalid_response" });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(["marketplace_begin_request"]);
  });

  it("preserves safe native codes and never exposes raw transport errors", async () => {
    for (const [nativeError, code] of [["invalid_response", "invalid_response"], ["rate_limited", "rate_limited"], ["PRIVATE_TOKEN https://private.test", "network"]]) {
      invoke.mockImplementation(async command => {
        if (command === "marketplace_begin_request") return nativeRequestId;
        if (command === "marketplace_cancel_request") return;
        throw nativeError;
      });
      await expect(desktopClient().catalog()).rejects.toMatchObject({ code, message: new MarketplaceServiceError(code).message });
    }
    expect(invoke).toHaveBeenCalledWith("marketplace_cancel_request", { requestId: nativeRequestId });
  });

  it("retains browser fetch outside the desktop runtime", async () => {
    vi.stubGlobal("window", {});
    const browserFetch = vi.fn().mockResolvedValue(json(emptyPage()));
    vi.stubGlobal("fetch", browserFetch);
    await expect(desktopClient().catalog()).resolves.toMatchObject(emptyPage());
    expect(browserFetch).toHaveBeenCalledOnce();
    expect(browserFetch.mock.calls[0][1]).toMatchObject({ credentials: "omit", redirect: "error", referrerPolicy: "no-referrer" });
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("package privacy and typed digest", () => {
  it("accepts public multiclient tool metadata regardless of JSON field order", () => {
    const value = packageFixture();
    value.manifest.id = "qa.reviewed-echo";
    value.manifest.name = "QA reviewed echo";
    value.manifest.description = "A reviewed local echo tool.";
    value.manifest.author = "QA Plugin Author";
    value.manifest.kind = "mcp";
    value.manifest.entry = "tools.json";
    value.manifest.clients = ["codex-desktop", "codex-cli", "claude-desktop", "claude-code", "opencode-desktop", "opencode-cli", "gemini-cli"];
    value.files = { "tools.json": JSON.stringify({ schemaVersion: 1, tools: [{ name: "echo", description: "Return reviewed JSON.",
      inputSchema: { type: "object", properties: { message: { type: "string", maxLength: 1024 } }, required: ["message"], additionalProperties: false },
      operation: { type: "json_passthrough" } }] }) };
    const sorted = (node: unknown): unknown => Array.isArray(node) ? node.map(sorted) : node && typeof node === "object"
      ? Object.fromEntries(Object.entries(node).sort(([first], [second]) => first.localeCompare(second)).map(([key, child]) => [key, sorted(child)])) : node;
    expect(parseMarketplacePackage(JSON.stringify(value))).toEqual(value);
    expect(parseMarketplacePackage(JSON.stringify(sorted(value)))).toEqual(value);
    expect(() => inspectMarketplacePrivacy({ manifest: value.manifest })).not.toThrow();
  });

  it("retains rejection of opaque encoding and secrets reconstructed from fragments", () => {
    const value = packageFixture();
    value.files["SKILL.md"] = "Q".repeat(128);
    expect(() => parseMarketplacePackage(JSON.stringify(value))).toThrow(MarketplaceServiceError);
    expect(() => inspectMarketplacePrivacy(["c2stcHJv", "ai1zeW50aGV0aWNwcml2YXRldmFsdWUxMjM0NQ=="])).toThrow(MarketplaceServiceError);
    expect(() => inspectMarketplacePrivacy(["qa-current-", "market-bearer"], ["qa-current-market-bearer"])).toThrow(MarketplaceServiceError);
    value.files["SKILL.md"] = JSON.stringify(["qa-current-", "market-bearer"]);
    expect(() => inspectMarketplacePrivacy(value, ["qa-current-market-bearer"])).toThrow(MarketplaceServiceError);
  });

  it("inspects every manifest field, permission field, file name and file body before upload", () => {
    const secret = "sk-proj-syntheticprivatevalue12345";
    const placements: Array<(value: PluginPackage) => void> = [
      ...(["id", "name", "description", "author", "version", "minimumHostVersion", "entry"] as const)
        .map(field => (value: PluginPackage) => { value.manifest[field] = secret; }),
      value => { value.manifest.clients = [secret as PluginPackage["manifest"]["clients"][number]]; },
      value => { value.manifest.dependencies = [secret]; },
      ...(["network", "readPaths", "writePaths", "credentials"] as const)
        .map(field => (value: PluginPackage) => { value.manifest.permissions[field] = [secret]; }),
      value => { value.files[`notes-${secret}.txt`] = "Public text"; },
      value => { value.files["SKILL.md"] = secret; },
    ];
    for (const place of placements) {
      const value = packageFixture(); place(value);
      expect(() => parseMarketplacePackage(JSON.stringify(value))).toThrow(marketplaceErrorText(new MarketplaceServiceError("privacy_rejected")));
    }
    for (const file of [".env", "credentials.json", "auth.json", "key.pem", "image.png", "binary.wasm"]) {
      const value = packageFixture(); value.files[file] = "Synthetic private material";
      expect(() => parseMarketplacePackage(JSON.stringify(value))).toThrow(MarketplaceServiceError);
    }
  });

  it("blocks nested, escaped, hex and base64 private content without echoing it", () => {
    const secret = "Bearer synthetic_private_token";
    const encodedHex = Array.from(new TextEncoder().encode(secret), byte => byte.toString(16).padStart(2, "0")).join("");
    const escaped = Array.from(secret, character => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`).join("");
    const percent = Array.from(new TextEncoder().encode(secret), byte => `%${byte.toString(16).padStart(2, "0")}`).join("");
    for (const content of [JSON.stringify({ credentials: { password: "synthetic private phrase" } }), btoa(secret), encodedHex, escaped, percent,
      "C:/Users/SyntheticOwner/private.txt", "synthetic-person@example.test", "api_key = 'synthetic-private-value'"]) {
      const value = packageFixture(); value.files["SKILL.md"] = content;
      try { parseMarketplacePackage(JSON.stringify(value)); expect.fail("Private content was accepted"); }
      catch (error) {
        expect(error).toBeInstanceOf(MarketplaceServiceError);
        expect((error as MarketplaceServiceError).code).toBe("privacy_rejected");
        expect(marketplaceErrorText(error)).not.toContain(content);
      }
    }
    expect(() => inspectMarketplacePrivacy({ setting: "${EXAMPLE_VALUE}" })).not.toThrow();
  });

  it("blocks current bearer and internal identity even without a recognizable secret prefix", async () => {
    const { client, fetcher } = setup(() => json(auth()));
    await client.login("ordinary_author", "synthetic market password");
    for (const privateValue of [token, authorId]) {
      const value = packageFixture(); value.manifest.description = privateValue;
      await expect(client.submit(value)).rejects.toMatchObject({ code: "privacy_rejected" });
    }
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("rejects duplicate and unknown fields while retaining pinned dependency versions", () => {
    expect(() => parseStrictMarketplaceJson('{"manifest":{},"manifest":{},"files":{}}')).toThrow(MarketplaceServiceError);
    expect(() => parseStrictMarketplaceJson('{"files":{"__proto__":"value"}}')).toThrow(MarketplaceServiceError);
    expect(() => parseStrictMarketplaceJson('{"files":{"\\u005f_proto__":"value"}}')).toThrow(MarketplaceServiceError);
    expect(() => parseStrictMarketplaceJson('{"files":{"constructor":"value"}}')).toThrow(MarketplaceServiceError);
    expect(() => parseStrictMarketplaceJson('{"files":{"prototype":"value"}}')).toThrow(MarketplaceServiceError);
    const value = packageFixture(); value.manifest.dependencies = ["another-plugin@1.2.0", "prerelease-plugin@2.0.0-beta.1+build7"];
    expect(parseMarketplacePackage(JSON.stringify(value)).manifest.dependencies).toEqual(value.manifest.dependencies);
    expect(() => inspectMarketplacePrivacy(value, ["another-plugin@1.2.0"])).toThrow(MarketplaceServiceError);
    expect(() => parseMarketplacePackage(JSON.stringify({ ...value, role: "reviewer" }))).toThrow(MarketplaceServiceError);
    value.manifest.dependencies = ["another-plugin"];
    expect(() => parseMarketplacePackage(JSON.stringify(value))).toThrow(MarketplaceServiceError);
  });

  it("matches Rust typed canonical SHA256 including numeric file-name ordering", async () => {
    const value = packageFixture(); value.files["2"] = "Second file\n"; value.files["10"] = "Tenth file\n";
    const canonical = canonicalMarketplacePackage(value);
    expect(canonical.indexOf('"10":')).toBeLessThan(canonical.indexOf('"2":'));
    expect(await marketplacePackageDigest(value)).toBe("c93097a70607461313e490b6521af27c162b3ea730fcc5328cdc1a5b3e485145");
    const reordered = { files: value.files, manifest: Object.fromEntries(Object.entries(value.manifest).reverse()) } as PluginPackage;
    expect(canonicalMarketplacePackage(reordered)).toBe(canonical);
  });
});

describe("REST publication checks", () => {
  it("requires a fresh submitted state for every upload and sends the replacement link", async () => {
    let uploaded = packageFixture();
    const { client, fetcher } = setup(async (path, init) => {
      if (path === "/v1/auth/login") return json(auth());
      uploaded = JSON.parse(String(init.body)).package as PluginPackage;
      return json(await submissionFixture(uploaded), 201);
    });
    await client.login("ordinary_author", "synthetic market password");
    expect((await client.submit(packageFixture())).status).toBe("submitted");
    const update = packageFixture(); update.manifest.version = "1.1.0";
    expect((await client.submit(update, submissionId)).status).toBe("submitted");
    expect(JSON.parse(String(fetcher.mock.calls.at(-1)?.[1]?.body))).toEqual({ package: update, replacesSubmissionId: submissionId });
  });

  it("fails closed on swapped digest, extra public identity and malformed response", async () => {
    const release = await publishedFixture();
    const { client } = setup(() => json({ ...release, digest: "00".repeat(32) }));
    await expect(client.download("shared-example", "1.0.0")).rejects.toMatchObject({ code: "invalid_response" });
    const extra = setup(() => json({ ...emptyPage(), username: "hidden_internal_identity" })).client;
    await expect(extra.catalog()).rejects.toMatchObject({ code: "invalid_response" });
    const malformed = setup(() => new Response("synthetic raw secret", { headers: { "Content-Type": "text/plain" } })).client;
    await expect(malformed.catalog()).rejects.toMatchObject({ code: "invalid_response" });
  });
});

describe("independent local signature trust and installation", () => {
  it("requires an independently trusted key and never configures trust from a service response", async () => {
    const release = await publishedFixture();
    const { client } = setup(() => json(release));
    invoke.mockRejectedValue({ code: "plugin_untrusted_key", detail: "synthetic native private detail" });
    await expect(client.prepareInstall("shared-example", "1.0.0")).rejects.toMatchObject({ code: "untrusted_key" });
    expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install"]);
  });

  it("binds explicit confirmation, exact permissions and allowed clients to the original preview", async () => {
    const release = await publishedFixture();
    const { client } = setup(path => path.endsWith("/withdrawal") ? missing() : json(release));
    invoke.mockImplementation(async command => command === "plugins_preview_install" ? previewFixture(release) : installedFixture(release));
    const plan = await client.prepareInstall("shared-example", "1.0.0");
    expect(Object.isFrozen(plan.published.package.manifest.permissions)).toBe(true);
    await expect(client.confirmInstall({ ...plan }, { clients: ["codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: "confirmation" });
    await expect(client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: false })).rejects.toMatchObject({ code: "confirmation" });
    await expect(client.confirmInstall(plan, { clients: ["gemini-cli"], confirmed: true })).rejects.toMatchObject({ code: "confirmation" });
    await expect(client.confirmInstall(plan, { clients: ["codex-desktop", "codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: "confirmation" });
    expect((await client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true })).digest).toBe(release.digest);
    const call = invoke.mock.calls.find(entry => entry[0] === "plugins_confirm_install");
    expect(JSON.parse(call?.[1].confirmationJson as string)).toEqual({ previewId: plan.preview.previewId, digest: release.digest,
      acceptedPermissions: release.package.manifest.permissions, clients: ["codex-desktop"], confirmed: true, enable: false });
    await expect(client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: "confirmation" });
    expect(invoke.mock.calls.filter(entry => entry[0] === "plugins_confirm_install")).toHaveLength(1);
  });

  it("consumes the preview before asynchronous checks so concurrent confirmation cannot install twice", async () => {
    const release = await publishedFixture();
    const gate = deferred<Response>();
    let noticeReads = 0;
    const { client } = setup(path => path.endsWith("/withdrawal") ? (++noticeReads === 1 ? gate.promise : missing()) : json(release));
    invoke.mockImplementation(async command => command === "plugins_preview_install" ? previewFixture(release) : installedFixture(release));
    const plan = await client.prepareInstall("shared-example", "1.0.0");
    const first = client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true });
    await expect(client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: "confirmation" });
    gate.resolve(missing());
    await first;
    expect(invoke.mock.calls.filter(entry => entry[0] === "plugins_confirm_install")).toHaveLength(1);
  });

  it("stops installation when the exact published signature changes after preview", async () => {
    const release = await publishedFixture();
    let reads = 0;
    const { client } = setup(path => path.endsWith("/withdrawal") ? missing() : json({ ...release, signature: ++reads === 1 ? release.signature : "b1".repeat(64) }));
    invoke.mockResolvedValue(previewFixture(release));
    const plan = await client.prepareInstall("shared-example", "1.0.0");
    await expect(client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: "conflict" });
    expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install"]);
  });

  it("blocks a withdrawn version and requires the notice to match the preview digest", async () => {
    const release = await publishedFixture();
    for (const mismatch of [false, true]) {
      invoke.mockReset();
      const notice = { id: "shared-example", version: "1.0.0", digest: mismatch ? "00".repeat(32) : release.digest,
        reason: "privacy", signingKeyId: release.signingKeyId, signature: "c1".repeat(64) };
      const { client } = setup(path => path.endsWith("/withdrawal") ? json(notice) : json(release));
      invoke.mockImplementation(async command => command === "plugins_preview_install" ? previewFixture(release) : installedFixture(release));
      const plan = await client.prepareInstall("shared-example", "1.0.0");
      await expect(client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: mismatch ? "invalid_response" : "withdrawn" });
      expect(invoke.mock.calls.filter(call => call[0] === "plugins_confirm_install")).toHaveLength(0);
      expect(invoke.mock.calls.filter(call => call[0] === "plugins_apply_withdrawal")).toHaveLength(mismatch ? 0 : 1);
    }
  });

  it("applies a verified withdrawal discovered immediately after native installation", async () => {
    const release = await publishedFixture();
    let withdrawn = false;
    const notice = { id: "shared-example", version: "1.0.0", digest: release.digest, reason: "security",
      signingKeyId: release.signingKeyId, signature: "d1".repeat(64) };
    const { client } = setup(path => path.endsWith("/withdrawal") ? (withdrawn ? json(notice) : missing()) : json(release));
    invoke.mockImplementation(async command => {
      if (command === "plugins_preview_install") return previewFixture(release);
      if (command === "plugins_confirm_install") withdrawn = true;
      return installedFixture(release);
    });
    const plan = await client.prepareInstall("shared-example", "1.0.0");
    await expect(client.confirmInstall(plan, { clients: ["codex-desktop"], confirmed: true })).rejects.toMatchObject({ code: "withdrawn" });
    expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install", "plugins_confirm_install", "plugins_apply_withdrawal"]);
    expect(JSON.parse(invoke.mock.calls.at(-1)?.[1].withdrawalJson as string)).toEqual(notice);
  });
});

describe("market page entry", () => {
  it("renders without required props and keeps draft and review sections private while logged out", () => {
    vi.stubGlobal("navigator", { languages: ["zh-CN"] });
    const markup = renderToStaticMarkup(<MarketplacePage />);
    expect(markup).toContain("公开已发布插件");
    expect(markup).toContain("独立市场身份");
    expect(markup).toContain('type="password"');
    expect(markup).not.toContain("我的审核状态");
    expect(markup).not.toContain("独立审核队列");
    expect(markup).not.toContain("投稿包全文");
    expect(invoke).not.toHaveBeenCalled();
  });
});
