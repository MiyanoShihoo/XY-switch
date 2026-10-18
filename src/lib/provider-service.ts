import { invoke } from "@tauri-apps/api/core";
import type { Provider, StoredProvider } from "../types/provider";
import { providers as builtInProviders, normalizeProvider } from "./providers";

const selectedProviderKey = "xy-switch:selected-provider";

function isDesktopRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function previewProviders(): Provider[] {
  return builtInProviders.map((provider) => ({
    ...provider,
    advanced: {
      ...provider.advanced,
      modelMappings: provider.advanced.modelMappings.map((mapping) => ({ ...mapping })),
      headerOverrides: { ...provider.advanced.headerOverrides },
      bodyOverrides: { ...provider.advanced.bodyOverrides },
      codex: { ...provider.advanced.codex },
      claude: { ...provider.advanced.claude },
    },
  }));
}

export async function loadProviders(): Promise<Provider[]> {
  if (!isDesktopRuntime()) return previewProviders();
  const records = await invoke<StoredProvider[]>("list_providers");
  return records.map(normalizeProvider);
}

export async function loadCurrentProvider(): Promise<string> {
  if (!isDesktopRuntime()) return loadSelectedProvider();
  const provider = await invoke<StoredProvider | null>("get_current_provider");
  return provider?.id ?? "";
}

export function loadSelectedProvider(): string {
  return localStorage.getItem(selectedProviderKey) ?? "anthropic";
}

export async function saveSelectedProvider(id: string): Promise<void> {
  if (!id) {
    localStorage.removeItem(selectedProviderKey);
    return;
  }

  if (isDesktopRuntime()) {
    await invoke("set_current_provider", { id });
  }
  localStorage.setItem(selectedProviderKey, id);
}

export async function saveProvider(provider: Provider): Promise<void> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能修改供应商");
  const payload: StoredProvider = {
    id: provider.id,
    name: provider.name,
    endpoint: provider.endpoint,
    model: provider.model,
    enabled: provider.enabled,
    advanced: provider.advanced,
    appGroup: provider.group,
    providerType: provider.providerType || "custom",
    sortOrder: provider.sortOrder ?? 0,
  };
  await invoke("save_provider", { provider: payload });
}

export async function deleteProvider(id: string): Promise<void> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能删除供应商");
  await invoke("delete_provider", { id });
}

export async function saveProviderOrder(ids: string[]): Promise<void> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能调整供应商顺序");
  await invoke("reorder_providers", { providerIds: ids });
}

export async function refreshCodexProvider(id: string): Promise<void> {
  if (isDesktopRuntime()) await invoke("refresh_codex_provider", { id });
}
