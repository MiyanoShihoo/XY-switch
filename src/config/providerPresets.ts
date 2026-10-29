import type { UpstreamFormat } from "../types/provider";

/** Only connection templates; client protocols are independent of provider selection. */
export interface ProviderPreset {
  id: string;
  name: string;
  providerType: string;
  endpoint: string;
  model: string;
  upstreamFormat: UpstreamFormat;
}

export const providerPresets: ProviderPreset[] = [
  { id: "custom", name: "自定义配置", providerType: "custom", endpoint: "https://", model: "", upstreamFormat: "openai_responses" },
  { id: "openai", name: "OpenAI", providerType: "openai", endpoint: "https://api.openai.com/v1", model: "gpt-5", upstreamFormat: "openai_responses" },
  { id: "deepseek", name: "DeepSeek", providerType: "openai_compatible", endpoint: "https://api.deepseek.com", model: "deepseek-chat", upstreamFormat: "openai_responses" },
  { id: "anthropic", name: "Anthropic", providerType: "anthropic", endpoint: "https://api.anthropic.com", model: "claude-sonnet", upstreamFormat: "anthropic" },
  { id: "glm", name: "GLM", providerType: "openai_compatible", endpoint: "https://open.bigmodel.cn/api/v1", model: "glm-4", upstreamFormat: "openai_responses" },
];

export function findProviderPreset(id: string): ProviderPreset | undefined {
  return providerPresets.find(preset => preset.id === id);
}
