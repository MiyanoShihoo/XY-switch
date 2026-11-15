import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { confirmInstallReview } from "./features/plugins/review";
import { MarketplaceClient, canonicalMarketplacePackage } from "./lib/marketplace-service";
import type { InstallPreview, InstalledPlugin, PluginClient, PluginPackage, PublishedPlugin } from "./types/plugins";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const initialTime = 1_800_000_000_000;
const clients: MarketplaceClient[] = [];

function setup(expiresAt = initialTime + 300_000) {
  const clock = { now: initialTime };
  const packageValue: PluginPackage = {
    manifest: { schemaVersion: 1, id: "preview-clock", name: "Preview clock", description: "Synthetic install preview",
      author: "Synthetic publisher", version: "1.2.3", minimumHostVersion: "1.0.0", kind: "skill", entry: "SKILL.md",
      clients: ["codex-desktop", "claude-desktop"],
      permissions: { network: ["https://docs.example.test"], readPaths: ["notes.txt"], writePaths: ["output.txt"],
        process: true, credentials: ["synthetic-reference"] }, dependencies: [] },
    files: { "SKILL.md": "# Preview clock\nSynthetic package.\n" },
  };
  const published: PublishedPlugin = { package: packageValue, digest: "a1".repeat(32), signature: "b2".repeat(64), signingKeyId: "synthetic-publisher" };
  const preview: InstallPreview = { previewId: "synthetic-preview", expiresAt, signingKeyId: published.signingKeyId,
    signingKeyFingerprint: "c3".repeat(32), previousVersion: null, permissionChanges: [],
    validation: { digest: published.digest, manifest: packageValue.manifest, canonicalSize: canonicalMarketplacePackage(packageValue).length,
      fileCount: 1, runtimeSupported: false, warnings: [] } };
  const installed: InstalledPlugin = { id: packageValue.manifest.id, manifest: packageValue.manifest, activeVersion: packageValue.manifest.version,
    digest: published.digest, installed: true, enabledClients: [], versions: [], verification: "verified", warnings: [] };
  const fetcher = vi.fn<typeof fetch>(() => { throw new Error("Unexpected network request"); });
  const client = new MarketplaceClient("https://market.example.test", { fetch: fetcher, now: () => clock.now });
  clients.push(client);
  const download = vi.spyOn(client, "download").mockResolvedValue(published);
  const withdrawal = vi.spyOn(client, "withdrawal").mockResolvedValue(null);
  invoke.mockImplementation(async (command: string) => {
    if (command === "plugins_preview_install") return preview;
    if (command === "plugins_confirm_install") return installed;
    throw new Error("Unexpected native command");
  });
  const choices = { digest: published.digest, clients: ["codex-desktop"] as PluginClient[], accepted: true, enable: false };
  return { client, clock, published, preview, installed, fetcher, download, withdrawal, choices };
}

beforeEach(() => {
  invoke.mockReset();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.stubGlobal("fetch", vi.fn(() => { throw new Error("Unexpected network request"); }));
});

afterEach(() => {
  for (const client of clients.splice(0)) client.dispose();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("marketplace install preview timestamps in milliseconds", () => {
  it.each([300_000, 1])("keeps a preview valid with %i ms remaining through preparation, review and installation", async remaining => {
    const { client, clock, published, preview, installed, fetcher, download, choices } = setup(initialTime + remaining);
    const plan = await client.prepareInstall("preview-clock", "1.2.3");
    expect(plan.preview.expiresAt).toBe(initialTime + remaining);
    expect(plan.preview.signingKeyFingerprint).toBe(preview.signingKeyFingerprint);
    expect(Object.isFrozen(plan.preview)).toBe(true);
    expect(Object.isFrozen(plan.published.package.manifest.permissions)).toBe(true);
    const confirmation = confirmInstallReview(plan.preview, choices, clock.now);
    await expect(client.confirmInstall(plan, { clients: confirmation.clients, confirmed: confirmation.confirmed })).resolves.toEqual(installed);
    expect(download.mock.calls).toEqual([["preview-clock", "1.2.3"], ["preview-clock", "1.2.3"]]);
    expect(invoke).toHaveBeenNthCalledWith(1, "plugins_preview_install", { publishedJson: JSON.stringify(published) });
    expect(JSON.parse(invoke.mock.calls[1][1].confirmationJson as string)).toEqual({ previewId: preview.previewId, digest: published.digest,
      acceptedPermissions: published.package.manifest.permissions, clients: ["codex-desktop"], confirmed: true, enable: false });
    expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install", "plugins_confirm_install"]);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it.each([
    { state: "already expired", expiresAt: initialTime - 1 },
    { state: "at the exact deadline", expiresAt: initialTime },
    { state: "incorrectly expressed in seconds", expiresAt: initialTime / 1000 + 300 },
  ])("rejects a preview that is $state during preparation", async ({ expiresAt }) => {
    const { client, fetcher, withdrawal } = setup(expiresAt);
    await expect(client.prepareInstall("preview-clock", "1.2.3")).rejects.toMatchObject({ code: "invalid_response" });
    expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install"]);
    expect(withdrawal).not.toHaveBeenCalled();
    expect(fetcher).not.toHaveBeenCalled();
  });

  it.each([0, 1])("rejects confirmation %i ms after the deadline before asynchronous rechecks", async elapsed => {
    const { client, clock, preview, download, withdrawal, choices } = setup();
    const plan = await client.prepareInstall("preview-clock", "1.2.3");
    clock.now = preview.expiresAt + elapsed;
    expect(() => confirmInstallReview(plan.preview, choices, clock.now)).toThrow("过期");
    await expect(client.confirmInstall(plan, { clients: choices.clients, confirmed: true })).rejects.toMatchObject({ code: "confirmation" });
    expect(download).toHaveBeenCalledTimes(1);
    expect(withdrawal).not.toHaveBeenCalled();
    expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install"]);
  });

  it.each([-1, 0, 1])("rechecks expiry at %i ms relative to the deadline after the last asynchronous verification", async elapsed => {
    const { client, clock, preview, installed, download, withdrawal, choices } = setup();
    const plan = await client.prepareInstall("preview-clock", "1.2.3");
    withdrawal.mockResolvedValueOnce(null).mockImplementationOnce(async () => {
      clock.now = preview.expiresAt + elapsed;
      return null;
    });
    const result = client.confirmInstall(plan, { clients: choices.clients, confirmed: true });
    if (elapsed < 0) {
      await expect(result).resolves.toEqual(installed);
      expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install", "plugins_confirm_install"]);
      expect(withdrawal).toHaveBeenCalledTimes(3);
    } else {
      await expect(result).rejects.toMatchObject({ code: "confirmation" });
      expect(invoke.mock.calls.map(call => call[0])).toEqual(["plugins_preview_install"]);
      expect(withdrawal).toHaveBeenCalledTimes(2);
    }
    expect(download.mock.calls).toEqual([["preview-clock", "1.2.3"], ["preview-clock", "1.2.3"]]);
  });
});
