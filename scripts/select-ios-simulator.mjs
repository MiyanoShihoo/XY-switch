#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

function compareVersions(left, right) {
  const a = String(left).split(".").map(Number);
  const b = String(right).split(".").map(Number);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const difference = (a[i] || 0) - (b[i] || 0);
    if (difference) return difference;
  }
  return 0;
}

export function selectSimulator(inventory) {
  const types = new Map((inventory.devicetypes || []).map(type => [type.identifier, type]));
  const runtimes = (inventory.runtimes || [])
    .filter(runtime => runtime.isAvailable === true && runtime.identifier?.includes(".iOS-"))
    .sort((left, right) => compareVersions(right.version, left.version));
  for (const runtime of runtimes) {
    for (const device of inventory.devices?.[runtime.identifier] || []) {
      if (device.isAvailable !== true) continue;
      const type = device.deviceTypeIdentifier
        ? types.get(device.deviceTypeIdentifier)
        : [...types.values()].find(candidate => candidate.name === device.name);
      if (!type?.name?.startsWith("iPhone ") || !type.identifier) continue;
      // An existing available device establishes compatibility with this runtime.
      // Picking the newest runtime and device type independently can fail with SimError 403.
      return {
        runtimeIdentifier: runtime.identifier,
        runtimeVersion: runtime.version,
        deviceTypeIdentifier: type.identifier,
        deviceName: type.name,
      };
    }
  }
  throw new Error("No available compatible iPhone/runtime pair found. Install an iOS simulator runtime and create a matching iPhone simulator in Xcode.");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const inventory = JSON.parse(readFileSync(0, "utf8"));
    console.log(JSON.stringify(selectSimulator(inventory), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
