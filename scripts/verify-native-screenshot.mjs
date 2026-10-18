import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { inflateSync } from "node:zlib";

function decodePng(bytes) {
  if (bytes.length > 48 * 1024 * 1024 || !bytes.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) throw new Error("Expected a bounded PNG screenshot");
  let header, ended = false;
  const chunks = [];
  for (let offset = 8; offset < bytes.length;) {
    if (offset + 12 > bytes.length) throw new Error("Truncated PNG chunk");
    const length = bytes.readUInt32BE(offset), type = bytes.toString("ascii", offset + 4, offset + 8);
    if (offset + 12 + length > bytes.length) throw new Error("Truncated PNG payload");
    const payload = bytes.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      if (header || length !== 13) throw new Error("Invalid PNG header");
      header = { width: payload.readUInt32BE(0), height: payload.readUInt32BE(4), depth: payload[8], type: payload[9], compression: payload[10], filter: payload[11], interlace: payload[12] };
    }
    if (type === "IDAT") chunks.push(payload);
    if (type === "IEND") { ended = true; break; }
    offset += length + 12;
  }
  if (!header || !ended || !chunks.length) throw new Error("Incomplete PNG");
  const { width, height, depth, type, compression, filter, interlace } = header;
  if (!width || !height || width > 8192 || height > 8192 || width * height > 32_000_000 || depth !== 8 || ![2, 6].includes(type) || compression || filter || interlace) throw new Error("Unsupported screenshot dimensions or PNG format");
  const channels = type === 6 ? 4 : 3, stride = width * channels;
  const raw = inflateSync(Buffer.concat(chunks), { maxOutputLength: (stride + 1) * height });
  if (raw.length !== (stride + 1) * height) throw new Error("Invalid decoded PNG size");
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const kind = raw[y * (stride + 1)];
    if (kind > 4) throw new Error("Invalid PNG row filter");
    for (let x = 0; x < stride; x++) {
      const left = x >= channels ? pixels[y * stride + x - channels] : 0;
      const up = y ? pixels[(y - 1) * stride + x] : 0;
      const diagonal = y && x >= channels ? pixels[(y - 1) * stride + x - channels] : 0;
      let prediction = 0;
      if (kind === 1) prediction = left;
      if (kind === 2) prediction = up;
      if (kind === 3) prediction = Math.floor((left + up) / 2);
      if (kind === 4) {
        const p = left + up - diagonal, a = Math.abs(p - left), b = Math.abs(p - up), c = Math.abs(p - diagonal);
        prediction = a <= b && a <= c ? left : b <= c ? up : diagonal;
      }
      pixels[y * stride + x] = (raw[y * (stride + 1) + x + 1] + prediction) & 255;
    }
  }
  return { width, height, channels, pixels };
}

export function inspectNativeScreenshot(bytes) {
  const { width, height, channels, pixels } = decodePng(bytes);
  if (width < 100 || height < 200) throw new Error("Screenshot is too small");
  const histogram = new Map(), samples = [];
  // Ignore the status bar, home indicator and edges: a live OS is not a rendered app.
  for (let y = Math.ceil(height * 0.14); y < height * 0.92; y += 3) {
    for (let x = Math.ceil(width * 0.05); x < width * 0.95; x += 3) {
      const i = (y * width + x) * channels, alpha = channels === 4 ? pixels[i + 3] / 255 : 1;
      const rgb = [0, 1, 2].map(c => Math.round(pixels[i + c] * alpha + 255 * (1 - alpha)));
      const key = (rgb[0] >> 4) << 8 | (rgb[1] >> 4) << 4 | rgb[2] >> 4;
      histogram.set(key, (histogram.get(key) || 0) + 1);
      samples.push(rgb);
    }
  }
  const [dominant] = [...histogram].sort((a, b) => b[1] - a[1])[0];
  const background = [dominant >> 8, dominant >> 4 & 15, dominant & 15].map(v => v * 16 + 8);
  const contrasting = samples.filter(rgb => rgb.some((value, c) => Math.abs(value - background[c]) >= 32)).length;
  const contrastRatio = contrasting / samples.length;
  const passed = contrastRatio >= 0.005 && contrasting >= 100 && histogram.size >= 6;
  return { passed, width, height, contentContrastRatio: Number(contrastRatio.toFixed(6)), distinctContentColors: histogram.size, scope: "Nonblank application content outside system chrome; not a full interaction test", reason: passed ? "Visible application content detected" : "Application content is blank or insufficiently rendered" };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length !== 3) throw new Error("Usage: node verify-native-screenshot.mjs screenshot.png");
    const result = inspectNativeScreenshot(await readFile(process.argv[2]));
    console.log(JSON.stringify(result, null, 2));
    if (!result.passed) process.exitCode = 1;
  } catch (error) {
    console.log(JSON.stringify({ passed: false, error: error.message }, null, 2));
    process.exitCode = 2;
  }
}
