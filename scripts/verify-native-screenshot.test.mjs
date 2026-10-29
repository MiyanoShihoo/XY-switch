import assert from "node:assert/strict";
import test from "node:test";
import { deflateSync } from "node:zlib";
import { inspectNativeScreenshot } from "./verify-native-screenshot.mjs";

function chunk(type, data) {
  const bytes = Buffer.concat([Buffer.from(type), data]);
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  const length = Buffer.alloc(4), checksum = Buffer.alloc(4);
  length.writeUInt32BE(data.length); checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([length, bytes, checksum]);
}
function screenshot(content, channels = 3, filter = 0) {
  const width = 180, height = 360, stride = width * channels;
  const pixels = Buffer.alloc(stride * height), raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    let color = [244, 244, 244];
    if (y < 30 || y > 340) color = [0, 0, 0];
    if (content && y > 70 && y < 300 && x > 25 && x < 155) color = [35 + x % 90, 55 + y % 90, 85 + x % 60];
    const i = (y * width + x) * channels;
    color.forEach((c, offset) => { pixels[i + offset] = c; });
    if (channels === 4) pixels[i + 3] = 255;
  }
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = filter;
    for (let x = 0; x < stride; x++) {
      const left = x >= channels ? pixels[y * stride + x - channels] : 0, up = y ? pixels[(y - 1) * stride + x] : 0, diagonal = y && x >= channels ? pixels[(y - 1) * stride + x - channels] : 0;
      let prediction = filter === 1 ? left : filter === 2 ? up : filter === 3 ? Math.floor((left + up) / 2) : 0;
      if (filter === 4) {
        const p = left + up - diagonal, a = Math.abs(p - left), b = Math.abs(p - up), c = Math.abs(p - diagonal);
        prediction = a <= b && a <= c ? left : b <= c ? up : diagonal;
      }
      raw[y * (stride + 1) + x + 1] = (pixels[y * stride + x] - prediction) & 255;
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = channels === 4 ? 6 : 2;
  return Buffer.concat([Buffer.from("89504e470d0a1a0a", "hex"), chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

test("a live status bar and home indicator cannot pass application rendering", () => {
  const result = inspectNativeScreenshot(screenshot(false));
  assert.equal(result.passed, false);
  assert.equal(result.contentContrastRatio, 0);
});
for (const channels of [3, 4]) for (const filter of [0, 1, 2, 3, 4]) {
  test(`detects app content in ${channels}-channel PNG with filter ${filter}`, () => {
    assert.equal(inspectNativeScreenshot(screenshot(true, channels, filter)).passed, true);
  });
}
test("rejects malformed or truncated images", () => {
  assert.throws(() => inspectNativeScreenshot(Buffer.from("not png")), /PNG/);
  assert.throws(() => inspectNativeScreenshot(screenshot(true).subarray(0, 80)), /Truncated|Incomplete/);
});
