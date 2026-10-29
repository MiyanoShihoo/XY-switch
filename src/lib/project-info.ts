import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";
import packageMetadata from "../../package.json";

/** Accept package.json repository metadata, but never publish credentials or an executable URL scheme. */
export function repositoryUrlFromMetadata(repository: unknown): string | null {
  const value = typeof repository === "string" ? repository
    : repository && typeof repository === "object" && "url" in repository ? repository.url : null;
  if (typeof value !== "string" || !value.trim()) return null;
  try {
    const url = new URL(value.trim().replace(/^git\+/, ""));
    if (!["https:", "http:"].includes(url.protocol) || url.username || url.password || url.search || url.hash) return null;
    const path = url.pathname.replace(/\/+$/, "").replace(/\.git$/i, "");
    if (!path || path === "/") return null;
    url.pathname = path;
    return url.toString();
  } catch {
    return null;
  }
}

export const PROJECT_REPOSITORY_URL = repositoryUrlFromMetadata(
  (packageMetadata as { repository?: unknown }).repository,
);

function requireRepositoryUrl(repository: string): string {
  const url = repositoryUrlFromMetadata(repository);
  if (!url) throw new Error(t("仓库地址无效，仅支持不含凭据的 HTTP/HTTPS 项目地址。"));
  return url;
}

/** Desktop links must use the native command, not navigate the application's WebView. */
export async function openProjectRepository(repository: string): Promise<void> {
  await invoke<void>("open_external_url", { url: requireRepositoryUrl(repository) });
}

export async function copyProjectRepository(repository: string): Promise<void> {
  const url = requireRepositoryUrl(repository);
  if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
    throw new Error(t("当前环境无法访问剪贴板，请手动复制仓库地址。"));
  }
  await navigator.clipboard.writeText(url);
}
