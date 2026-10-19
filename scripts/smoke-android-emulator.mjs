#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import * as fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { inspectNativeScreenshot } from "./verify-native-screenshot.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export function parseArgs(args) {
  const result = {}, names = { "--serial": "serial", "--out-dir": "outDir", "--adb": "adb" };
  for (let n = 0; n < args.length; n++) {
    const name = names[args[n]], value = args[++n];
    if (!name || !value || value.startsWith("--") || result[name]) throw new Error("Expected unique --serial, --out-dir and/or --adb options");
    result[name] = value;
  }
  if (!/^emulator-\d+$/.test(result.serial || "")) throw new Error("An explicit Android emulator serial is required; physical devices are never accepted");
  return result;
}
export function assertEmulatorIdentity(serial, qemu, avdName) {
  if (!/^emulator-\d+$/.test(serial) || qemu.trim() !== "1" || !/^xy-switch-ci-[A-Za-z0-9_-]+$/.test(avdName.trim())) {
    throw new Error("Refusing installation: use a disposable emulator named xy-switch-ci-<unique-id>");
  }
}
export function parseInstrumentation(text, expected) {
  const summaries = [...text.matchAll(/^OK \((\d+) tests?\)\r?$/gm)];
  const codes = [...text.matchAll(/^INSTRUMENTATION_CODE: (-?\d+)\r?$/gm)];
  const ok = summaries[0];
  if (!Number.isSafeInteger(expected) || expected < 1 || summaries.length !== 1 || +ok[1] !== expected || codes.length !== 1 || +codes[0][1] !== -1 || /FAILURES!!!|INSTRUMENTATION_FAILED|INSTRUMENTATION_RESULT: shortMsg|Process crashed|^Runner error:/m.test(text)) {
    throw new Error(`Android instrumentation did not pass exactly ${expected} tests`);
  }
  return { passed: true, tests: +ok[1] };
}
export function execute(command, args, { bytes = false, timeout = 240000, allowFailure = false, spawn = spawnSync } = {}) {
  const result = spawn(command, args, { cwd: root, shell: false, encoding: bytes ? undefined : "utf8", maxBuffer: 32 * 1024 * 1024, timeout, windowsHide: true });
  if (result.error || (!allowFailure && result.status !== 0)) {
    const error = result.error || new Error(`${path.basename(command)} failed (${result.status}): ${String(result.stderr || "").slice(-1200)}`);
    error.stdout = String(result.stdout || "");
    error.stderr = String(result.stderr || "");
    throw error;
  }
  return bytes ? result.stdout : (result.stdout || "").trim();
}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

export function parseEmulatorSnapshot(cpu, memory) {
  const line = cpu.split(/\r?\n/).find(value => /^cpu\s/.test(value));
  const fields = line?.trim().split(/\s+/).slice(1, 9) || [];
  const available = /^MemAvailable:\s+(\d+)\s+kB\s*$/m.exec(memory);
  if (fields.length !== 8 || fields.some(value => !/^\d+$/.test(value) || !Number.isSafeInteger(+value)) || !available || !Number.isSafeInteger(+available[1])) {
    throw new Error("Unrecognized disposable emulator CPU/memory counters");
  }
  return { ticks: fields.map(Number), availableKiB: +available[1] };
}
export function emulatorHealth(previous, current) {
  const delta = current.ticks.map((value, index) => value - previous.ticks[index]);
  const total = delta.reduce((sum, value) => sum + value, 0);
  if (delta.some(value => value < 0) || !Number.isSafeInteger(total) || total <= 0) throw new Error("Disposable emulator CPU counters reset or stopped");
  // Linux counts guest time in user/nice already. Only the first eight fields are summed.
  return { busyFraction: (total - delta[3] - delta[4]) / total, availableMemoryMiB: current.availableKiB / 1024 };
}
export async function waitForEmulatorReady(run, { sleep = delay, now = Date.now, timeout = 180000, interval = 5000, stableSamples = 3 } = {}) {
  const read = () => parseEmulatorSnapshot(run(["shell", "cat", "/proc/stat"]), run(["shell", "cat", "/proc/meminfo"]));
  const started = now(), samples = [];
  let previous = read(), stable = 0;
  while (now() - started < timeout) {
    await sleep(Math.min(interval, timeout - (now() - started)));
    const current = read(), health = emulatorHealth(previous, current);
    previous = current;
    samples.push({ elapsedMs: now() - started, ...health });
    stable = health.busyFraction <= 0.8 && health.availableMemoryMiB >= 512 ? stable + 1 : 0;
    if (stable >= stableSamples) return { passed: true, maxBusyFraction: 0.8, minAvailableMemoryMiB: 512, stableSamples, samples };
  }
  const error = new Error("Disposable emulator did not settle after boot; refusing to run app tests under sustained CPU/memory pressure");
  error.report = { passed: false, maxBusyFraction: 0.8, minAvailableMemoryMiB: 512, stableSamples, samples };
  throw error;
}
export async function captureEmulatorDiagnostics(run, directory, write = fs.writeFile) {
  const commands = [
    ["emulator-logcat.txt", ["logcat", "-d", "-b", "all", "-t", "20000"]],
    ["emulator-last-anr.txt", ["shell", "dumpsys", "activity", "lastanr"]],
    ["emulator-anr-traces.txt", ["shell", "dumpsys", "dropbox", "--print", "data_app_anr"]],
    ["emulator-crashes.txt", ["shell", "dumpsys", "dropbox", "--print", "data_app_crash"]],
  ];
  for (const [name, args] of commands) {
    let output;
    try { output = run(args, { allowFailure: true, timeout: 60000 }); }
    catch (error) { output = [error.stdout, error.stderr, `Diagnostic error: ${error.message}`].filter(Boolean).join("\n"); }
    await write(path.join(directory, name), output + "\n");
  }
}
export async function main(args = process.argv.slice(2)) {
  const options = parseArgs(args);
  const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
  const config = JSON.parse(await fs.readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
  const output = path.resolve(root, options.outDir || `release/${pkg.version}/android-universal`);
  const sdk = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT;
  const adb = options.adb || (sdk && path.join(sdk, "platform-tools", process.platform === "win32" ? "adb.exe" : "adb"));
  if (!adb) throw new Error("Set ANDROID_HOME or pass --adb");
  const run = (args, settings) => execute(adb, ["-s", options.serial, ...args], settings);
  const qemu = run(["shell", "getprop", "ro.kernel.qemu"]) || run(["shell", "getprop", "ro.boot.qemu"]);
  const avd = run(["emu", "avd", "name"]).split(/\r?\n/)[0];
  assertEmulatorIdentity(options.serial, qemu, avd);
  if (run(["shell", "getprop", "sys.boot_completed"]) !== "1") throw new Error("Disposable emulator has not finished booting");
  const manifest = JSON.parse(await fs.readFile(path.join(output, "manifest.json"), "utf8"));
  if (manifest.platform !== "android" || manifest.version !== pkg.version || manifest.files.length !== 1) throw new Error("Unexpected Android release manifest");
  const entry = manifest.files[0];
  if (path.basename(entry.name) !== entry.name || !entry.name.endsWith(".apk")) throw new Error("Unsafe APK filename");
  const apk = path.join(output, entry.name), bytes = await fs.readFile(apk);
  if (bytes.length !== entry.bytes || createHash("sha256").update(bytes).digest("hex") !== entry.sha256) throw new Error("Final APK checksum differs from its manifest");
  const verification = path.join(output, "verification");
  await fs.mkdir(verification, { recursive: true });
  const identifier = config.identifier, libraryTest = "com.xyswitch.credentials.test", appTest = identifier + ".test";
  const installed = [];
  try {
    // These operations are reachable only after checking both QEMU and our disposable AVD name.
    for (const [id, file] of [[identifier, apk], [libraryTest, path.join(verification, "test-apks", "keystore-test.apk")], [appTest, path.join(verification, "test-apks", "app-test.apk")]]) {
      const result = run(["install", "-r", file]);
      if (!/\bSuccess\b/.test(result)) throw new Error(`APK installation failed for ${id}`);
      installed.push(id);
    }
    const instrumentations = run(["shell", "pm", "list", "instrumentation"]);
    await fs.writeFile(path.join(verification, "instrumentations.txt"), instrumentations + "\n");
    const cases = [
      { id: libraryTest, target: libraryTest, className: "com.xyswitch.credentials.AndroidCredentialStoreTest", name: "keystore", count: 4 },
      { id: appTest, target: identifier, className: "com.xyswitch.desktop.NativeAppTest", name: "release-app", count: 1 },
    ];
    // A boot-completed property alone is not readiness: fresh Google API images still
    // initialize services and dex files, which can starve the app's startup process.
    try {
      const readiness = await waitForEmulatorReady(run);
      await fs.writeFile(path.join(verification, "emulator-readiness.json"), JSON.stringify(readiness, null, 2) + "\n");
    } catch (error) {
      if (error.report) await fs.writeFile(path.join(verification, "emulator-readiness.json"), JSON.stringify(error.report, null, 2) + "\n");
      throw error;
    }
    const tests = {};
    for (const test of cases) {
      const component = `${test.id}/androidx.test.runner.AndroidJUnitRunner`;
      if (!instrumentations.split(/\r?\n/).some(line => line === `instrumentation:${component} (target=${test.target})`)) throw new Error(`Missing expected ${test.name} instrumentation target`);
      let result;
      const log = path.join(verification, test.name + "-instrumentation.txt");
      try {
        result = run(["shell", "am", "instrument", "-w", "-r", "-e", "class", test.className, component], { timeout: 300000 });
      } catch (error) {
        // Keep partial test output even when adb times out; never turn a timeout into a pass.
        await fs.writeFile(log, [error.stdout, error.stderr, `Runner error: ${error.message}`].filter(Boolean).join("\n") + "\n");
        throw error;
      }
      await fs.writeFile(log, result + "\n");
      tests[test.name] = parseInstrumentation(result, test.count);
    }
    run(["shell", "am", "force-stop", identifier]);
    const launch = run(["shell", "am", "start", "-W", "-n", `${identifier}/.MainActivity`]);
    await fs.writeFile(path.join(verification, "launch.txt"), launch + "\n");
    if (!/Status: ok/.test(launch)) throw new Error("Final Android APK did not launch");
    let screenshot, pid, rendered = false;
    for (let attempt = 0; attempt < 30; attempt++) {
      await delay(2000);
      pid = run(["shell", "pidof", identifier], { allowFailure: true });
      if (!/^\d+$/.test(pid)) throw new Error("Android app exited before rendering");
      const windows = run(["shell", "dumpsys", "window", "windows"]);
      if (!windows.split(/\r?\n/).some(line => /mCurrentFocus/.test(line) && line.includes(`${identifier}/`))) continue;
      const image = run(["exec-out", "screencap", "-p"], { bytes: true });
      await fs.writeFile(path.join(verification, "native-launch.png"), image);
      screenshot = inspectNativeScreenshot(image);
      if (screenshot.passed) { rendered = true; break; }
    }
    if (!rendered) throw new Error("No nonblank focused Android application window was verified");
    const report = { passed: true, serial: options.serial, disposableAvd: avd, apkSha256: entry.sha256,
      sdk: +run(["shell", "getprop", "ro.build.version.sdk"]), abi: run(["shell", "getprop", "ro.product.cpu.abi"]),
      pageSize: +run(["shell", "getconf", "PAGE_SIZE"]), pid: +pid, tests, screenshot,
      scope: "Final release APK on a disposable emulator, real Rust IPC/Keystore and loopback-only HTTP; not physical hardware or store certification" };
    await fs.writeFile(path.join(verification, "native-verification.json"), JSON.stringify(report, null, 2) + "\n");
    console.log(JSON.stringify(report, null, 2));
  } finally {
    await captureEmulatorDiagnostics(run, verification);
    for (const id of installed.reverse()) run(["uninstall", id], { allowFailure: true });
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
