#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import * as fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const formats = Object.freeze({
  nsis: { dir: "nsis", ext: ".exe", suffix: "-setup.exe" },
  msi: { dir: "msi", ext: ".msi", suffix: ".msi" },
  deb: { dir: "deb", ext: ".deb", suffix: ".deb" },
  appimage: { dir: "appimage", ext: ".AppImage", suffix: ".AppImage" },
  app: { dir: "macos", ext: ".app", suffix: ".app.zip", directory: true },
  dmg: { dir: "dmg", ext: ".dmg", suffix: ".dmg" },
});

export function parseArgs(args) {
  const [platform, ...rest] = args;
  if (!["windows", "linux", "macos"].includes(platform)) throw new Error("Usage: node scripts/package-release.mjs windows|linux|macos [--tools-dir PATH] [--out-dir PATH] [--bundles FORMATS] [--universal] [--dry-run]");
  const options = { platform, dryRun: false };
  const names = { "--tools-dir": "toolsDir", "--out-dir": "outDir", "--bundles": "bundles" };
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === "--dry-run") { options.dryRun = true; continue; }
    if (rest[i] === "--universal") {
      if (platform !== "macos" || options.universal) throw new Error("--universal is supported once, on macOS only");
      options.universal = true; continue;
    }
    const name = names[rest[i]];
    if (!name || !rest[i + 1] || rest[i + 1].startsWith("--")) throw new Error(`Invalid or incomplete option: ${rest[i]}`);
    if (options[name] !== undefined) throw new Error(`Duplicate option: ${rest[i]}`);
    options[name] = rest[++i];
  }
  return options;
}

export function planBuild(options, context) {
  const { projectRoot, host, arch, version, env = {} } = context;
  const expected = { windows: "win32", linux: "linux", macos: "darwin" }[options.platform];
  if (!expected) throw new Error("Unsupported platform");
  if (options.universal && host !== "darwin") throw new Error("Universal builds require macOS");
  if (host !== expected) throw new Error(`Build ${options.platform} on a native ${expected} host (use WSL for Linux); refusing cross-platform packaging.`);
  if (!["x64", "arm64"].includes(arch)) throw new Error(`Unsupported architecture: ${arch}`);
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error("Invalid package version");
  const allowed = { windows: ["nsis", "msi"], linux: ["deb", "appimage"], macos: ["app", "dmg"] }[options.platform];
  const bundles = options.bundles === undefined ? (arch === "arm64" && host === "win32" ? ["nsis"] : allowed) : options.bundles.split(",");
  if (!bundles.length || new Set(bundles).size !== bundles.length || bundles.some(b => !allowed.includes(b))) throw new Error(`Invalid bundle set for ${options.platform}: ${options.bundles}`);
  if (host === "win32" && arch === "arm64" && bundles.includes("msi")) throw new Error("Use NSIS for Windows ARM64");
  if (env.CARGO_BUILD_TARGET) throw new Error("Unset CARGO_BUILD_TARGET for native packaging; use the Tauri CLI directly for cross-compilation.");
  const targetDir = path.resolve(projectRoot, env.CARGO_TARGET_DIR || "src-tauri/target");
  const toolsDir = path.resolve(projectRoot, options.toolsDir || env.XY_SWITCH_TOOLS_DIR || ".build-tools");
  const cacheDir = path.join(targetDir, ".tauri");
  if (toolsDir === cacheDir || toolsDir.startsWith(cacheDir + path.sep)) throw new Error("Tools directory must be outside target/.tauri to avoid a symlink cycle");
  const outputArch = options.universal ? "universal" : arch;
  const targetArgs = options.universal ? ["--target", "universal-apple-darwin"] : [];
  return {
    platform: options.platform, arch: outputArch, universal: Boolean(options.universal), version, bundles, targetDir, toolsDir, cacheDir,
    outDir: path.resolve(projectRoot, options.outDir || `release/${version}/${options.platform}-${outputArch}`),
    bundleDir: options.universal ? path.join(targetDir, "universal-apple-darwin", "release", "bundle") : path.join(targetDir, "release", "bundle"),
    command: process.execPath,
    args: [path.join(projectRoot, "node_modules", "@tauri-apps", "cli", "tauri.js"), "build", "--ci", "--bundles", bundles.join(","), ...targetArgs, "--", "--locked"],
  };
}

export async function ensureToolsCache(cacheDir, toolsDir, host = process.platform) {
  await fs.mkdir(toolsDir, { recursive: true });
  await fs.mkdir(path.dirname(cacheDir), { recursive: true });
  try {
    await fs.lstat(cacheDir);
    const [existing, desired] = await Promise.all([fs.realpath(cacheDir), fs.realpath(toolsDir)]);
    if (existing === desired) return;
    throw new Error(`Refusing to replace existing tool cache: ${cacheDir}. Choose a fresh CARGO_TARGET_DIR or move the cache yourself.`);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    // Do not silently replace a dangling link: symlink() fails with EEXIST.
    await fs.symlink(path.resolve(toolsDir), cacheDir, host === "win32" ? "junction" : "dir");
  }
}

export async function sha256(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

export async function archiveMacApplication(source, destination) {
  if (process.platform !== "darwin") throw new Error("Archiving .app bundles requires macOS ditto");
  const result = spawnSync("ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", source, destination], { stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error("Failed to archive the macOS application bundle");
}

export async function collectBundles(plan, startedAt, { archiveApp = archiveMacApplication } = {}) {
  const candidates = [];
  for (const kind of plan.bundles) {
    const format = formats[kind];
    const directory = path.join(plan.bundleDir, format.dir);
    const entries = await fs.readdir(directory, { withFileTypes: true });
    const matches = [];
    for (const entry of entries) {
      if (!(format.directory ? entry.isDirectory() : entry.isFile()) || !entry.name.endsWith(format.ext)) continue;
      const source = path.join(directory, entry.name);
      // The app directory may predate this build; its generated plist must be fresh.
      const stat = await fs.stat(format.directory ? path.join(source, "Contents", "Info.plist") : source);
      if (format.directory) {
        const executables = await fs.readdir(path.join(source, "Contents", "MacOS"), { withFileTypes: true });
        if (!executables.some(file => file.isFile())) throw new Error("macOS application has no executable");
      }
      if (stat.size > 0 && stat.mtimeMs >= startedAt - 2000) matches.push({ source, size: stat.size });
    }
    if (matches.length !== 1) throw new Error(`Expected exactly one freshly built ${kind} installer, found ${matches.length}. No old package will be relabeled.`);
    candidates.push({ ...matches[0], kind, name: `XY-Switch-${plan.version}-${plan.platform}-${plan.arch}${format.suffix}` });
  }
  // Validate every format before writing any package into the output directory.
  await fs.mkdir(plan.outDir, { recursive: true });
  const files = [];
  for (const candidate of candidates) {
    const destination = path.join(plan.outDir, candidate.name);
    let actual;
    if (candidate.kind === "app") {
      const temporary = destination + `.partial-${process.pid}`;
      try {
        await archiveApp(candidate.source, temporary);
        const stat = await fs.stat(temporary);
        if (!stat.size) throw new Error("Empty macOS application archive");
        candidate.size = stat.size;
        actual = await sha256(temporary);
        await fs.rename(temporary, destination);
      } finally { await fs.rm(temporary, { force: true }); }
    } else {
      await fs.copyFile(candidate.source, destination);
      const expected = await sha256(candidate.source);
      actual = await sha256(destination);
      if (expected !== actual) throw new Error(`Copied installer checksum mismatch: ${candidate.name}`);
    }
    if (candidate.kind === "appimage") await fs.chmod(destination, 0o755);
    files.push({ name: candidate.name, format: candidate.kind, bytes: candidate.size, sha256: actual });
  }
  const manifest = { version: plan.version, platform: plan.platform, architecture: plan.arch, builtAt: new Date().toISOString(), signatureVerification: "not-performed", files };
  // Signing, when configured separately, must be independently checked before claiming a signed package.
  await fs.writeFile(path.join(plan.outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  await fs.writeFile(path.join(plan.outDir, "SHA256SUMS.txt"), files.map(f => `${f.sha256}  ${f.name}\n`).join(""));
  return manifest;
}

export async function main(args = process.argv.slice(2)) {
  const options = parseArgs(args);
  const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
  const config = JSON.parse(await fs.readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
  if (config.version !== pkg.version) throw new Error("package.json and tauri.conf.json versions differ");
  if (config.bundle?.useLocalToolsDir !== true) throw new Error("Set bundle.useLocalToolsDir=true so downloaded bundlers stay in the selected tools directory");
  const plan = planBuild(options, { projectRoot: root, host: process.platform, arch: process.arch, version: pkg.version, env: process.env });
  if (options.dryRun) { console.log(JSON.stringify(plan, null, 2)); return; }
  await fs.access(plan.args[0]);
  await ensureToolsCache(plan.cacheDir, plan.toolsDir);
  if (plan.universal) {
    const targets = spawnSync("rustup", ["target", "add", "aarch64-apple-darwin", "x86_64-apple-darwin"], { stdio: "inherit", shell: false });
    if (targets.error) throw targets.error;
    if (targets.status !== 0) throw new Error("Failed to install universal macOS Rust targets");
  }
  const startedAt = Date.now();
  const result = spawnSync(plan.command, plan.args, {
    cwd: root, stdio: "inherit", shell: false,
    env: { ...process.env, CARGO_TARGET_DIR: plan.targetDir, ...(plan.platform === "macos" ? { MACOSX_DEPLOYMENT_TARGET: "11.0", APPLE_SIGNING_IDENTITY: process.env.APPLE_SIGNING_IDENTITY || "-" } : {}) },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Tauri build failed (${result.signal || result.status}); no installer was published`);
  const manifest = await collectBundles(plan, startedAt);
  console.log(JSON.stringify({ output: plan.outDir, ...manifest }, null, 2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
