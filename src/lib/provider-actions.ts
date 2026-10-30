import { t } from "../i18n/language";
import { DEFAULT_PROVIDER_ICON } from "./providers";
import type { Provider, ProviderAdvancedConfig } from "../types/provider";

function cloneAdvanced(config: ProviderAdvancedConfig): ProviderAdvancedConfig {
  return {
    ...config,
    modelMappings: config.modelMappings.map((mapping) => ({ ...mapping })),
    headerOverrides: { ...config.headerOverrides },
    bodyOverrides: structuredClone(config.bodyOverrides),
    codex: { ...config.codex },
    claude: { ...config.claude },
  };
}

export function duplicateProvider(source: Provider, providers: Provider[]): Provider {
  const ids = new Set(providers.map((provider) => provider.id.toLowerCase()));
  const names = new Set(providers.map((provider) => provider.name.toLowerCase()));
  const baseId = `${source.id}-copy`;
  const baseName = t("{value0} 副本", { value0: source.name });
  let id = baseId;
  let name = baseName;
  let suffix = 2;
  while (ids.has(id.toLowerCase()) || names.has(name.toLowerCase())) {
    id = `${baseId}-${suffix}`;
    name = `${baseName} ${suffix}`;
    suffix += 1;
  }
  const highestSortOrder = providers.reduce(
    (highest, provider) => Math.max(highest, provider.sortOrder ?? 0),
    0,
  );
  return {
    ...source,
    id,
    name,
    icon: DEFAULT_PROVIDER_ICON,
    builtIn: false,
    enabled: false,
    providerType: source.providerType ?? "custom",
    sortOrder: highestSortOrder + 10,
    advanced: cloneAdvanced(source.advanced),
  };
}

export function reorderProviders(
  providers: Provider[],
  sourceId: string,
  targetId: string,
): Provider[] {
  const sourceIndex = providers.findIndex((provider) => provider.id === sourceId);
  const targetIndex = providers.findIndex((provider) => provider.id === targetId);
  if (sourceIndex < 0 || targetIndex < 0 || sourceIndex === targetIndex) return providers;

  const reordered = [...providers];
  const [source] = reordered.splice(sourceIndex, 1);
  const adjustedTargetIndex = reordered.findIndex((provider) => provider.id === targetId);
  const insertionIndex = sourceIndex < targetIndex ? adjustedTargetIndex + 1 : adjustedTargetIndex;
  reordered.splice(insertionIndex, 0, source);
  return reordered.map((provider, index) => ({ ...provider, sortOrder: (index + 1) * 10 }));
}
