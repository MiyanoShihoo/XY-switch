import type { DiscoveredModel } from "./model-service";
import type { ProviderModelMapping } from "../types/provider";

function normalizedModelId(value: string): string {
  return value.trim();
}

export function addDiscoveredModelMapping(
  existing: ProviderModelMapping[],
  discovered: DiscoveredModel,
): ProviderModelMapping[] {
  const model = normalizedModelId(discovered.id);
  if (!model) return [...existing];

  const duplicate = existing.some(
    (mapping) => normalizedModelId(mapping.model).toLocaleLowerCase() === model.toLocaleLowerCase(),
  );
  if (duplicate) return [...existing];

  return [
    ...existing,
    {
      model,
      displayName: discovered.name.trim() || model,
      contextWindow: null,
    },
  ];
}

export function mergeDiscoveredModelMappings(
  existing: ProviderModelMapping[],
  discovered: DiscoveredModel[],
): ProviderModelMapping[] {
  return discovered.reduce(
    (mappings, model) => addDiscoveredModelMapping(mappings, model),
    [...existing],
  );
}

export function reorderModelMappings(
  mappings: ProviderModelMapping[],
  fromIndex: number,
  toIndex: number,
): ProviderModelMapping[] {
  if (
    !Number.isInteger(fromIndex) ||
    !Number.isInteger(toIndex) ||
    fromIndex < 0 ||
    toIndex < 0 ||
    fromIndex >= mappings.length ||
    toIndex >= mappings.length ||
    fromIndex === toIndex
  ) {
    return [...mappings];
  }

  const reordered = [...mappings];
  const [moved] = reordered.splice(fromIndex, 1);
  reordered.splice(toIndex, 0, moved);
  return reordered;
}
