#!/usr/bin/env bash
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'Simulator smoke test requires macOS' >&2; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(node -p "require('./package.json').version")"
product="$(node -p "require('./src-tauri/tauri.conf.json').productName")"
identifier="$(node -p "require('./src-tauri/tauri.conf.json').identifier")"
[[ "$(uname -m)" == arm64 ]] && arch=arm64 || arch=x64
output="$root/release/$version/ios-$arch-simulator"
archive="$output/XY-Switch-$version-ios-$arch-simulator.zip"
[[ -f "$archive" ]] || { echo 'Build the simulator archive first' >&2; exit 1; }
# Select a pair known to be available together, not independent last entries.
xcrun simctl list --json | node scripts/select-ios-simulator.mjs > "$output/simulator-selection.json"
cat "$output/simulator-selection.json"
runtime="$(node -e 'console.log(require(process.argv[1]).runtimeIdentifier)' "$output/simulator-selection.json")"
type="$(node -e 'console.log(require(process.argv[1]).deviceTypeIdentifier)' "$output/simulator-selection.json")"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/xy-switch-simulator.XXXXXX")"
[[ "$temporary" == "${TMPDIR:-/tmp}"/xy-switch-simulator.* ]] || exit 1
device=''
cleanup() {
  if [[ -n "$device" ]]; then xcrun simctl shutdown "$device" >/dev/null 2>&1 || true; xcrun simctl delete "$device" >/dev/null 2>&1 || true; fi
  rm -rf -- "$temporary"
}
trap cleanup EXIT
device="$(xcrun simctl create 'XY Switch isolated smoke test' "$type" "$runtime")"
xcrun simctl boot "$device"
xcrun simctl bootstatus "$device" -b
ditto -x -k "$archive" "$temporary"
xcrun simctl install "$device" "$temporary/$product.app"
xcrun simctl launch "$device" "$identifier" | tee "$output/simulator-launch.txt"
sleep 5
xcrun simctl spawn "$device" launchctl list | awk -v bundle="$identifier" '
  $1 ~ /^[0-9]+$/ && $1 > 0 && index($3, bundle) { print; found = 1 }
  END { exit !found }
' > "$output/simulator-process.txt"
xcrun simctl io "$device" screenshot "$output/simulator-launch.png"
echo 'Simulator installation and process-launch smoke passed; this is not physical-device validation.'
