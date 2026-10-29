import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const root = process.cwd();
const read = (file: string) => readFileSync(resolve(root, file), "utf8");

describe("release metadata and bilingual documentation", () => {
  it("uses one application version across JavaScript, Tauri and Cargo", () => {
    const version = JSON.parse(read("package.json")).version;
    expect(version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(JSON.parse(read("src-tauri/tauri.conf.json")).version).toBe(version);
    expect(read("src-tauri/Cargo.toml").match(/^version = "([^"]+)"/m)?.[1]).toBe(version);
    expect(read("src-tauri/Cargo.lock").match(/name = "xy-switch"\r?\nversion = "([^"]+)"/)?.[1]).toBe(version);
    expect(existsSync(resolve(root, `docs/release-notes-${version}.md`))).toBe(true);
  });

  for (const file of ["README.md", "README.zh-CN.md", "docs/user-guide-en.md", "docs/user-guide-zh.md", "docs/session-favorites-20261029.md", "docs/governance-acceptance-20261029.md", "docs/packaging-en.md", "docs/packaging.md", "docs/release-notes-1.1.0.md", "docs/release-notes-1.2.0.md", "docs/governance-live-followup-20261029.md"]) {
    it(`keeps local links resolvable in ${file}`, () => {
      for (const match of read(file).matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) {
        const href = match[1].split("#")[0];
        if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href)) continue;
        expect(existsSync(resolve(root, dirname(file), decodeURIComponent(href))), `${file}: ${href}`).toBe(true);
      }
    });
  }

  it("ships a complete standalone bilingual website without remote scripts", () => {
    const page = read("website/index.html");
    const match = page.match(/<script id="translations" type="application\/json">([\s\S]*?)<\/script>/);
    expect(match).not.toBeNull();
    const catalog = JSON.parse(match![1]) as Record<string, Record<string, string>>;
    expect(Object.keys(catalog["en"]).sort()).toEqual(Object.keys(catalog["zh-CN"]).sort());
    for (const key of [...page.matchAll(/data-i18n="([^"]+)"/g)].map(match => match[1])) {
      for (const language of ["en", "zh-CN"]) expect(catalog[language][key]?.trim(), `${language}: ${key}`).toBeTruthy();
    }
    expect(page).not.toMatch(/<script[^>]+src\s*=/i);
    expect(page).not.toMatch(/cdn\.tailwindcss|unpkg\.com|DPAPI/);
    expect(page).toContain('data-language="en"');
    expect(page).toContain('data-language="zh-CN"');
    expect(page).toContain("MiyanoShihoo/XY-switch");
  });
});
