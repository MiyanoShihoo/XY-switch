#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import * as fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { inflateRawSync } from "node:zlib";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const targets = Object.freeze({ aarch64: { rust: "aarch64-linux-android", abi: "arm64-v8a", arch: "arm64", machine: 183 }, x86_64: { rust: "x86_64-linux-android", abi: "x86_64", arch: "x86_64", machine: 62 } });

export function parseArgs(args) {
  const result = { targets: ["aarch64", "x86_64"], dryRun: false, instrumentation: false };
  const keys = { "--tools-dir": "toolsDir", "--out-dir": "outDir", "--targets": "targetList" };
  const seen = new Set();
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (seen.has(flag)) throw new Error(`Duplicate option: ${flag}`);
    seen.add(flag);
    if (flag === "--dry-run") { result.dryRun = true; continue; }
    if (flag === "--instrumentation") { result.instrumentation = true; continue; }
    const key = keys[flag];
    if (!key || !args[i + 1] || args[i + 1].startsWith("--")) throw new Error(`Invalid or incomplete option: ${flag}`);
    result[key] = args[++i];
  }
  if (result.targetList !== undefined) result.targets = result.targetList.split(",");
  if (!result.targets.length || new Set(result.targets).size !== result.targets.length || result.targets.some(t => !targets[t])) throw new Error("Android targets must be aarch64 and/or x86_64");
  return result;
}

function within(parent, child) { return child === parent || child.startsWith(parent + path.sep); }
export function planBuild(options, { projectRoot, host, version, env = {} }) {
  if (!["win32", "linux", "darwin"].includes(host)) throw new Error("Unsupported Android build host");
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error("Android requires a numeric x.y.z version");
  if (env.CARGO_BUILD_TARGET) throw new Error("Unset CARGO_BUILD_TARGET for multi-ABI Android builds");
  const toolsDir = path.resolve(projectRoot, options.toolsDir || env.XY_SWITCH_TOOLS_DIR || (host === "win32" ? "E:\\AI tools\\xy-switch\\android" : ".build-tools/android"));
  const outDir = path.resolve(projectRoot, options.outDir || `release/${version}/android-universal`);
  if (within(outDir, toolsDir) || within(toolsDir, outDir)) throw new Error("Tools and output directories must not overlap");
  if (within(path.join(projectRoot, "src"), toolsDir) || within(path.join(projectRoot, "src-tauri"), toolsDir) || within(path.join(projectRoot, ".git"), toolsDir)) throw new Error("Tools directory must not be inside public sources or Git metadata");
  const cli = path.join(projectRoot, "node_modules", "@tauri-apps", "cli", "tauri.js");
  return { projectRoot, toolsDir, outDir, version, targets: options.targets, abis: options.targets.map(t => targets[t].abi), cli,
    androidDir: path.join(projectRoot, "src-tauri", "gen", "android"),
    targetDir: path.resolve(projectRoot, env.CARGO_TARGET_DIR || "src-tauri/target"),
    args: [cli, "android", "build", "--ci", "--apk", "--target", ...options.targets, "--", "--locked"] };
}

function once(text, old, replacement) {
  if (text.split(old).length !== 2) throw new Error(`Unsupported generated Android template: ${old.slice(0, 70)}`);
  return text.replace(old, replacement);
}
export function patchManifest(text) {
  text = text.replace(/\r\n/g, "\n");
  if (text.includes("<!-- xy-switch-managed-android -->")) return text;
  if (/android:allowBackup=|android:fullBackupContent=/.test(text)) throw new Error("Review the existing backup policy before patching the Android manifest");
  return once(text, "    <application\n", '    <!-- xy-switch-managed-android -->\n    <application\n        android:allowBackup="false"\n        android:fullBackupContent="false"\n');
}
export function patchGradle(text, ndkVersion) {
  text = text.replace(/\r\n/g, "\n");
  if (text.includes("// xy-switch-managed-android")) return patchTestRuntimeDependency(text);
  if (!/^\d+\.\d+\.\d+$/.test(ndkVersion)) throw new Error("Invalid NDK version");
  if (/signingConfigs\s*\{|testBuildType\s*=/.test(text)) throw new Error("Refusing to replace custom Android signing or test configuration");
  text = once(text, "android {\n", `// xy-switch-managed-android\nandroid {\n    ndkVersion = "${ndkVersion}"\n    testBuildType = "release"\n    signingConfigs {\n        create("xyRelease") {\n            storeFile = file(System.getenv("XY_ANDROID_KEYSTORE_PATH") ?: error("Missing XY_ANDROID_KEYSTORE_PATH"))\n            storePassword = System.getenv("XY_ANDROID_STORE_PASSWORD") ?: error("Missing XY_ANDROID_STORE_PASSWORD")\n            keyAlias = System.getenv("XY_ANDROID_KEY_ALIAS") ?: error("Missing XY_ANDROID_KEY_ALIAS")\n            keyPassword = System.getenv("XY_ANDROID_KEY_PASSWORD") ?: error("Missing XY_ANDROID_KEY_PASSWORD")\n        }\n    }\n`);
  text = once(text, '        manifestPlaceholders["usesCleartextTraffic"] = "false"', '        // User-selected local/LAN HTTP endpoints must remain usable; no endpoint is contacted on startup.\n        manifestPlaceholders["usesCleartextTraffic"] = "true"\n        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"');
  text = once(text, '        getByName("release") {\n', '        getByName("release") {\n            signingConfig = signingConfigs.getByName("xyRelease")\n');
  text = once(text, '    androidTestImplementation("androidx.test.ext:junit:1.1.4")', '    androidTestImplementation("androidx.test:runner:1.6.2")\n    androidTestImplementation("androidx.test:core:1.6.1")\n    androidTestImplementation("androidx.test.ext:junit:1.2.1")');
  return patchTestRuntimeDependency(text);
}
function patchTestRuntimeDependency(text) {
  const marker = "// xy-switch-managed-test-runtime";
  const runtime = `    ${marker}
    // AndroidJUnitRunner 1.6.2 calls forceEnableAppTracing, added in tracing 1.1.0.
    // AGP excludes shared app dependencies from the test APK; align the app version too.
    implementation("androidx.tracing:tracing:1.1.0")
`;
  if (text.includes(marker) || /implementation\("androidx\.tracing:tracing:/.test(text)) {
    if (text.split(marker).length !== 2 || !text.includes(runtime) || text.split('implementation("androidx.tracing:tracing:').length !== 2) {
      throw new Error("Review the modified managed Android test runtime dependency");
    }
    return text;
  }
  return once(text, '    androidTestImplementation("androidx.test:runner:1.6.2")', runtime + '    androidTestImplementation("androidx.test:runner:1.6.2")');
}
export function patchProguard(text) {
  text = text.replace(/\r\n/g, "\n");
  const start = "# xy-switch-managed-instrumentation-start";
  const end = "# xy-switch-managed-instrumentation-end";
  const legacyRules = `${start}
# AndroidX test code runs in a separate APK and calls these Kotlin runtime methods.
# Keep their ABI stable: the test APK cannot follow R8 parameter rewriting/removal.
-keep class kotlin.jvm.internal.Intrinsics {
    public static <methods>;
}
${end}`;
  const rules = `${start}
# Instrumentation is a separate APK. AGP deduplicates these shared libraries, but
# app R8 cannot see their test-only callers. Keep the shared public/protected ABI,
# including Kotlin multifile facades; keepnames alone still removes methods.
# App/Tauri code and unrelated dependencies remain R8-optimized.
-keep class kotlin.** {
    public *;
    protected *;
}
-keep class kotlinx.coroutines.** {
    public *;
    protected *;
}
-keep class androidx.tracing.Trace {
    public static <methods>;
}
# concurrent-futures-ktx also calls package-private future glue.
-keep class androidx.concurrent.futures.** { *; }
-keep interface com.google.common.util.concurrent.ListenableFuture {
    public *;
}
-keep class androidx.lifecycle.Lifecycle$State { *; }
${end}`;
  if (text.includes(start) || text.includes(end)) {
    if (text.split(start).length !== 2 || text.split(end).length !== 2) {
      throw new Error("Review the modified managed Android instrumentation keep rules");
    }
    const block = text.slice(text.indexOf(start), text.indexOf(end) + end.length);
    if (block === rules) return text;
    if (block === legacyRules) return text.replace(legacyRules, rules);
    throw new Error("Review the modified managed Android instrumentation keep rules");
  }
  return text + (text.endsWith("\n") || !text ? "" : "\n") + "\n" + rules + "\n";
}
export function patchBuildTask(text, node, cli) {
  text = text.replace(/\r\n/g, "\n");
  if ([node, cli].some(value => /[\r\n"]/.test(value))) throw new Error("Unsafe Android runner path");
  if (!/val executable = .*;/.test(text) || !/val args = listOf\([^\n]+\);/.test(text)) throw new Error("Unsupported generated BuildTask");
  return text.replace(/val executable = .*;/, `val executable = """${node}""";`).replace(/val args = listOf\([^\n]+\);/, `val args = listOf("""${cli}""", "android", "android-studio-script");`);
}

export function inspectElf(bytes, expectedMachine) {
  if (bytes.length < 64 || bytes.subarray(0, 4).toString("hex") !== "7f454c46" || bytes[4] !== 2 || bytes[5] !== 1) throw new Error("Expected a little-endian 64-bit ELF library");
  if (bytes.readUInt16LE(18) !== expectedMachine) throw new Error("ELF architecture does not match the APK ABI");
  const offset = Number(bytes.readBigUInt64LE(32)), size = bytes.readUInt16LE(54), count = bytes.readUInt16LE(56);
  if (!Number.isSafeInteger(offset) || size < 56 || !count || offset + count * size > bytes.length) throw new Error("Invalid ELF program header bounds");
  const alignments = [];
  for (let n = 0; n < count; n++) {
    const p = offset + n * size;
    if (bytes.readUInt32LE(p) !== 1) continue;
    const alignment = bytes.readBigUInt64LE(p + 48), file = bytes.readBigUInt64LE(p + 8), virtual = bytes.readBigUInt64LE(p + 16);
    if (alignment < 16384n || (alignment & (alignment - 1n)) || file % alignment !== virtual % alignment) throw new Error("Native ELF is not 16 KB page-aligned");
    alignments.push(Number(alignment));
  }
  if (!alignments.length) throw new Error("ELF has no load segments");
  return { machine: expectedMachine, loadAlignments: alignments, pageSizeCompatible: 16384 };
}

export function inspectApkLibraries(zip, abis) {
  let end = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 65557); i--) if (zip.readUInt32LE(i) === 0x06054b50 && i + 22 + zip.readUInt16LE(i + 20) === zip.length) { end = i; break; }
  if (end < 0 || zip.readUInt16LE(end + 4) || zip.readUInt16LE(end + 6)) throw new Error("Invalid APK ZIP directory");
  const count = zip.readUInt16LE(end + 10), directorySize = zip.readUInt32LE(end + 12), directoryStart = zip.readUInt32LE(end + 16);
  if (count === 65535 || directorySize === 0xffffffff || directoryStart + directorySize > end) throw new Error("Unsupported APK ZIP64 or directory bounds");
  let cursor = directoryStart;
  const libraries = [], names = new Set();
  for (let n = 0; n < count; n++) {
    if (cursor + 46 > end || zip.readUInt32LE(cursor) !== 0x02014b50) throw new Error("Invalid ZIP central entry");
    const flags = zip.readUInt16LE(cursor + 8), method = zip.readUInt16LE(cursor + 10), compressed = zip.readUInt32LE(cursor + 20), plain = zip.readUInt32LE(cursor + 24), length = zip.readUInt16LE(cursor + 28), extra = zip.readUInt16LE(cursor + 30), comment = zip.readUInt16LE(cursor + 32), local = zip.readUInt32LE(cursor + 42);
    const name = zip.toString("utf8", cursor + 46, cursor + 46 + length);
    if (names.has(name) || name.startsWith("/") || name.includes("\\") || name.split("/").includes("..")) throw new Error("Unsafe or duplicate APK ZIP entry");
    names.add(name); cursor += 46 + length + extra + comment;
    if (cursor > directoryStart + directorySize) throw new Error("Invalid central directory length");
    if (!/^lib\/[^/]+\/[^/]+\.so$/.test(name)) continue;
    const abi = name.split("/")[1];
    if (!abis.includes(abi) || flags & 1 || ![0, 8].includes(method) || plain > 300_000_000 || local + 30 > directoryStart || zip.readUInt32LE(local) !== 0x04034b50) throw new Error("Unexpected native library entry");
    const start = local + 30 + zip.readUInt16LE(local + 26) + zip.readUInt16LE(local + 28);
    if (start + compressed > directoryStart) throw new Error("Native library ZIP entry is truncated");
    const content = method === 0 ? zip.subarray(start, start + compressed) : inflateRawSync(zip.subarray(start, start + compressed), { maxOutputLength: plain });
    if (content.length !== plain) throw new Error("Native library size mismatch");
    const machine = Object.values(targets).find(t => t.abi === abi).machine;
    libraries.push({ name, bytes: plain, sha256: createHash("sha256").update(content).digest("hex"), ...inspectElf(content, machine) });
  }
  if (cursor !== directoryStart + directorySize || abis.some(abi => !libraries.some(l => l.name === `lib/${abi}/libxy_switch_lib.so`))) throw new Error("APK is missing the newly built XY Switch library for an expected ABI");
  return libraries;
}

function run(command, args, env, capture = false) {
  const result = spawnSync(command, args, { cwd: root, env, shell: false, ...(capture ? { encoding: "utf8", maxBuffer: 12 * 1024 * 1024 } : { stdio: "inherit" }) });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${path.basename(command)} failed (${result.signal || result.status})${capture ? ": " + (result.stderr || "").slice(-1500) : ""}`);
  return capture ? result.stdout.trim() : undefined;
}
function javaTool(env, args) { return run(path.join(env.JAVA_HOME, "bin", process.platform === "win32" ? "java.exe" : "java"), args, env, true); }
export function prependEnvironmentPath(env, directories, delimiter = path.delimiter) {
  // Windows exposes Path, not necessarily PATH; a spread object loses process.env's case-insensitive lookup.
  const keys = Object.keys(env).filter(key => key.toUpperCase() === "PATH");
  const original = env.PATH ?? env.Path ?? env[keys[0]] ?? "";
  for (const key of keys) delete env[key];
  env.PATH = [...directories, original].filter(Boolean).join(delimiter);
  return env;
}
async function androidEnvironment(plan) {
  const env = { ...process.env };
  for (const name of ["JAVA_HOME", "ANDROID_HOME", "NDK_HOME"]) if (!env[name]) throw new Error(`Set ${name} to an existing toolchain (new Windows tools belong under E:\\AI tools)`);
  env.ANDROID_SDK_ROOT = env.ANDROID_HOME;
  env.GRADLE_USER_HOME ||= path.join(plan.toolsDir, "gradle");
  env.ANDROID_USER_HOME ||= path.join(plan.toolsDir, "android-user");
  const userHome = path.join(plan.toolsDir, "user-home");
  for (const dir of [userHome, env.GRADLE_USER_HOME, env.ANDROID_USER_HOME, path.join(plan.toolsDir, "tmp")]) await fs.mkdir(dir, { recursive: true });
  env.JAVA_TOOL_OPTIONS = `${env.JAVA_TOOL_OPTIONS || ""} -Duser.home="${userHome}"`.trim();
  prependEnvironmentPath(env, [path.join(env.JAVA_HOME, "bin"), path.join(env.ANDROID_HOME, "platform-tools")]);
  env.CARGO_TARGET_DIR = plan.targetDir;
  env.RUSTFLAGS = `${env.RUSTFLAGS || ""} -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384`.trim();
  env.ANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES = "ON";
  env.TMP = env.TEMP = path.join(plan.toolsDir, "tmp");
  return env;
}
export async function verifyWindowsSymlinkSupport(toolsDir, host = process.platform, io = fs) {
  if (host !== "win32") return;
  const directory = await io.mkdtemp(path.join(toolsDir, ".xy-symlink-check-"));
  const source = path.join(directory, "source"), link = path.join(directory, "link");
  try {
    await io.writeFile(source, "XY Switch Android build preflight\n", { flag: "wx" });
    await io.symlink(source, link, "file");
  } catch (error) {
    if (["EPERM", "EACCES"].includes(error.code)) {
      throw new Error("Tauri Android builds on Windows require file symbolic-link permission. Enable Windows Developer Mode yourself, use a permitted build account, or build on Linux/macOS. No system settings were changed.", { cause: error });
    }
    throw error;
  } finally {
    for (const file of [link, source]) {
      try { await io.unlink(file); } catch (error) { if (error.code !== "ENOENT") throw error; }
    }
    await io.rmdir(directory);
  }
}

async function configureSigning(plan, env) {
  const keys = ["XY_ANDROID_KEYSTORE_PATH", "XY_ANDROID_STORE_PASSWORD", "XY_ANDROID_KEY_ALIAS", "XY_ANDROID_KEY_PASSWORD"];
  const provided = keys.filter(key => env[key]);
  if (provided.length && provided.length !== keys.length) throw new Error("Supply all four XY_ANDROID signing variables, or none for an explicitly development-signed APK");
  if (provided.length) {
    const keyPath = await fs.realpath(env.XY_ANDROID_KEYSTORE_PATH);
    if (within(plan.projectRoot, keyPath)) throw new Error("Android signing keys must be outside the source workspace");
    env.XY_ANDROID_KEYSTORE_PATH = keyPath;
    return "external-release-key";
  }
  const signingDir = path.join(plan.toolsDir, "signing");
  await fs.mkdir(signingDir, { recursive: true, mode: 0o700 });
  const privateFile = path.join(signingDir, "development-private.json");
  let privateConfig;
  try { privateConfig = JSON.parse(await fs.readFile(privateFile, "utf8")); }
  catch (error) {
    if (error.code !== "ENOENT") throw error;
    privateConfig = { password: randomBytes(32).toString("hex"), alias: "xy-switch-development" };
    await fs.writeFile(privateFile, JSON.stringify(privateConfig), { flag: "wx", mode: 0o600 });
  }
  if (!/^[0-9a-f]{64}$/.test(privateConfig.password) || privateConfig.alias !== "xy-switch-development") throw new Error("Invalid existing development signing configuration");
  const key = path.join(signingDir, "development.p12");
  Object.assign(env, { XY_ANDROID_KEYSTORE_PATH: key, XY_ANDROID_STORE_PASSWORD: privateConfig.password, XY_ANDROID_KEY_PASSWORD: privateConfig.password, XY_ANDROID_KEY_ALIAS: privateConfig.alias });
  try { await fs.access(key); }
  catch (error) {
    if (error.code !== "ENOENT") throw error;
    run(path.join(env.JAVA_HOME, "bin", process.platform === "win32" ? "keytool.exe" : "keytool"), ["-genkeypair", "-keystore", key, "-storetype", "PKCS12", "-storepass:env", "XY_ANDROID_STORE_PASSWORD", "-keypass:env", "XY_ANDROID_KEY_PASSWORD", "-alias", privateConfig.alias, "-keyalg", "RSA", "-keysize", "3072", "-validity", "3650", "-dname", "CN=XY Switch Development, O=XY Switch"], env, true);
    await fs.chmod(key, 0o600);
  }
  return "development";
}

async function prepareProject(plan, env) {
  run("rustup", ["target", "add", ...plan.targets.map(t => targets[t].rust)], env);
  try { await fs.access(path.join(plan.androidDir, "app", "build.gradle.kts")); }
  catch (error) { if (error.code !== "ENOENT") throw error; run(process.execPath, [plan.cli, "android", "init", "--ci", "--skip-targets-install"], env); }
  const ndkVersion = path.basename(env.NDK_HOME.replace(/[\\/]+$/, ""));
  const manifest = path.join(plan.androidDir, "app", "src", "main", "AndroidManifest.xml");
  const gradle = path.join(plan.androidDir, "app", "build.gradle.kts");
  const proguard = path.join(plan.androidDir, "app", "proguard-rules.pro");
  await fs.writeFile(manifest, patchManifest(await fs.readFile(manifest, "utf8")));
  await fs.writeFile(gradle, patchGradle(await fs.readFile(gradle, "utf8"), ndkVersion));
  await fs.writeFile(proguard, patchProguard(await fs.readFile(proguard, "utf8")));
  const buildTasks = [];
  async function walk(dir) { for (const entry of await fs.readdir(dir, { withFileTypes: true })) { const p = path.join(dir, entry.name); if (entry.isDirectory()) await walk(p); else if (entry.name === "BuildTask.kt") buildTasks.push(p); } }
  await walk(path.join(plan.androidDir, "buildSrc", "src"));
  if (buildTasks.length !== 1) throw new Error("Expected one generated Android BuildTask");
  await fs.writeFile(buildTasks[0], patchBuildTask(await fs.readFile(buildTasks[0], "utf8"), process.execPath, plan.cli));
  const testDir = path.join(plan.androidDir, "app", "src", "androidTest", "java", "com", "xyswitch", "desktop");
  await fs.mkdir(testDir, { recursive: true });
  await fs.copyFile(path.join(root, "src-tauri", "android-tests", "NativeAppTest.kt"), path.join(testDir, "NativeAppTest.kt"));
}

export function parseBadging(output, { identifier, version }) {
  const packageLine = output.match(/^package: name='([^']+)' versionCode='(\d+)' versionName='([^']+)'/m);
  // Build Tools 36 uses minSdkVersion; older aapt2 versions printed sdkVersion.
  const minima = [...output.matchAll(/^(?:minSdkVersion|sdkVersion):'(\d+)'\s*$/gm)];
  const targets = [...output.matchAll(/^targetSdkVersion:'(\d+)'\s*$/gm)];
  const min = minima.length === 1 ? minima[0] : null, target = targets.length === 1 ? targets[0] : null;
  if (!packageLine || packageLine[1] !== identifier || packageLine[3] !== version || !min || +min[1] !== 24 || !target || +target[1] !== 36) {
    throw new Error(`APK package/version/SDK metadata does not match the build configuration: ${JSON.stringify({ identifier: packageLine?.[1], version: packageLine?.[3], minSdk: min?.[1], targetSdk: target?.[1] })}`);
  }
  if (/^application-debuggable$/m.test(output)) throw new Error("Release APK is unexpectedly debuggable");
  return { identifier: packageLine[1], versionCode: +packageLine[2], versionName: packageLine[3], minSdk: +min[1], targetSdk: +target[1], debuggable: false };
}
export function verifyManifestPolicies(xml) {
  const lines = xml.replace(/\r\n/g, "\n").split("\n");
  const applications = lines.flatMap((line, index) => /^\s*E: application(?:\s|$)/.test(line) ? [index] : []);
  if (applications.length !== 1) throw new Error("Expected one APK application element");
  const attributes = [];
  for (const line of lines.slice(applications[0] + 1)) {
    if (/^\s*E: /.test(line)) break;
    attributes.push(line);
  }
  function boolean(name) {
    const pattern = new RegExp(`^\\s*A: (?:android:|http://schemas\\.android\\.com/apk/res/android:)${name}(?:\\(0x[0-9a-f]+\\))?\\s*=\\s*(.+?)\\s*$`, "i");
    const matches = attributes.map(line => line.match(pattern)).filter(Boolean);
    if (matches.length !== 1) throw new Error(`Missing or ambiguous APK application attribute: ${name}`);
    const value = matches[0][1].toLowerCase();
    if (value === "true" || value === "(type 0x12)0xffffffff") return true;
    if (value === "false" || value === "(type 0x12)0x0") return false;
    throw new Error(`Unexpected APK boolean representation: ${name}`);
  }
  const allowBackup = boolean("allowBackup"), usesCleartextTraffic = boolean("usesCleartextTraffic");
  if (allowBackup || !usesCleartextTraffic) throw new Error("APK backup/HTTP endpoint policies were not applied");
  return { allowBackup, usesCleartextTraffic };
}
export function matchNativeLibraryInput(packagedSha256, inputBytes, strippedBytes) {
  const packagingInputSha256 = createHash("sha256").update(inputBytes).digest("hex");
  if (packagedSha256 === packagingInputSha256) return { packagingInputSha256, packagingTransform: "none" };
  if (strippedBytes && packagedSha256 === createHash("sha256").update(strippedBytes).digest("hex")) {
    return { packagingInputSha256, packagingTransform: "ndk-strip-unneeded" };
  }
  throw new Error("APK native library differs from both the Gradle input and independently stripped input");
}
export async function verifyApk(apk, plan, env, identifier) {
  const buildTools = path.join(env.ANDROID_HOME, "build-tools", "36.0.0");
  const suffix = process.platform === "win32" ? ".exe" : "";
  const metadata = parseBadging(run(path.join(buildTools, "aapt2" + suffix), ["dump", "badging", apk], env, true), { identifier, version: plan.version });
  const xml = run(path.join(buildTools, "aapt2" + suffix), ["dump", "xmltree", "--file", "AndroidManifest.xml", apk], env, true);
  verifyManifestPolicies(xml);
  run(path.join(buildTools, "zipalign" + suffix), ["-c", "-P", "16", "4", apk], env, true);
  const signer = javaTool(env, ["-jar", path.join(buildTools, "lib", "apksigner.jar"), "verify", "--verbose", "--print-certs", apk]);
  const fingerprints = [...signer.matchAll(/Signer #\d+ certificate SHA-256 digest: ([0-9a-f]{64})/gi)].map(match => match[1].toLowerCase());
  if (fingerprints.length !== 1 || !/Verified using v2 scheme[^\n]*true/.test(signer)) throw new Error("APK signature verification did not confirm one v2 signer");
  const bytes = await fs.readFile(apk), libraries = inspectApkLibraries(bytes, plan.abis);
  // The APK must contain these just-built libraries, not a stale cached ABI.
  for (const target of plan.targets) {
    const lib = libraries.find(lib => lib.name === `lib/${targets[target].abi}/libxy_switch_lib.so`);
    const source = path.join(plan.androidDir, "app", "src", "main", "jniLibs", targets[target].abi, "libxy_switch_lib.so");
    const expected = await fs.readFile(source);
    let stripped;
    if (createHash("sha256").update(expected).digest("hex") !== lib.sha256) {
      // AGP strips native libraries. Reproduce only that exact transformation with the pinned NDK;
      // accepting a different binary or comparing just its architecture would mask stale payloads.
      const host = { win32: "windows-x86_64", linux: "linux-x86_64", darwin: "darwin-x86_64" }[process.platform];
      if (!host || !env.NDK_HOME) throw new Error("An NDK host strip tool is required for native payload verification");
      const temporary = await fs.mkdtemp(path.join(plan.toolsDir, ".xy-native-payload-"));
      const output = path.join(temporary, "stripped.so");
      try {
        run(path.join(env.NDK_HOME, "toolchains", "llvm", "prebuilt", host, "bin", "llvm-strip" + suffix), ["--strip-unneeded", "-o", output, source], env, true);
        stripped = await fs.readFile(output);
        if (!expected.equals(await fs.readFile(source))) throw new Error("Native packaging input changed during verification");
      } finally {
        try { await fs.unlink(output); } catch (error) { if (error.code !== "ENOENT") throw error; }
        await fs.rmdir(temporary);
      }
    }
    Object.assign(lib, matchNativeLibraryInput(lib.sha256, expected, stripped));
  }
  return { ...metadata, allowBackup: false, cleartextEndpoints: "explicitly supported for user-selected local/LAN HTTP endpoints", signature: { verified: true, scheme: "v2-or-later", certificateSha256: fingerprints[0] }, zipAlignment: "16 KB", libraries, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}
async function freshApks(directory, startedAt) {
  const result = [];
  async function walk(dir) { for (const entry of await fs.readdir(dir, { withFileTypes: true })) { const p = path.join(dir, entry.name); if (entry.isDirectory()) await walk(p); else if (entry.name.endsWith(".apk")) { const stat = await fs.stat(p); if (stat.size && stat.mtimeMs >= startedAt - 2000) result.push(p); } } }
  await walk(directory); return result;
}

export async function main(args = process.argv.slice(2)) {
  const options = parseArgs(args);
  const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
  const config = JSON.parse(await fs.readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
  if (pkg.version !== config.version) throw new Error("Package and Tauri versions differ");
  const plan = planBuild(options, { projectRoot: root, host: process.platform, version: pkg.version, env: process.env });
  if (options.dryRun) { console.log(JSON.stringify(plan, null, 2)); return; }
  const env = await androidEnvironment(plan);
  await verifyWindowsSymlinkSupport(plan.toolsDir);
  const signing = await configureSigning(plan, env);
  await prepareProject(plan, env);
  const startedAt = Date.now();
  run(process.execPath, plan.args, env);
  const apks = await freshApks(path.join(plan.androidDir, "app", "build", "outputs", "apk", "universal", "release"), startedAt);
  if (apks.length !== 1) throw new Error(`Expected exactly one newly built universal release APK; found ${apks.length}`);
  const verified = await verifyApk(apks[0], plan, env, config.identifier);
  await fs.mkdir(plan.outDir, { recursive: true });
  const name = `XY-Switch-${plan.version}-android-universal${signing === "development" ? "-development-signed" : ""}.apk`;
  const destination = path.join(plan.outDir, name);
  await fs.copyFile(apks[0], destination);
  if (createHash("sha256").update(await fs.readFile(destination)).digest("hex") !== verified.sha256) throw new Error("Copied APK checksum mismatch");
  const manifest = { platform: "android", architecture: "universal", abis: plan.abis, version: plan.version, builtAt: new Date().toISOString(), signing, nativeVerification: verified, files: [{ name, format: "apk", bytes: verified.bytes, sha256: verified.sha256 }] };
  await fs.writeFile(path.join(plan.outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  await fs.writeFile(path.join(plan.outDir, "SHA256SUMS.txt"), `${verified.sha256}  ${name}\n`);
  await fs.writeFile(path.join(plan.outDir, "BUILD-TYPE.txt"), (signing === "development" ? "Development-signed Android APK. Not a Google Play release. CI uses a disposable signing key; do not assume updates from other builds share this certificate.\n" : "Signed with an externally supplied Android release key. APK signatures independently verified; no store publication performed.\n") + "Supports Android 7.0/API 24+, ARM64 and/or x86_64 as listed in manifest.json. Native libraries and APK alignment checked for 16 KB pages. Mobile cannot manage other apps' desktop CLI files; background proxy lifetime is not guaranteed.\n");
  if (options.instrumentation) {
    const properties = [`-PabiList=${plan.abis.join(",")}`, `-ParchList=${plan.targets.map(t => targets[t].arch).join(",")}`, `-PtargetList=${plan.targets.join(",")}`];
    const excludes = plan.targets.flatMap(t => ["-x", `rustBuild${targets[t].arch[0].toUpperCase() + targets[t].arch.slice(1)}Release`]);
    // Invoke the wrapper JAR directly to avoid shell quoting and spaces in tool paths.
    javaTool(env, ["-classpath", path.join(plan.androidDir, "gradle", "wrapper", "gradle-wrapper.jar"), "org.gradle.wrapper.GradleWrapperMain", "-p", plan.androidDir, "--no-daemon", ":app:assembleUniversalReleaseAndroidTest", ":tauri-plugin-xy-credentials:assembleDebugAndroidTest", ...properties, ...excludes]);
    const testOutput = path.join(plan.outDir, "verification", "test-apks");
    await fs.mkdir(testOutput, { recursive: true });
    const testApks = await freshApks(path.join(plan.androidDir, "app", "build", "outputs", "apk", "androidTest", "universal", "release"), startedAt);
    if (testApks.length !== 1) throw new Error("Expected one release instrumentation APK");
    await fs.copyFile(testApks[0], path.join(testOutput, "app-test.apk"));
    const libTests = await freshApks(path.join(root, "src-tauri", "plugins", "tauri-plugin-xy-credentials", "android", "build", "outputs", "apk", "androidTest", "debug"), startedAt);
    if (libTests.length !== 1) throw new Error("Expected one Keystore library instrumentation APK");
    await fs.copyFile(libTests[0], path.join(testOutput, "keystore-test.apk"));
  }
  console.log(JSON.stringify({ output: plan.outDir, platform: "android", signing, abis: plan.abis, signatureVerified: true, files: manifest.files }, null, 2));
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
