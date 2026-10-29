import { describe, expect, it } from "vitest";
import { providerPresets, findProviderPreset } from "./config/providerPresets";
import { presetToProviderDraft } from "./lib/provider-presets";
import { createProviderAdvanced, normalizeProvider } from "./lib/providers";
import { validateProviderAdvanced } from "./lib/provider-advanced";

describe("精简供应商预设", () => {
  it("只保留五个模板，默认顺序固定", () => {
    expect(providerPresets.map(p => p.id)).toEqual(["custom", "openai", "deepseek", "anthropic", "glm"]);
    expect(new Set(providerPresets.map(p => p.id)).size).toBe(5);
  });
  it("Anthropic 默认 Messages，其余默认原生 Responses", () => {
    for (const preset of providerPresets) {
      const draft = presetToProviderDraft(preset);
      expect(draft.advanced.upstreamFormat).toBe(preset.id === "anthropic" ? "anthropic" : "openai_responses");
      expect(draft.providerType).toBe(preset.providerType);
      expect(draft.advanced.codex).toEqual(createProviderAdvanced().codex);
      expect(validateProviderAdvanced(draft.advanced)).toBe("");
    }
  });
  it("ID 冲突时生成唯一 ID", () => {
    expect(presetToProviderDraft(findProviderPreset("anthropic")!, ["Anthropic", "anthropic-2"]).id).toBe("anthropic-3");
  });
  it("兼容旧配置，不覆盖显式上游协议", () => {
    expect(normalizeProvider({ id: "legacy", providerType: "anthropic", advanced: {} }).advanced.upstreamFormat).toBe("anthropic");
    expect(normalizeProvider({ id: "legacy", providerType: "custom", advanced: {} }).advanced.upstreamFormat).toBe("openai_responses");
    expect(normalizeProvider({ id: "legacy", advanced: { upstreamFormat: "openai_chat" } }).advanced.upstreamFormat).toBe("openai_chat");
  });
});
