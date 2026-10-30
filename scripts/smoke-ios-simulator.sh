#!/usr/bin/env bash
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'Simulator acceptance requires macOS' >&2; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(node -p "require('./package.json').version")"
product="$(node -p "require('./src-tauri/tauri.conf.json').productName")"
identifier="$(node -p "require('./src-tauri/tauri.conf.json').identifier")"
[[ "$(uname -m)" == arm64 ]] && arch=arm64 || arch=x64
package_root="${XY_IOS_PACKAGE_ROOT:-$root/release/$version/ios-$arch-simulator}"
output="${XY_IOS_QA_OUTPUT:-$root/release/$version/ios-$arch-simulator}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
node scripts/native-acceptance/package-input.mjs "$package_root" "ios-$arch-simulator" > "$output/native-package-input.json"
archive="$(node -e 'console.log(require(process.argv[1]).archive)' "$output/native-package-input.json")"
xcrun simctl list --json | node scripts/select-ios-simulator.mjs > "$output/simulator-selection.json"
cat "$output/simulator-selection.json"
runtime="$(node -e 'console.log(require(process.argv[1]).runtimeIdentifier)' "$output/simulator-selection.json")"
type="$(node -e 'console.log(require(process.argv[1]).deviceTypeIdentifier)' "$output/simulator-selection.json")"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/xy-switch-simulator.XXXXXX")"
[[ "$temporary" == "${TMPDIR:-/tmp}"/xy-switch-simulator.* ]] || exit 1
device=''
cleanup() {
  if [[ -n "$device" ]]; then
    xcrun simctl shutdown "$device" >/dev/null 2>&1 || true
    xcrun simctl delete "$device" >/dev/null 2>&1 || true
  fi
  rm -rf -- "$temporary"
}
trap cleanup EXIT
device="$(xcrun simctl create 'XY Switch isolated native acceptance' "$type" "$runtime")"
xcrun simctl boot "$device"
xcrun simctl bootstatus "$device" -b
mkdir -p "$temporary/application"
ditto -x -k "$archive" "$temporary/application"
app="$temporary/application/$product.app"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")" == "$identifier" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleSupportedPlatforms:0' "$app/Info.plist")" == iPhoneSimulator ]]
xcrun simctl install "$device" "$app"
SIMCTL_CHILD_RUST_BACKTRACE=1 xcrun simctl launch --stdout="$output/simulator-stdout.log" --stderr="$output/simulator-stderr.log" "$device" "$identifier" | tee "$output/simulator-launch.txt"
# Nonblank pixels are diagnostic only. XCTest must observe ready UI and persistent native writes.
node scripts/native-acceptance/ios-project.mjs "$temporary/ui-tests"
xcodebuild -version > "$output/xcode-version.txt"
set +e
xcodebuild test -project "$temporary/ui-tests/NativeAcceptance.xcodeproj" -scheme NativeAcceptance \
  -sdk iphonesimulator -destination "id=$device" -destination-timeout 120 \
  -parallel-testing-enabled NO -maximum-concurrent-test-simulator-destinations 1 \
  -derivedDataPath "$temporary/derived" -resultBundlePath "$output/ios-ui.xcresult" \
  CODE_SIGNING_ALLOWED=NO 2>&1 | tee "$output/ios-ui-test.log"
test_status=${PIPESTATUS[0]}
set -e
xcrun simctl io "$device" screenshot "$output/simulator-final.png" || true
if [[ -d "$output/ios-ui.xcresult" ]]; then
  xcrun xcresulttool export attachments --path "$output/ios-ui.xcresult" --output-path "$output/ios-ui-attachments" || true
  xcrun xcresulttool get test-results summary --path "$output/ios-ui.xcresult" > "$output/ios-ui-summary.json" || true
fi
[[ "$test_status" == 0 ]] || { echo 'iOS native readiness/interaction acceptance failed; retain XCTest diagnostics' >&2; exit "$test_status"; }
node -e 'const s=require(process.argv[1]);if(!(s.passedTests>=1 && s.failedTests===0 && s.skippedTests===0))throw Error("XCTest did not execute a passing, unskipped native acceptance test")' "$output/ios-ui-summary.json"
container="$(xcrun simctl get_app_container "$device" "$identifier" data)"
python3 - "$container" "$output/native-database-verification.json" <<'PY'
import json, pathlib, sqlite3, sys
root = pathlib.Path(sys.argv[1]).resolve()
paths = list(root.rglob('xy-switch.db'))
assert len(paths) == 1 and paths[0].resolve().is_relative_to(root), 'Expected one application-owned database'
c = sqlite3.connect(paths[0].resolve().as_uri() + '?mode=ro', uri=True)
assert c.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
assert not c.execute('PRAGMA foreign_key_check').fetchall()
preferences = json.loads(c.execute('SELECT value FROM app_settings WHERE key=?', ('app_preferences',)).fetchone()[0])
assert preferences['requestTimeoutSecs'] == 175, 'Real UI did not persist the native preference'
providers = c.execute('SELECT COUNT(*) FROM providers').fetchone()[0]
assert providers >= 4
c.close()
pathlib.Path(sys.argv[2]).write_text(json.dumps({'integrity': 'ok', 'foreignKeyViolations': 0, 'requestTimeoutSecs': 175, 'providerCount': providers, 'verifiedFromApplicationContainer': True}, indent=2) + '\n')
PY
node scripts/verify-native-screenshot.mjs "$output/simulator-final.png" > "$output/simulator-content-verification.json"
node -e 'const fs=require("fs");const r={passed:true,sourceCommit:process.env.NATIVE_SOURCE_COMMIT||process.env.GITHUB_SHA,harnessCommit:process.env.GITHUB_SHA,sourceRun:process.env.NATIVE_SOURCE_RUN,scope:"Unmodified simulator package; seeded provider readiness, actual UI preference write, process restart, SQLite readback and mobile encrypted-sync read-only UI",physicalDeviceVerified:false};fs.writeFileSync(process.argv[1],JSON.stringify(r,null,2)+"\n")' "$output/native-acceptance.json"
echo 'iOS simulator native UI readiness, preference persistence/relaunch and mobile sync read-only acceptance passed. This is not physical-device validation.'
