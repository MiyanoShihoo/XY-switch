import { invoke } from "@tauri-apps/api/core";

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function setProviderApiKey(providerId: string, apiKey: string): Promise<void> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不支持 Windows 安全凭据存储");
  await invoke("set_provider_api_key", { providerId, apiKey });
}

export async function hasProviderApiKey(providerId: string): Promise<boolean> {
  if (!isTauriRuntime()) return false;
  return invoke<boolean>("has_provider_api_key", { providerId });
}

export async function deleteProviderApiKey(providerId: string): Promise<void> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不支持 Windows 安全凭据存储");
  await invoke("delete_provider_api_key", { providerId });
}
