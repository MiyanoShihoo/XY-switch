import { describe, expect, it } from "vitest";
import { duplicateProvider, reorderProviders } from "./lib/provider-actions";
import { createProviderAdvanced, DEFAULT_PROVIDER_ICON } from "./lib/providers";
import type { Provider } from "./types/provider";

function provider(id: string, sortOrder: number): Provider {
  return {
    id,
    name: id,
    group: "Codex",
    endpoint: "https://example.com/v1",
    model: "model",
    enabled: true,
    icon: "/source.svg",
    builtIn: true,
    providerType: "openai",
    sortOrder,
    advanced: createProviderAdvanced("openai_responses"),
  };
}

describe("provider actions", () => {
  it("reorders in both directions and normalizes sort order", () => {
    const providers = [provider("a", 20), provider("b", 30), provider("c", 40)];
    expect(reorderProviders(providers, "a", "c").map((item) => item.id)).toEqual(["b", "c", "a"]);
    const upwards = reorderProviders(providers, "c", "a");
    expect(upwards.map((item) => item.id)).toEqual(["c", "a", "b"]);
    expect(upwards.map((item) => item.sortOrder)).toEqual([10, 20, 30]);
  });

  it("duplicates configuration with a unique custom identity and independent advanced data", () => {
    const source = provider("openai", 10);
    source.advanced.modelMappings.push({ model: "m1", displayName: "M1", contextWindow: 1000 });
    const first = duplicateProvider(source, [source]);
    const second = duplicateProvider(source, [source, first]);
    expect(first.id).toBe("openai-copy");
    expect(second.id).toBe("openai-copy-2");
    expect(first.builtIn).toBe(false);
    expect(first.enabled).toBe(false);
    expect(second.enabled).toBe(false);
    expect(source.enabled).toBe(true);
    expect(first.icon).toBe(DEFAULT_PROVIDER_ICON);
    first.advanced.modelMappings[0].model = "changed";
    expect(source.advanced.modelMappings[0].model).toBe("m1");
  });
});
