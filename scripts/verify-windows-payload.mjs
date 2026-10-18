#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const marker = Buffer.from("__TAURI_BUNDLE_TYPE_VAR_");
const installedMarkers = { nsis: "NSS", msi: "MSI" };
const knownTypes = new Set(["UNK", "___", "NSS", "MSI"]);
function inspect(binary) {
  if (!Buffer.isBuffer(binary) || binary.length < 256 || binary.readUInt16LE(0) !== 0x5a4d) throw new Error("Expected a PE executable");
  const pe = binary.readUInt32LE(0x3c);
  if (pe < 64 || pe + 26 > binary.length || binary.toString("hex", pe, pe + 4) !== "50450000" || binary.readUInt16LE(pe + 4) !== 0x8664 || binary.readUInt16LE(pe + 24) !== 0x20b) throw new Error("Expected a Windows x64 PE32+ executable");
  const candidates = [];
  for (let position = binary.indexOf(marker); position !== -1; position = binary.indexOf(marker, position + 1)) {
    const offset = position + marker.length;
    if (offset + 3 > binary.length) continue;
    const type = binary.toString("latin1", offset, offset + 3);
    if (knownTypes.has(type)) candidates.push({ offset, type });
  }
  if (!candidates.length) throw new Error("Expected exactly one matching known Tauri bundle-type marker; absent or Unknown marker");
  return candidates;
}

export function compareWindowsPayload(built, installed, format) {
  if (!Object.hasOwn(installedMarkers, format)) throw new Error("Expected nsis or msi package format");
  const sources = inspect(built), payloads = inspect(installed);
  if (built.length !== installed.length) throw new Error("Installed executable differs in length from the packaging input");
  // Rust also embeds comparison constants with this prefix. Select the one mutable
  // value by requiring byte identity everywhere outside its exact three-byte suffix.
  const matches = sources.flatMap(source => payloads.filter(payload =>
    source.offset === payload.offset && payload.type === installedMarkers[format] &&
    built.subarray(0, source.offset).equals(installed.subarray(0, payload.offset)) &&
    built.subarray(source.offset + 3).equals(installed.subarray(payload.offset + 3))
  ).map(payload => ({ source, payload })));
  if (matches.length !== 1) throw new Error("Expected exactly one matching marker: wrong Tauri package type or executable differs beyond the exact three-byte marker");
  const { source, payload } = matches[0];
  return { passed: true, format, architecture: "x86_64", bytes: installed.length,
    builtSha256: createHash("sha256").update(built).digest("hex"),
    installedSha256: createHash("sha256").update(installed).digest("hex"),
    bundleMarkerOffset: payload.offset - marker.length, packagingInputMarker: source.type,
    installedMarker: payload.type, matchesPackagingInputExceptBundleMarker: true };
}

async function main(args) {
  if (args.length !== 3) throw new Error("Usage: verify-windows-payload.mjs <built.exe> <installed.exe> <nsis|msi>");
  const [built, installed] = await Promise.all(args.slice(0, 2).map(file => readFile(file)));
  console.log(JSON.stringify(compareWindowsPayload(built, installed, args[2]), null, 2));
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
