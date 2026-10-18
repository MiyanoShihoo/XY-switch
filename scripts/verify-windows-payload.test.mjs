import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { compareWindowsPayload } from "./verify-windows-payload.mjs";
function executable(kind = "UNK") {
  const bytes = Buffer.alloc(1024, 0);
  bytes.write("MZ"); bytes.writeUInt32LE(128, 0x3c);
  bytes.write("PE\0\0", 128, "ascii"); bytes.writeUInt16LE(0x8664, 132); bytes.writeUInt16LE(0x20b, 152);
  bytes.write("__TAURI_BUNDLE_TYPE_VAR_" + kind, 300);
  return bytes;
}
test("Windows payload permits only the known NSIS/MSI bundle-type substitution", () => {
  for (const [format, kind] of [["nsis", "NSS"], ["msi", "MSI"]]) {
    const result = compareWindowsPayload(executable(), executable(kind), format);
    assert.equal(result.passed, true); assert.equal(result.matchesPackagingInputExceptBundleMarker, true);
    assert.equal(result.packagingInputMarker, "UNK");
    assert.equal(result.installedMarker, kind); assert.equal(result.bundleMarkerOffset, 300);
    assert.notEqual(result.builtSha256, result.installedSha256);
  }
});
test("Windows payload compares all bytes outside the exact marker", () => {
  for (const offset of [0, 256, 299, 326, 700]) {
    const altered = executable("NSS"); altered[offset] ^= 1;
    assert.throws(() => compareWindowsPayload(executable(), altered, "nsis"));
  }
  assert.throws(() => compareWindowsPayload(executable(), Buffer.concat([executable("NSS"), Buffer.from([0])]), "nsis"), /differs/);
});
test("Windows payload rejects wrong architectures and malformed PE headers", () => {
  const arm = executable("NSS"); arm.writeUInt16LE(0xaa64, 132);
  assert.throws(() => compareWindowsPayload(executable(), arm, "nsis"), /x64/);
  const broken = executable("NSS"); broken.writeUInt32LE(0xffffffff, 0x3c);
  assert.throws(() => compareWindowsPayload(executable(), broken, "nsis"), /PE32/);
});
test("Windows payload rejects absent or newly inserted markers", () => {
  const missing = executable("NSS"); missing.fill(0, 300, 350);
  assert.throws(() => compareWindowsPayload(executable(), missing, "nsis"), /exactly one/);
  const duplicate = executable("NSS"); duplicate.write("__TAURI_BUNDLE_TYPE_VAR_NSS", 600);
  assert.throws(() => compareWindowsPayload(executable(), duplicate, "nsis"), /exactly one/);
});
test("Windows payload verifies the expected installed package type", () => {
  assert.throws(() => compareWindowsPayload(executable(), executable("MSI"), "nsis"), /wrong Tauri package type/);
  assert.throws(() => compareWindowsPayload(executable(), executable("NSS"), "msi"), /wrong Tauri package type/);
  assert.throws(() => compareWindowsPayload(executable("bad"), executable("NSS"), "nsis"), /Unknown/);
  assert.throws(() => compareWindowsPayload(executable(), executable("NSS"), "other"), /format/);
});
test("Windows payload can compare an input already carrying a known package marker", () => {
  const result = compareWindowsPayload(executable("MSI"), executable("NSS"), "nsis");
  assert.equal(result.packagingInputMarker, "MSI"); assert.equal(result.passed, true);
});
test("Windows payload distinguishes the mutable UNK marker from Rust comparison constants", () => {
  const built = executable(), installed = executable("NSS");
  for (const buffer of [built, installed]) buffer.write("__TAURI_BUNDLE_TYPE_VAR_NSS", 600);
  const result = compareWindowsPayload(built, installed, "nsis");
  assert.equal(result.bundleMarkerOffset, 300);
  installed[624] ^= 1;
  assert.throws(() => compareWindowsPayload(built, installed, "nsis"), /differs/);
});
test("Windows payload fails closed for ambiguous identical markers", () => {
  const built = executable("NSS"); built.write("__TAURI_BUNDLE_TYPE_VAR_NSS", 600);
  assert.throws(() => compareWindowsPayload(built, Buffer.from(built), "nsis"), /exactly one/);
});
test("Windows payload does not accept high-bit characters as ASCII marker values", () => {
  const installed = executable("NSS"); installed[322] |= 0x80;
  assert.throws(() => compareWindowsPayload(executable(), installed, "nsis"), /Unknown/);
});
test("Windows verifier invokes the payload checker without escaped control characters", () => {
  const script = readFileSync(new URL("./verify-windows-bundles.ps1", import.meta.url), "utf8");
  assert.doesNotMatch(script, /[\x00-\x08\x0b\x0c\x0e-\x1f]/);
  assert.match(script, /scripts\/verify-windows-payload\.mjs/);
});
