import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { selectSimulator } from "./select-ios-simulator.mjs";

const runtime = (version, available = true) => ({ identifier: "com.apple.CoreSimulator.SimRuntime.iOS-" + version.replaceAll(".", "-"), version, isAvailable: available });
const phone = { identifier: "com.apple.CoreSimulator.SimDeviceType.iPhone-17", name: "iPhone 17" };
const newerPhone = { identifier: "com.apple.CoreSimulator.SimDeviceType.iPhone-17e", name: "iPhone 17e" };
const device = (type = phone, available = true) => ({ name: type.name, deviceTypeIdentifier: type.identifier, isAvailable: available });
const inventory = (runtimes, pairs, devicetypes = [phone, newerPhone]) => ({ runtimes, devices: pairs, devicetypes });

test("uses a known compatible pair rather than the independently last runtime/type", () => {
  const old = runtime("26.2");
  const current = runtime("26.5");
  const selected = selectSimulator(inventory([current, old], { [current.identifier]: [device(phone)], [old.identifier]: [] }));
  assert.equal(selected.runtimeIdentifier, current.identifier);
  assert.equal(selected.deviceTypeIdentifier, phone.identifier);
});

test("orders runtime versions numerically", () => {
  const a = runtime("26.9"); const b = runtime("26.10.1");
  const selected = selectSimulator(inventory([b, a], { [a.identifier]: [device()], [b.identifier]: [device(newerPhone)] }));
  assert.equal(selected.runtimeVersion, "26.10.1");
});

test("skips unavailable runtimes and unavailable devices", () => {
  const a = runtime("26.2"); const b = runtime("26.4"); const c = runtime("26.5", false);
  assert.equal(selectSimulator(inventory([a, b, c], { [a.identifier]: [device()], [b.identifier]: [device(phone, false)], [c.identifier]: [device()] })).runtimeIdentifier, a.identifier);
});

test("skips non-iOS runtimes and iPad device types", () => {
  const a = runtime("26.5"); const tv = { ...a, identifier: "com.apple.CoreSimulator.SimRuntime.tvOS-26-5" };
  const ipad = { identifier: "com.apple.CoreSimulator.SimDeviceType.iPad", name: "iPad Pro" };
  assert.equal(selectSimulator(inventory([tv, a], { [tv.identifier]: [device()], [a.identifier]: [device(ipad), device()] }, [ipad, phone])).deviceName, "iPhone 17");
});

test("supports older simctl inventories without deviceTypeIdentifier", () => {
  const a = runtime("26.5");
  assert.equal(selectSimulator(inventory([a], { [a.identifier]: [{ name: phone.name, isAvailable: true }] })).deviceTypeIdentifier, phone.identifier);
});

test("does not trust an unknown device type even when its name matches", () => {
  const a = runtime("26.5");
  assert.throws(() => selectSimulator(inventory([a], { [a.identifier]: [{ ...device(), deviceTypeIdentifier: "unknown" }] })), /No available compatible/);
});

test("uses type metadata for a renamed available iPhone", () => {
  const a = runtime("26.5");
  assert.equal(selectSimulator(inventory([a], { [a.identifier]: [{ ...device(), name: "My QA Phone" }] })).deviceName, phone.name);
});

test("fails clearly when no compatible installed pair exists", () => {
  assert.throws(() => selectSimulator(inventory([runtime("26.5")], {})), /Install an iOS simulator runtime/);
  assert.throws(() => selectSimulator({}), /No available compatible/);
});

const cli = fileURLToPath(new URL("./select-ios-simulator.mjs", import.meta.url));
const runCli = input => spawnSync(process.execPath, [cli], { input, encoding: "utf8" });

test("CLI reads a real JSON inventory from stdin and prints the selection", () => {
  const current = runtime("26.5");
  const data = inventory([current], { [current.identifier]: [device()] });
  const result = runCli(JSON.stringify(data));
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.deepEqual(JSON.parse(result.stdout), selectSimulator(data));
});

test("CLI reports invalid JSON on stderr with a failing exit status", () => {
  const result = runCli("not JSON");
  assert.ifError(result.error);
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /JSON/);
});

test("CLI fails without emitting a selection when there is no compatible pair", () => {
  const result = runCli("{}");
  assert.ifError(result.error);
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /No available compatible iPhone\/runtime pair/);
});
