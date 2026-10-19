import test from "node:test";
import assert from "node:assert/strict";
import { parseArgs, assertEmulatorIdentity, parseInstrumentation, execute, parseEmulatorSnapshot, emulatorHealth, waitForEmulatorReady, captureEmulatorDiagnostics, isAppWindowFocused, waitForRenderedApp } from "./smoke-android-emulator.mjs";
test("Android smoke test always requires an explicit emulator serial", () => {
  assert.deepEqual(parseArgs(["--serial", "emulator-5558"]), { serial: "emulator-5558" });
  for (const args of [[], ["--serial", "real-phone"], ["--serial", "emulator-5558", "--serial", "emulator-5556"], ["--unknown", "value"]]) assert.throws(() => parseArgs(args));
});
test("Android smoke refuses physical devices and non-task AVDs before installing anything", () => {
  assert.doesNotThrow(() => assertEmulatorIdentity("emulator-5558", "1\n", "xy-switch-ci-local-20261018"));
  for (const [serial, qemu, avd] of [["phone", "1", "xy-switch-ci-1"], ["emulator-5558", "0", "xy-switch-ci-1"], ["emulator-5558", "1", "Personal_Pixel"], ["emulator-5558", "1", "xy-switch-ci-"]]) assert.throws(() => assertEmulatorIdentity(serial, qemu, avd));
});
test("Android command failures retain partial instrumentation output without accepting a timeout", () => {
  const timeout = Object.assign(new Error("fixture timeout"), { code: "ETIMEDOUT" });
  assert.throws(() => execute("adb", ["-s", "emulator-5558"], { spawn: (_command, _args, options) => {
    assert.equal(options.shell, false); assert.equal(options.windowsHide, true);
    return { error: timeout, status: null, stdout: "INSTRUMENTATION_STATUS: test=fixture", stderr: "fixture stderr" };
  }}), error => error === timeout && error.stdout.includes("test=fixture") && error.stderr === "fixture stderr");
  assert.throws(() => execute("adb", [], { spawn: () => ({ status: 1, stdout: "partial fixture", stderr: "failed" }) }), error => error.stdout === "partial fixture" && error.stderr === "failed");
});
test("Android instrumentation requires the exact passing test count", () => {
  assert.deepEqual(parseInstrumentation("Time: 0.3\n\nOK (4 tests)\nINSTRUMENTATION_CODE: -1", 4), { passed: true, tests: 4 });
  for (const text of ["OK (0 tests)", "OK (4 tests)\nOK (4 tests)", "OK (3 tests)", "FAILURES!!!\nTests run: 4", "INSTRUMENTATION_FAILED", "OK (4 tests)\nINSTRUMENTATION_RESULT: shortMsg=Process crashed"]) assert.throws(() => parseInstrumentation(text, 4));
});

test("Android instrumentation requires full-line success and a unique successful completion code", () => {
  assert.deepEqual(parseInstrumentation("OK (1 test)\r\nINSTRUMENTATION_CODE: -1\r\n", 1), { passed: true, tests: 1 });
  for (const text of [
    "OK (1 test)", "prefix OK (1 test)\nINSTRUMENTATION_CODE: -1", "OK (1 test) suffix\nINSTRUMENTATION_CODE: -1",
    "OK (1 test)\nINSTRUMENTATION_CODE: 0", "OK (1 test)\nINSTRUMENTATION_CODE: -1\nINSTRUMENTATION_CODE: -1",
    "OK (1 test)\nINSTRUMENTATION_CODE: -1\nRunner error: fixture timeout",
  ]) assert.throws(() => parseInstrumentation(text, 1));
  assert.throws(() => parseInstrumentation("OK (0 tests)\nINSTRUMENTATION_CODE: -1", 0));
});
test("Android readiness parses aggregate counters and does not double-count guest CPU", () => {
  const previous = parseEmulatorSnapshot("cpu 100 0 20 400 10 0 0 0 90 0\ncpu0 1 2", "MemTotal: 4194304 kB\nMemAvailable: 1048576 kB\n");
  const current = parseEmulatorSnapshot("cpu 120 0 30 450 20 0 0 0 110 0\n", "MemAvailable: 524288 kB\n");
  assert.deepEqual(emulatorHealth(previous, current), { busyFraction: 1 / 3, availableMemoryMiB: 512 });
  assert.throws(() => emulatorHealth(current, previous));
  assert.throws(() => emulatorHealth(previous, previous));
  for (const [cpu, memory] of [["cpu0 1 2 3 4 5 6 7 8", "MemAvailable: 10 kB"], ["cpu 1 2", "MemAvailable: 10 kB"], ["cpu 1 2 3 4 5 6 7 8", "MemFree: 10 kB"], ["cpu -1 2 3 4 5 6 7 8", "MemAvailable: 10 kB"]]) assert.throws(() => parseEmulatorSnapshot(cpu, memory));
});
function readinessFixture({ busyTicks = 20, idleTicks = 80, availableKiB = 1048576 } = {}) {
  let time = 0, reads = 0;
  return {
    run(args) {
      if (args[2] === "/proc/meminfo") return `MemAvailable: ${availableKiB} kB\n`;
      assert.equal(args[2], "/proc/stat");
      return `cpu ${++reads * busyTicks} 0 0 ${reads * idleTicks} 0 0 0 0\n`;
    },
    options: { now: () => time, sleep: async ms => { time += ms; }, timeout: 20000, interval: 5000, stableSamples: 3 },
  };
}
test("Android app tests wait for three healthy emulator samples", async () => {
  const fixture = readinessFixture();
  const result = await waitForEmulatorReady(fixture.run, fixture.options);
  assert.equal(result.passed, true);
  assert.equal(result.samples.length, 3);
  assert.equal(result.samples[2].elapsedMs, 15000);
});
test("Android readiness rejects sustained CPU or memory pressure, rather than waiving runtime tests", async () => {
  for (const options of [{ busyTicks: 99, idleTicks: 1 }, { availableKiB: 262144 }]) {
    const fixture = readinessFixture(options);
    await assert.rejects(waitForEmulatorReady(fixture.run, fixture.options), error => error.report.passed === false && error.report.samples.length === 4);
  }
});
test("Android diagnostics retain partial errors and collect ANR traces after a failed logcat", async () => {
  const files = new Map(), calls = [];
  await captureEmulatorDiagnostics(args => {
    calls.push(args);
    if (args[0] === "logcat") throw Object.assign(new Error("fixture timeout"), { stdout: "partial log", stderr: "fixture stderr" });
    return "fixture diagnostic";
  }, "fixture", async (name, content) => { files.set(name.replaceAll("\\", "/"), content); });
  assert.equal(files.size, 6);
  assert.match(files.get("fixture/emulator-logcat.txt"), /partial log[\s\S]*fixture timeout/);
  assert.equal(files.get("fixture/emulator-anr-traces.txt"), "fixture diagnostic\n");
  assert.deepEqual(calls[0], ["logcat", "-d", "-b", "all", "-t", "20000"]);
});

const appId = "com.xyswitch.desktop";
const appFocus = "  mCurrentFocus=Window{a15bc u0 com.xyswitch.desktop/com.xyswitch.desktop.MainActivity}";
test("Android focus requires the exact currently focused application package", () => {
  assert.equal(isAppWindowFocused("WINDOW MANAGER DISPLAY CONTENTS\n" + appFocus, appId), true);
  assert.equal(isAppWindowFocused("  mCurrentFocus=Window{abc u10 com.xyswitch.desktop/.MainActivity}\r\n", appId), true);
  for (const dump of [
    "", "  mCurrentFocus=null", appFocus.replace("mCurrentFocus", "mLastFocus"),
    appFocus.replace("mCurrentFocus", "mFocusedApp"), "note: " + appFocus,
    appFocus.replace("com.xyswitch.desktop/", "com.xyswitch.desktop.test/"),
    appFocus.replace("com.xyswitch.desktop/", "other.com.xyswitch.desktop/"),
    "  mCurrentFocus=Window{abc u0 com.android.launcher/.Launcher}\nWindow{def u0 com.xyswitch.desktop/.MainActivity}",
  ]) assert.equal(isAppWindowFocused(dump, appId), false, dump);
  assert.throws(() => isAppWindowFocused(appFocus, ""));
});
function renderingFixture({ focus = [appFocus], inspections = [true], pids = ["4080"] } = {}) {
  const files = new Map(), calls = [];
  let dumpCount = 0, screenshotCount = 0, pidCount = 0;
  const result = (items, index) => items[Math.min(index, items.length - 1)];
  return {
    files, calls,
    run(args, options) {
      calls.push(args);
      if (args[1] === "pidof") return result(pids, pidCount++);
      if (args[1] === "dumpsys") {
        assert.deepEqual(args, ["shell", "dumpsys", "window"]);
        return result(focus, dumpCount++);
      }
      assert.deepEqual(args, ["exec-out", "screencap", "-p"]);
      assert.equal(options.bytes, true);
      return Buffer.from("screenshot-fixture");
    },
    options: {
      sleep: async () => {}, attempts: 3,
      write: async (name, content) => { files.set(name.replaceAll("\\", "/"), content); },
      inspect: image => { assert.equal(image.toString(), "screenshot-fixture"); return { passed: result(inspections, screenshotCount++), width: 1080, height: 2400 }; },
    },
  };
}
test("Android rendering reads the full window dump and waits for real app focus", async () => {
  const fixture = renderingFixture({ focus: ["  mCurrentFocus=null", appFocus] });
  const result = await waitForRenderedApp(fixture.run, appId, "fixture", fixture.options);
  assert.equal(result.pid, 4080);
  assert.equal(result.screenshot.passed, true);
  assert.equal(fixture.calls.filter(args => args[1] === "dumpsys").length, 2);
  assert.equal(fixture.calls.filter(args => args[0] === "exec-out").length, 1);
  assert.equal(fixture.files.get("fixture/window-state.txt"), appFocus + "\n");
  assert.equal(fixture.files.get("fixture/native-launch.png").toString(), "screenshot-fixture");
});
test("Android rendering still requires nonblank content after focus", async () => {
  const fixture = renderingFixture({ inspections: [false, true] });
  const result = await waitForRenderedApp(fixture.run, appId, "fixture", fixture.options);
  assert.equal(result.screenshot.passed, true);
  assert.equal(fixture.calls.filter(args => args[0] === "exec-out").length, 2);
});
test("Android rendering fails closed on an unfocused, blank or terminated app", async () => {
  for (const options of [{ focus: ["  mCurrentFocus=null"] }, { inspections: [false] }]) {
    const fixture = renderingFixture(options);
    await assert.rejects(waitForRenderedApp(fixture.run, appId, "fixture", fixture.options), /No nonblank focused/);
    assert.equal(fixture.calls.filter(args => args[1] === "dumpsys").length, 3);
  }
  const fixture = renderingFixture({ pids: [""] });
  await assert.rejects(waitForRenderedApp(fixture.run, appId, "fixture", fixture.options), /app exited/);
  assert.equal(fixture.calls.length, 1);
});
