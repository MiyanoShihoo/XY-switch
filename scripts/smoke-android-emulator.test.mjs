import test from "node:test";
import assert from "node:assert/strict";
import { parseArgs, assertEmulatorIdentity, parseInstrumentation, execute } from "./smoke-android-emulator.mjs";
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
