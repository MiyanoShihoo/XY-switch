import assert from "node:assert/strict";
import test from "node:test";
import * as fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { collectBundles, ensureToolsCache, parseArgs, planBuild, sha256 } from "./package-release.mjs";

const context = { projectRoot: path.resolve("fixture project"), host: "linux", arch: "x64", version: "1.0.0" };
async function temporary(t) {
  const base = path.resolve(os.tmpdir());
  const directory = await fs.mkdtemp(path.join(base, "xy-switch-packaging-"));
  t.after(async () => {
    const target = path.resolve(directory);
    assert.equal(path.dirname(target), base);
    assert.ok(path.basename(target).startsWith("xy-switch-packaging-"));
    await fs.rm(target, { recursive: true, force: true });
  });
  return directory;
}

test("parses paths with spaces without invoking a shell", () => {
  assert.deepEqual(parseArgs(["windows", "--tools-dir", "E:\\AI tools", "--out-dir", "release folder", "--bundles", "nsis", "--dry-run"]), {
    platform: "windows", toolsDir: "E:\\AI tools", outDir: "release folder", bundles: "nsis", dryRun: true,
  });
});
for (const args of [[], ["ios"], ["linux", "--tools-dir"], ["linux", "--bad"], ["linux", "--out-dir", "--dry-run"], ["linux", "--bundles", "deb", "--bundles", "appimage"]]) {
  test(`rejects invalid arguments ${JSON.stringify(args)}`, () => assert.throws(() => parseArgs(args)));
}
test("plans a native locked Linux build with isolated tool cache", () => {
  const plan = planBuild(parseArgs(["linux"]), context);
  assert.deepEqual(plan.bundles, ["deb", "appimage"]);
  assert.equal(plan.toolsDir, path.join(context.projectRoot, ".build-tools"));
  assert.equal(plan.cacheDir, path.join(context.projectRoot, "src-tauri", "target", ".tauri"));
  assert.deepEqual(plan.args.slice(-2), ["--", "--locked"]);
  assert.ok(plan.args[0].endsWith(path.join("@tauri-apps", "cli", "tauri.js")));
});
test("supports explicit tools, output and target directories", () => {
  const plan = planBuild(parseArgs(["linux", "--tools-dir", "downloaded tools", "--out-dir", "packages", "--bundles", "deb"]), {
    ...context, env: { CARGO_TARGET_DIR: "native target", XY_SWITCH_TOOLS_DIR: "unused" },
  });
  assert.equal(plan.toolsDir, path.join(context.projectRoot, "downloaded tools"));
  assert.equal(plan.cacheDir, path.join(context.projectRoot, "native target", ".tauri"));
  assert.equal(plan.outDir, path.join(context.projectRoot, "packages"));
});
test("respects the tools environment variable", () => {
  assert.equal(planBuild(parseArgs(["linux"]), { ...context, env: { XY_SWITCH_TOOLS_DIR: "shared tools" } }).toolsDir, path.join(context.projectRoot, "shared tools"));
});
test("Windows x64 builds NSIS and MSI", () => {
  assert.deepEqual(planBuild(parseArgs(["windows"]), { ...context, host: "win32" }).bundles, ["nsis", "msi"]);
});
test("Windows ARM64 defaults to NSIS, never silently produces an x64 MSI", () => {
  assert.deepEqual(planBuild(parseArgs(["windows"]), { ...context, host: "win32", arch: "arm64" }).bundles, ["nsis"]);
  assert.throws(() => planBuild(parseArgs(["windows", "--bundles", "msi"]), { ...context, host: "win32", arch: "arm64" }), /ARM64/);
});
for (const bundles of ["nsis", "deb,deb", "", "deb,", "all"]) {
  test(`rejects a wrong or ambiguous Linux format: ${bundles}`, () => assert.throws(() => planBuild({ platform: "linux", bundles }, context), /bundle/));
}
test("refuses wrong hosts, architectures, versions, implicit cross targets and cache cycles", () => {
  assert.throws(() => planBuild({ platform: "windows" }, context), /native/);
  assert.throws(() => planBuild({ platform: "linux" }, { ...context, arch: "ia32" }), /architecture/);
  assert.throws(() => planBuild({ platform: "linux" }, { ...context, version: "..\/oops" }), /version/);
  assert.throws(() => planBuild({ platform: "linux" }, { ...context, env: { CARGO_BUILD_TARGET: "aarch64-unknown-linux-gnu" } }), /CARGO_BUILD_TARGET/);
  assert.throws(() => planBuild({ platform: "linux", toolsDir: "src-tauri/target/.tauri/nested" }, context), /cycle/);
});
test("creates a reusable cache link and preserves an unrelated existing cache", async t => {
  const directory = await temporary(t);
  const tools = path.join(directory, "AI tools");
  const cache = path.join(directory, "target", ".tauri");
  await ensureToolsCache(cache, tools);
  await ensureToolsCache(cache, tools);
  assert.equal(await fs.realpath(cache), await fs.realpath(tools));
  const existing = path.join(directory, "old-cache");
  await fs.mkdir(existing);
  await fs.writeFile(path.join(existing, "keep"), "untouched");
  await assert.rejects(ensureToolsCache(existing, tools), /Refusing to replace/);
  assert.equal(await fs.readFile(path.join(existing, "keep"), "utf8"), "untouched");
});
test("only publishes a fresh installer with independently matching checksums", async t => {
  const directory = await temporary(t);
  const plan = planBuild({ platform: "linux", bundles: "deb" }, { ...context, projectRoot: directory });
  const bundle = path.join(plan.bundleDir, "deb");
  await fs.mkdir(bundle, { recursive: true });
  const source = path.join(bundle, "fixture_1.0.0_amd64.deb");
  await fs.writeFile(source, "fixture package");
  const manifest = await collectBundles(plan, Date.now());
  assert.equal(manifest.files.length, 1);
  assert.equal(manifest.signatureVerification, "not-performed");
  const file = manifest.files[0];
  assert.equal(file.name, "XY-Switch-1.0.0-linux-x64.deb");
  assert.equal(file.sha256, await sha256(path.join(plan.outDir, file.name)));
  assert.equal(await fs.readFile(path.join(plan.outDir, "SHA256SUMS.txt"), "utf8"), `${file.sha256}  ${file.name}\n`);
});
test("does not relabel stale installers from an earlier build", async t => {
  const directory = await temporary(t);
  const plan = planBuild({ platform: "linux", bundles: "deb" }, { ...context, projectRoot: directory });
  const bundle = path.join(plan.bundleDir, "deb");
  await fs.mkdir(bundle, { recursive: true });
  const file = path.join(bundle, "old.deb");
  await fs.writeFile(file, "old package");
  await fs.utimes(file, new Date(0), new Date(0));
  await assert.rejects(collectBundles(plan, Date.now()), /freshly built/);
  await assert.rejects(fs.access(plan.outDir));
});
test("rejects empty and ambiguous installers before writing release output", async t => {
  const directory = await temporary(t);
  const plan = planBuild({ platform: "windows", bundles: "nsis" }, { ...context, projectRoot: directory, host: "win32" });
  const bundle = path.join(plan.bundleDir, "nsis");
  await fs.mkdir(bundle, { recursive: true });
  await fs.writeFile(path.join(bundle, "empty.exe"), "");
  await assert.rejects(collectBundles(plan, Date.now()), /found 0/);
  await fs.writeFile(path.join(bundle, "a.exe"), "fixture A");
  await fs.writeFile(path.join(bundle, "b.exe"), "fixture B");
  await assert.rejects(collectBundles(plan, Date.now()), /found 2/);
  await assert.rejects(fs.access(plan.outDir));
});

test("Linux config explicitly exposes the dynamic tray dependency to the bundler", async () => {
  const config = JSON.parse(await fs.readFile(new URL("../src-tauri/tauri.linux.conf.json", import.meta.url), "utf8"));
  assert.deepEqual(config.build.features, ["linux-tray"]);
  const cargo = await fs.readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8");
  // The CLI expands crate feature aliases, but ignores a raw dependency feature
  // when computing the bundler's library list (even though Cargo accepts it).
  const features = cargo.split("[features]")[1]?.split(/\n\[/)[0];
  assert.match(features || "", /^linux-tray\s*=\s*\["tauri\/tray-icon"\]/m);
  assert.ok(config.bundle.linux.deb.recommends.includes("gnome-keyring"));
  assert.ok(config.bundle.linux.deb.recommends.includes("fonts-noto-cjk"));
});

test("desktop-only bundler features are not enabled by the shared or iOS config", async () => {
  for (const file of ["tauri.conf.json", "tauri.ios.conf.json"]) {
    const config = JSON.parse(await fs.readFile(new URL("../src-tauri/" + file, import.meta.url), "utf8"));
    assert.ok(!(config.build?.features || []).some(feature => ["tauri/tray-icon", "linux-tray"].includes(feature)));
  }
});


test("macOS packages a native app and DMG or an explicit universal binary", () => {
  const native = planBuild(parseArgs(["macos"]), { ...context, host: "darwin", arch: "arm64" });
  assert.deepEqual(native.bundles, ["app", "dmg"]);
  assert.equal(native.arch, "arm64");
  const plan = planBuild(parseArgs(["macos", "--universal"]), { ...context, host: "darwin", arch: "arm64" });
  assert.equal(plan.arch, "universal");
  assert.ok(plan.args.includes("universal-apple-darwin"));
  assert.ok(plan.bundleDir.endsWith(path.join("universal-apple-darwin", "release", "bundle")));
  assert.throws(() => parseArgs(["windows", "--universal"]), /macOS/);
  assert.throws(() => parseArgs(["macos", "--universal", "--universal"]), /once/);
  assert.throws(() => planBuild(parseArgs(["macos"]), context), /native/);
  assert.throws(() => planBuild(parseArgs(["macos", "--bundles", "deb"]), { ...context, host: "darwin" }), /bundle/);
});

test("macOS app freshness uses the generated plist and hashes the actual archive", async t => {
  const directory = await temporary(t);
  const plan = planBuild({ platform: "macos", bundles: "app" }, { ...context, projectRoot: directory, host: "darwin" });
  const app = path.join(plan.bundleDir, "macos", "XY Switch.app");
  await fs.mkdir(path.join(app, "Contents", "MacOS"), { recursive: true });
  const plist = path.join(app, "Contents", "Info.plist");
  await fs.writeFile(plist, "fixture plist");
  await fs.writeFile(path.join(app, "Contents", "MacOS", "xy-switch"), "fixture binary");
  await fs.utimes(app, new Date(0), new Date(0));
  const manifest = await collectBundles(plan, Date.now(), { archiveApp: async (source, destination) => {
    assert.equal(source, app); await fs.writeFile(destination, "fixture archive");
  } });
  assert.equal(manifest.files[0].name, "XY-Switch-1.0.0-macos-x64.app.zip");
  assert.equal(manifest.files[0].bytes, Buffer.byteLength("fixture archive"));
  assert.equal(manifest.files[0].sha256, await sha256(path.join(plan.outDir, manifest.files[0].name)));
  await fs.utimes(plist, new Date(0), new Date(0));
  await assert.rejects(collectBundles(plan, Date.now()), /freshly built/);
});

test("Android/macOS configuration preserves platform boundaries", async () => {
  const android = JSON.parse(await fs.readFile(new URL("../src-tauri/tauri.android.conf.json", import.meta.url), "utf8"));
  const macos = JSON.parse(await fs.readFile(new URL("../src-tauri/tauri.macos.conf.json", import.meta.url), "utf8"));
  assert.equal(android.bundle.android.minSdkVersion, 24);
  assert.ok(!(android.build?.features || []).includes("linux-tray"));
  assert.equal(macos.bundle.macOS.minimumSystemVersion, "11.0");
});

test("application and ACL tests share the single embedded Tauri context", async () => {
  const lib = await fs.readFile(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
  const acl = await fs.readFile(new URL("../src-tauri/src/acl_regression_tests.rs", import.meta.url), "utf8");
  assert.equal((lib.match(/tauri::generate_context!/g) || []).length, 1);
  assert.match(lib, /\.build\(application_context\(\)\)/);
  assert.match(acl, /super::application_context\(\)/);
  assert.doesNotMatch(acl, /tauri::generate_context!/);
});
