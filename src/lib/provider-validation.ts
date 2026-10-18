import type { Provider } from "../types/provider";

export type ProviderFormInput = Pick<Provider, "id" | "name" | "endpoint" | "model">;
export const MAX_API_KEY_BYTES = 2560;

export function validateProvider(form: ProviderFormInput, providers: Provider[], editingId: string | null): string {
  const id = form.id.trim();
  if (!id || !form.name.trim() || !form.endpoint.trim() || !form.model.trim()) {
    return "请完整填写供应商 ID、名称、端点和模型。";
  }
  if (!/^[a-z0-9][a-z0-9_-]*$/i.test(id)) {
    return "供应商 ID 只能包含字母、数字、下划线和短横线。";
  }
  try {
    const url = new URL(form.endpoint.trim());
    if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error();
  } catch {
    return "请输入有效的 HTTP 或 HTTPS 端点地址。";
  }
  const duplicate = providers.some((provider) =>
    provider.id.toLowerCase() === id.toLowerCase() &&
    provider.id.toLowerCase() !== editingId?.toLowerCase()
  );
  return duplicate ? "供应商 ID 已存在，请使用其他 ID。" : "";
}

export function validateApiKey(apiKey: string): string {
  if (!apiKey) return "";
  if (!apiKey.trim()) return "API Key 不能只包含空格。";
  return new TextEncoder().encode(apiKey.trim()).length > MAX_API_KEY_BYTES
    ? `API Key 不能超过 ${MAX_API_KEY_BYTES} 字节。`
    : "";
}

export function selectProviderAfterDelete(currentId: string, deletedId: string, remaining: Provider[]): string {
  if (currentId !== deletedId) return currentId;
  return remaining.find((provider) => provider.enabled)?.id ?? remaining[0]?.id ?? "";
}
