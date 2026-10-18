import type { ProviderPreset } from "../config/providerPresets";
import { createProviderAdvanced, DEFAULT_PROVIDER_ICON, inferProviderIcon } from "./providers";
import type { Provider } from "../types/provider";

/** 供应商编辑表单需要的那几个字段（与 App.tsx 的 FormState 同构）。 */
export type ProviderDraft = Pick<
  Provider,
  "id" | "name" | "group" | "endpoint" | "model" | "enabled" | "advanced" | "icon" | "providerType"
>;

/** 预设 id 与已有供应商冲突时，追加 -2 / -3 …… 直到不重复。 */
function uniqueId(base: string, taken: string[]): string {
  const used = new Set(taken.map((id) => id.toLowerCase()));
  if (!used.has(base.toLowerCase())) return base;
  let suffix = 2;
  while (used.has(`${base}-${suffix}`.toLowerCase())) suffix += 1;
  return `${base}-${suffix}`;
}

/**
 * 把一条快捷预设展开成可以直接塞进编辑表单的草稿。
 *
 * 只填预设明确声明的字段，其余一律取 createProviderAdvanced() 的默认值，
 * 这样以后 advanced 里新增开关不会因为预设没写而变成 undefined。
 */
export function presetToProviderDraft(
  preset: ProviderPreset,
  existingIds: string[] = [],
): ProviderDraft {
  const advanced = createProviderAdvanced(preset.upstreamFormat);
  return {
    id: uniqueId(preset.id, existingIds),
    name: preset.name,
    group: "Codex", // Legacy serialization only; never used for routing.
    providerType: preset.providerType,
    endpoint: preset.endpoint,
    model: preset.model,
    enabled: true,
    icon: inferProviderIcon(preset.id, preset.name) ?? DEFAULT_PROVIDER_ICON,
    advanced,
  };
}
