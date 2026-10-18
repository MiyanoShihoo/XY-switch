import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import packageMetadata from "../package.json";
import { copyProjectRepository, openProjectRepository, PROJECT_REPOSITORY_URL, repositoryUrlFromMetadata } from "./lib/project-info";
import { ProjectRepository } from "./features/settings/ProjectRepository";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
const repository = "https://example.org/team/xy-switch";
beforeEach(() => { invokeMock.mockReset(); });
afterEach(() => { vi.unstubAllGlobals(); });

describe("project repository metadata", () => {
  it.each([
    [repository, repository],
    [repository + ".git", repository],
    [repository + ".git/", repository],
    ["git+" + repository + ".git", repository],
    [{ type: "git", url: repository + ".git" }, repository],
    ["  " + repository + "  ", repository],
    ["http://example.org/team/project", "http://example.org/team/project"],
  ])("normalizes an explicit project address: %j", (input, expected) => {
    expect(repositoryUrlFromMetadata(input)).toBe(expected);
  });

  it.each([undefined, null, "", "   ", 42, {}, { url: false }, "not-a-url", "javascript:alert(1)", "file:///C:/test", "https://example.org", "https://example.org/.git", repository + "?token=fixture", repository + "#fragment", "git@example.org:team/repo.git"])("does not invent a web address for invalid metadata: %j", value => {
    expect(repositoryUrlFromMetadata(value)).toBeNull();
  });

  it("rejects a URL containing embedded authentication", () => {
    const url = new URL(repository);
    url.username = "fixture";
    url.password = "fixture";
    expect(repositoryUrlFromMetadata(url.toString())).toBeNull();
  });

  it("gets the application address from package metadata", () => {
    expect(PROJECT_REPOSITORY_URL).toBe(repositoryUrlFromMetadata((packageMetadata as { repository?: unknown }).repository));
  });
});

describe("repository actions", () => {
  it("opens the canonical address through the native external URL command", async () => {
    invokeMock.mockResolvedValue(undefined);
    await openProjectRepository(repository + ".git");
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("open_external_url", { url: repository });
  });

  it("never invokes the native shell for an invalid URL", async () => {
    await expect(openProjectRepository("file:///C:/test")).rejects.toThrow("仓库地址无效");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("propagates an external browser failure so the UI can display it", async () => {
    invokeMock.mockRejectedValue(new Error("open failed"));
    await expect(openProjectRepository(repository)).rejects.toThrow("open failed");
  });

  it("copies the canonical repository address", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await copyProjectRepository(repository + ".git");
    expect(writeText).toHaveBeenCalledExactlyOnceWith(repository);
  });

  it("reports an unavailable clipboard instead of announcing a false success", async () => {
    vi.stubGlobal("navigator", {});
    await expect(copyProjectRepository(repository)).rejects.toThrow("请手动复制");
  });

  it("propagates a clipboard permission failure", async () => {
    vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("permission denied")) } });
    await expect(copyProjectRepository(repository)).rejects.toThrow("permission denied");
  });
});

describe("about repository section", () => {
  it("renders an accessible link and copy action without opening a browser on render", () => {
    const html = renderToStaticMarkup(createElement(ProjectRepository, { repositoryUrl: repository }));
    expect(html).toContain('href="' + repository + '"');
    expect(html).toContain('target="_blank"');
    expect(html).toContain('rel="noopener noreferrer"');
    expect(html).toContain("项目仓库");
    expect(html).toContain("复制地址");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it.each([null, "javascript:alert(1)"])("does not advertise an unconfirmed or unsafe repository", value => {
    const html = renderToStaticMarkup(createElement(ProjectRepository, { repositoryUrl: value }));
    expect(html).toContain("仓库地址尚未配置");
    expect(html).not.toContain("href=");
    expect(html).not.toContain("复制地址");
  });
});
