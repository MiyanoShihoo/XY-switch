import { invoke } from "@tauri-apps/api/core";

export interface ImportResult {
  providers: number;
  prompts: number;
  mcpServers: number;
  proxyImported: boolean;
  modelPricing: number;
}

export const exportConfig = () => invoke<string>("export_config");
export const importConfig = (content: string) =>
  invoke<ImportResult>("import_config", { content });

export function downloadConfig(content: string): void {
  const blob = new Blob([content], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `xy-switch-config-${new Date().toISOString().slice(0, 10)}.json`;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
