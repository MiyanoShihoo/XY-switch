import { t } from "../i18n/language";
import type { McpServer } from "../types/mcp";

export function parseKeyValueLines(text: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [index, raw] of text.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line) continue;
    const separator = line.indexOf("=");
    if (separator <= 0) throw new Error(t("第 {value0} 行必须使用 KEY=value 格式", { value0: index + 1 }));
    const key = line.slice(0, separator).trim();
    if (!key || /[\r\n]/.test(key)) throw new Error(t("第 {value0} 行的名称无效", { value0: index + 1 }));
    if (Object.hasOwn(result, key)) throw new Error(t("第 {value0} 行的名称重复：{value1}", { value0: index + 1, value1: key }));
    result[key] = line.slice(separator + 1);
  }
  return result;
}

export function formatKeyValueLines(values: Record<string, string>): string {
  return Object.entries(values).map(([key, value]) => `${key}=${value}`).join("\n");
}

export function validateMcpServer(server: McpServer): string | null {
  if (!/^[A-Za-z0-9._-]+$/.test(server.id.trim())) {
    return t("MCP ID 只能包含字母、数字、点、下划线和连字符");
  }
  if (!server.name.trim()) return t("请输入 MCP 名称");
  if (server.transport === "stdio" && !server.command?.trim()) return t("请输入启动命令");
  if (server.transport === "http") {
    try {
      const url = new URL(server.url ?? "");
      if (!url.hostname || !["http:", "https:"].includes(url.protocol)) return t("请输入有效的 HTTP MCP URL");
    } catch {
      return t("请输入有效的 HTTP MCP URL");
    }
  }
  return null;
}
