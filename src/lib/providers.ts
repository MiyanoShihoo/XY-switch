import type { Provider, ProviderAdvancedConfig, UpstreamFormat } from "../types/provider";

export const DEFAULT_PROVIDER_ICON = "/app-icon.png";

export function createProviderAdvanced(upstreamFormat: UpstreamFormat = "openai_responses"): ProviderAdvancedConfig {
  return {
    upstreamFormat,
    modelMappings: [],
    customUserAgent: "",
    headerOverrides: {},
    bodyOverrides: {},
    codex: {
      remoteCompression: false,
      applyCommonConfig: false,
      context1m: false,
      compactionThreshold: 900000,
      configToml: "",
    },
    claude: {
      disableArtifactTool: false,
    },
  };
}

export const providers: Provider[] = [
  { id: "anthropic", name: "Anthropic", group: "Claude", endpoint: "https://api.anthropic.com", model: "claude-sonnet", enabled: true, latency: 186, icon: "/providers/anthropic.svg", builtIn: true, providerType: "anthropic", advanced: createProviderAdvanced("anthropic") },
  { id: "openai", name: "OpenAI", group: "Codex", endpoint: "https://api.openai.com/v1", model: "gpt-5", enabled: true, latency: 214, icon: "/providers/openai.svg", builtIn: true, providerType: "openai", advanced: createProviderAdvanced("openai_responses") },
  { id: "deepseek", name: "DeepSeek", group: "Codex", endpoint: "https://api.deepseek.com", model: "deepseek-chat", enabled: false, icon: "/providers/deepseek.svg", builtIn: true, providerType: "openai_compatible", advanced: createProviderAdvanced() },
  { id: "glm", name: "GLM", group: "Codex", endpoint: "https://open.bigmodel.cn/api/v1", model: "glm-4", enabled: false, icon: "/providers/zhipu.svg", builtIn: true, providerType: "openai_compatible", advanced: createProviderAdvanced() },

];

export function inferProviderIcon(id: string = "", name: string = ""): string | null {
  const text = `${id} ${name}`.toLowerCase();
  if (text.includes("deepseek")) return "/providers/deepseek.svg";
  if (text.includes("kimi") || text.includes("moonshot")) return "/providers/kimi.svg";
  if (text.includes("glm") || text.includes("zhipu") || text.includes("智谱") || text.includes("bigmodel")) return "/providers/zhipu.svg";
  if (text.includes("grok") || text.includes("xai") || text.includes("x.ai")) return "/providers/xai.svg";
  if (text.includes("gemini") || text.includes("google")) return "/providers/googlegemini.svg";
  if (text.includes("anthropic") || text.includes("claude")) return "/providers/anthropic.svg";
  if (text.includes("openai") || text.includes("chatgpt") || text.includes("gpt")) return "/providers/openai.svg";
  return null;
}

export function normalizeProvider(raw: any): Provider {
  if (!raw || typeof raw !== "object") {
    return providers[0];
  }
  const fallback = providers.find((p) => p.id === raw.id);
  const group = (raw.group || raw.appGroup || fallback?.group || "Codex") as "Claude" | "Codex" | "Gemini";
  const builtIn = raw.builtIn !== undefined ? Boolean(raw.builtIn) : Boolean(fallback?.builtIn);
  const inferred = inferProviderIcon(raw.id, raw.name);
  const rawIcon = raw.icon || fallback?.icon || inferred || DEFAULT_PROVIDER_ICON;
  const icon = (rawIcon.includes("custom.jpg") || rawIcon === DEFAULT_PROVIDER_ICON)
    ? (fallback?.icon || inferred || DEFAULT_PROVIDER_ICON)
    : rawIcon;
  const providerType = raw.providerType || fallback?.providerType || "openai";
  const model = typeof raw.model === "string" ? raw.model : (fallback?.model ?? "");
  const endpoint = typeof raw.endpoint === "string" ? raw.endpoint : (fallback?.endpoint ?? "");
  const enabled = raw.enabled !== undefined ? Boolean(raw.enabled) : true;
  const advancedBase = raw.advanced || fallback?.advanced || {};
  const defaults = createProviderAdvanced(providerType === "anthropic" ? "anthropic" : "openai_responses");
  // 旧版本存下来的 advanced 里可能没有 codex / claude 这两个嵌套对象，
  // 直接展开会让编辑器读 value.claude.disableArtifactTool 时白屏，这里统一补齐。
  const advanced = {
    ...defaults,
    ...advancedBase,
    codex: { ...defaults.codex, ...(advancedBase.codex ?? {}) },
    claude: { ...defaults.claude, ...(advancedBase.claude ?? {}) },
  };
  if (!["openai_responses", "openai_chat", "anthropic"].includes(advanced.upstreamFormat)) {
    advanced.upstreamFormat = defaults.upstreamFormat;
  }

  return {
    ...fallback,
    ...raw,
    group,
    builtIn,
    icon,
    providerType,
    model,
    endpoint,
    enabled,
    advanced,
  };
}

