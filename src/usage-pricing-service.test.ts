import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { deleteModelPricing, listModelPricing, saveModelPricing, syncModelsDevPricing } from "./lib/usage-service";
import type { ModelPricing, ModelPricingInput } from "./types/usage";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
beforeEach(() => { invoke.mockReset(); });
afterEach(() => vi.unstubAllGlobals());
const pricing: ModelPricingInput = { id: "qa-price", inputPerMillion: 2, outputPerMillion: 5, cacheReadPerMillion: 0, cacheCreationPerMillion: null };
const cases = [
  ["list_model_pricing", () => listModelPricing(), undefined],
  ["save_model_pricing", () => saveModelPricing(pricing), { pricing }],
  ["delete_model_pricing", () => deleteModelPricing(pricing.id), { id: pricing.id }],
  ["sync_models_dev_pricing", () => syncModelsDevPricing(), undefined],
] as const;
describe("native pricing service", () => {
  it.each(cases)("%s never pretends to succeed in a browser preview", async (_command, call) => {
    vi.stubGlobal("window", {});
    await expect(call()).rejects.toThrow("浏览器预览不会保存或同步价格");
    expect(invoke).not.toHaveBeenCalled();
  });
  it.each(cases)("%s propagates native failures without returning a fake empty success", async (command, call, args) => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockRejectedValue(new Error("database is locked"));
    await expect(call()).rejects.toThrow("database is locked");
    if (args) expect(invoke).toHaveBeenCalledExactlyOnceWith(command, args);
    else expect(invoke).toHaveBeenCalledExactlyOnceWith(command);
  });
  it("returns the stored catalog and real synchronized count", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const rows: ModelPricing[] = [{ ...pricing, source: "custom", updatedAt: 123 }];
    invoke.mockResolvedValueOnce(rows).mockResolvedValueOnce(17);
    expect(await listModelPricing()).toEqual(rows);
    expect(await syncModelsDevPricing()).toBe(17);
  });
  it("sends only editable fields when adopting an existing catalog row", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockResolvedValue(undefined);
    const catalogRow: ModelPricing = { ...pricing, source: "models.dev", updatedAt: 123 };
    await saveModelPricing(catalogRow);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("save_model_pricing", { pricing });
  });
  it("waits for the native remove operation", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockResolvedValue(undefined);
    await deleteModelPricing("qa-price");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("delete_model_pricing", { id: "qa-price" });
  });
});
