import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { createHash } from "node:crypto";
import { parseArgs, planBuild, patchManifest, patchGradle, patchProguard, patchBuildTask, inspectElf, inspectApkLibraries, parseBadging, verifyManifestPolicies, matchNativeLibraryInput, prependEnvironmentPath, verifyWindowsSymlinkSupport } from "./package-android.mjs";
const projectRoot = path.resolve("fixture-project");
const context = { projectRoot, host: "linux", version: "1.0.0", env: {} };
test("Android default plan builds ARM64 and x86_64 release APKs with the Cargo lock", () => {
  const plan = planBuild(parseArgs([]), context);
  assert.deepEqual(plan.abis, ["arm64-v8a", "x86_64"]);
  assert.deepEqual(plan.args.slice(1), ["android", "build", "--ci", "--apk", "--target", "aarch64", "x86_64", "--", "--locked"]);
});
test("Android options are validated without silently ignoring duplicates", () => {
  assert.deepEqual(parseArgs(["--targets", "aarch64"]).targets, ["aarch64"]);
  for (const args of [["--targets", "armv7"], ["--targets", "aarch64,aarch64"], ["--out-dir"], ["--dry-run", "--dry-run"], ["--unknown"]]) assert.throws(() => parseArgs(args));
});
test("Android rejects mixed output/tool paths, cross target overrides and invalid versions", () => {
  assert.throws(() => planBuild(parseArgs(["--tools-dir", "release/1.0.0/android-universal/tools"]), context), /overlap/);
  assert.throws(() => planBuild(parseArgs(["--tools-dir", "src-tauri/tools"]), context), /public sources/);
  assert.throws(() => planBuild(parseArgs([]), { ...context, version: "dev" }));
  assert.throws(() => planBuild(parseArgs([]), { ...context, env: { CARGO_BUILD_TARGET: "x86_64" } }));
});
test("Android manifest disables platform backups without changing unrelated permissions", () => {
  const manifest = '    <application\n        android:usesCleartextTraffic="${usesCleartextTraffic}">';
  const patched = patchManifest(manifest);
  assert.match(patched, /android:allowBackup="false"/); assert.match(patched, /android:fullBackupContent="false"/);
  assert.equal(patchManifest(patched), patched);
  assert.throws(() => patchManifest('    <application\n android:allowBackup="true">'), /backup policy/);
});
test("Android Gradle signing uses only environment variables and preserves release shrinking", () => {
  const template = 'android {\n    defaultConfig {\n        manifestPlaceholders["usesCleartextTraffic"] = "false"\n    }\n    buildTypes {\n        getByName("release") {\n            isMinifyEnabled = true\n        }\n    }\n}\ndependencies {\n    androidTestImplementation("androidx.test.ext:junit:1.1.4")\n}';
  const patched = patchGradle(template, "27.2.12479018");
  assert.match(patched, /testBuildType = "release"/); assert.match(patched, /isMinifyEnabled = true/);
  assert.match(patched, /System.getenv\("XY_ANDROID_STORE_PASSWORD"\)/); assert.match(patched, /usesCleartextTraffic"\] = "true"/);
  assert.equal(patchGradle(patched, "27.2.12479018"), patched);
  assert.throws(() => patchGradle(template + '\nsigningConfigs {', "27.2.12479018"));
});
test("Android release keeps only the cross-APK Intrinsics ABI without disabling R8", () => {
  const original = "# Existing project rules\n-keep class fixture.NativeBridge { native <methods>; }\n";
  const patched = patchProguard(original);
  assert.ok(patched.startsWith(original));
  assert.match(patched, /-keep class kotlin\.jvm\.internal\.Intrinsics \{\n    public static <methods>;\n\}/);
  assert.doesNotMatch(patched, /-dont(?:optimize|obfuscate|shrink)|allowoptimization|class kotlin\.\*|class \*\*/);
  assert.equal(patchProguard(patched), patched);
  assert.equal(patchProguard(patched.replace(/\n/g, "\r\n")), patched);
});
test("Android refuses changed, partial or duplicated managed instrumentation rules", () => {
  const patched = patchProguard("");
  for (const broken of [patched.replace("public static <methods>;", ""), patched.replace("# xy-switch-managed-instrumentation-end", ""), patched + patched]) {
    assert.throws(() => patchProguard(broken), /managed Android instrumentation keep rules/);
  }
});
test("Android project preparation applies the tested release keep rules", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(new URL("./package-android.mjs", import.meta.url), "utf8");
  assert.match(source, /const proguard = path\.join\(plan\.androidDir, "app", "proguard-rules\.pro"\)/);
  assert.match(source, /await fs\.writeFile\(proguard, patchProguard\(await fs\.readFile\(proguard, "utf8"\)\)\)/);
});
test("Android Gradle Rust runner invokes the actual Node CLI file, not node tauri", () => {
  const result = patchBuildTask('val executable = "node";\nval args = listOf("tauri", "android", "android-studio-script");', "/tools/node", "/project/node_modules/@tauri-apps/cli/tauri.js");
  assert.match(result, /\/project\/node_modules\/@tauri-apps\/cli\/tauri.js/);
  assert.throws(() => patchBuildTask(result, 'bad"path', "/cli"));
});
function elf(align = 16384n, machine = 183) {
  const b = Buffer.alloc(120); b.write("7f454c46", 0, "hex"); b[4] = 2; b[5] = 1;
  b.writeUInt16LE(machine, 18); b.writeBigUInt64LE(64n, 32); b.writeUInt16LE(56, 54); b.writeUInt16LE(1, 56);
  b.writeUInt32LE(1, 64); b.writeBigUInt64LE(align, 112); return b;
}
test("Android ELF validation detects 4 KB, bad ABI and truncated binaries", () => {
  assert.equal(inspectElf(elf(), 183).pageSizeCompatible, 16384);
  for (const binary of [elf(4096n), elf(16384n, 62), elf().subarray(0, 70)]) assert.throws(() => inspectElf(binary, 183));
  const mismatch = elf(); mismatch.writeBigUInt64LE(1n, 72); assert.throws(() => inspectElf(mismatch, 183));
});
test("Android APK rejects malformed ZIPs and missing ABI libraries", () => {
  assert.throws(() => inspectApkLibraries(Buffer.alloc(100), ["arm64-v8a"]));
  const empty = Buffer.alloc(22); empty.writeUInt32LE(0x06054b50); assert.throws(() => inspectApkLibraries(empty, ["arm64-v8a"]), /missing/);
});
test("Android manifest metadata must match identifier, version and SDK", () => {
  const good = "package: name='com.xyswitch.desktop' versionCode='1000000' versionName='1.0.0' platformBuildVersionName='16'\nsdkVersion:'24'\ntargetSdkVersion:'36'";
  const expected = { identifier: "com.xyswitch.desktop", version: "1.0.0" };
  assert.equal(parseBadging(good, expected).minSdk, 24);
  for (const bad of [good.replace("1.0.0", "0.9.0"), good.replace("sdkVersion:'24'", "sdkVersion:'21'"), good + "\napplication-debuggable"]) assert.throws(() => parseBadging(bad, expected));
});

test("Android Windows environment preserves the inherited Path and avoids duplicate case keys", () => {
  const env = { Path: "C:\\Windows;E:\\Rust\\.cargo\\bin", OTHER: "fixture" };
  prependEnvironmentPath(env, ["E:\\AI tools\\jdk\\bin"], ";");
  assert.equal(env.PATH, "E:\\AI tools\\jdk\\bin;C:\\Windows;E:\\Rust\\.cargo\\bin");
  assert.equal(env.Path, undefined);
  assert.equal(env.OTHER, "fixture");
  assert.equal(prependEnvironmentPath({ PATH: "/bin" }, ["/sdk"], ":").PATH, "/sdk:/bin");
});

test("Android patches normalize CRLF generated by Windows tooling", () => {
  assert.match(patchManifest('    <application\r\n        android:label="fixture">'), /allowBackup="false"/);
  const gradle = 'android {\r\n    defaultConfig {\r\n        manifestPlaceholders["usesCleartextTraffic"] = "false"\r\n    }\r\n    buildTypes {\r\n        getByName("release") {\r\n        }\r\n    }\r\n}\r\n    androidTestImplementation("androidx.test.ext:junit:1.1.4")';
  assert.match(patchGradle(gradle, "27.2.12479018"), /signingConfigs/);
});

test("Android Windows preflight is not required on Linux or macOS", async () => {
  const io = new Proxy({}, { get() { throw new Error("Unexpected filesystem access"); } });
  await verifyWindowsSymlinkSupport("tools", "linux", io);
  await verifyWindowsSymlinkSupport("tools", "darwin", io);
});
test("Android Windows preflight checks a file symlink and removes only its own files", async () => {
  const calls = [], directory = path.resolve("tools", ".xy-symlink-check-fixture");
  const io = {
    async mkdtemp(prefix) { assert.match(prefix, /\.xy-symlink-check-$/); return directory; },
    async writeFile(file) { calls.push(["write", file]); },
    async symlink(source, link, kind) { calls.push(["link", source, link, kind]); },
    async unlink(file) { calls.push(["unlink", file]); },
    async rmdir(file) { calls.push(["rmdir", file]); },
  };
  await verifyWindowsSymlinkSupport("tools", "win32", io);
  assert.deepEqual(calls, [
    ["write", path.join(directory, "source")],
    ["link", path.join(directory, "source"), path.join(directory, "link"), "file"],
    ["unlink", path.join(directory, "link")], ["unlink", path.join(directory, "source")], ["rmdir", directory],
  ]);
});
test("Android Windows preflight reports missing permission without changing system settings", async () => {
  let cleaned = false;
  const permission = Object.assign(new Error("symlink denied"), { code: "EPERM" });
  const io = {
    async mkdtemp() { return "fixture"; }, async writeFile() {},
    async symlink() { throw permission; }, async unlink() {}, async rmdir() { cleaned = true; },
  };
  await assert.rejects(verifyWindowsSymlinkSupport("tools", "win32", io), error => {
    assert.match(error.message, /symbolic-link permission/);
    assert.match(error.message, /No system settings were changed/);
    assert.equal(error.cause, permission);
    return true;
  });
  assert.equal(cleaned, true);
});

test("Build Tools 36 minSdkVersion spelling is accepted without accepting ambiguous metadata", () => {
  const expected = { identifier: "com.xyswitch.desktop", version: "1.0.0" };
  const actual = "package: name='com.xyswitch.desktop' versionCode='1000000' versionName='1.0.0' platformBuildVersionName='16' platformBuildVersionCode='36' compileSdkVersion='36' compileSdkVersionCodename='16'\nminSdkVersion:'24'\ntargetSdkVersion:'36'\n";
  assert.deepEqual(parseBadging(actual, expected), { identifier: expected.identifier, versionCode: 1000000, versionName: expected.version, minSdk: 24, targetSdk: 36, debuggable: false });
  for (const bad of [actual + "sdkVersion:'24'\n", actual + "minSdkVersion:'21'\n", actual + "targetSdkVersion:'35'\n", actual.replace("minSdkVersion:'24'", "minSdkVersion:'23'")]) {
    assert.throws(() => parseBadging(bad, expected), /metadata/);
  }
});

test("Android APK payload must exactly match the input or its independently reproduced strip output", () => {
  const raw = Buffer.from("fixture native input"), stripped = Buffer.from("fixture stripped output");
  const hash = value => createHash("sha256").update(value).digest("hex");
  assert.deepEqual(matchNativeLibraryInput(hash(raw), raw), { packagingInputSha256: hash(raw), packagingTransform: "none" });
  assert.deepEqual(matchNativeLibraryInput(hash(stripped), raw, stripped), { packagingInputSha256: hash(raw), packagingTransform: "ndk-strip-unneeded" });
  assert.throws(() => matchNativeLibraryInput(hash(stripped), raw), /differs/);
  assert.throws(() => matchNativeLibraryInput(hash(Buffer.from("stale native library")), raw, stripped), /differs/);
});

test("APK application policies accept actual Build Tools 36 XML output and older typed booleans", () => {
  const xml = `N: android=http://schemas.android.com/apk/res/android (line=1)
  E: manifest (line=1)
      E: application (line=1)
        A: http://schemas.android.com/apk/res/android:allowBackup(0x01010280)=false
        A: http://schemas.android.com/apk/res/android:usesCleartextTraffic(0x010104ec)=true
        E: activity (line=1)
          A: android:name=".MainActivity"
`;
  assert.deepEqual(verifyManifestPolicies(xml), { allowBackup: false, usesCleartextTraffic: true });
  assert.deepEqual(verifyManifestPolicies(xml.replaceAll("http://schemas.android.com/apk/res/android:", "android:").replace("=false", "=(type 0x12)0x0").replace("=true", "=(type 0x12)0xffffffff")), { allowBackup: false, usesCleartextTraffic: true });
  for (const bad of [xml.replace("=false", "=true"), xml.replace("=true", "=false"), xml.replace("=false", '="false"'), xml.replace("E: application", "E: activity"), xml.replace("        E: activity", "        A: android:allowBackup(0x01010280)=false\n        E: activity"), xml.replace("        A: http://schemas.android.com/apk/res/android:allowBackup", "        E: activity\n          A: http://schemas.android.com/apk/res/android:allowBackup")]) {
    assert.throws(() => verifyManifestPolicies(bad));
  }
});
