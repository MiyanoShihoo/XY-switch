#!/usr/bin/env bash
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'macOS verification requires macOS.' >&2; exit 1; }
[[ "${GITHUB_ACTIONS:-}" == true || "${XY_SWITCH_ISOLATED_MACOS_TEST:-}" == 1 ]] || {
  echo 'Native launch requires a disposable account (XY_SWITCH_ISOLATED_MACOS_TEST=1).' >&2; exit 1;
}
root="$(cd "$(dirname "$0")/.." && pwd)"; cd "$root"
version="$(node -p "require('./package.json').version")"
identifier="$(node -p "require('./src-tauri/tauri.conf.json').identifier")"
product="$(node -p "require('./src-tauri/tauri.conf.json').productName")"
arch="${1:-universal}"
case "$arch" in arm64|x64|universal) ;; *) echo 'Unsupported macOS architecture' >&2; exit 1;; esac
output="$root/release/$version/macos-$arch"
archive="$output/XY-Switch-$version-macos-$arch.app.zip"
dmg="$output/XY-Switch-$version-macos-$arch.dmg"
(cd "$output" && shasum -a 256 -c SHA256SUMS.txt)
tmp_root="${TMPDIR:-/tmp}"; tmp_root="${tmp_root%/}"
temporary="$(mktemp -d "$tmp_root/xy-switch-macos.XXXXXX")"
mounted=0; app_pid=''
cleanup() {
  if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi
  if [[ "$mounted" == 1 ]]; then hdiutil detach "$temporary/mount" -quiet || true; fi
  [[ "$temporary" == "$tmp_root"/xy-switch-macos.* ]] && rm -rf -- "$temporary"
}
trap cleanup EXIT
mkdir -p "$temporary/unpacked" "$temporary/mount" "$temporary/home" "$temporary/home/.codex"
ditto -x -k "$archive" "$temporary/unpacked"
app="$temporary/unpacked/$product.app"
plist="$app/Contents/Info.plist"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")" == "$identifier" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")" == "$version" ]]
executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist")"
binary="$app/Contents/MacOS/$executable"
file "$binary" | tee "$output/macho-type.txt"
lipo -archs "$binary" > "$output/macho-architectures.txt"
ARCH="$arch" python3 - "$output/macho-architectures.txt" <<'PY'
import os, pathlib, sys
actual=set(pathlib.Path(sys.argv[1]).read_text().split())
expected={'universal':{'x86_64','arm64'},'x64':{'x86_64'},'arm64':{'arm64'}}[os.environ['ARCH']]
assert actual==expected,(actual,expected)
PY
codesign --verify --deep --strict "$app" 2> "$output/codesign-verify.txt"
codesign --display --verbose=4 "$app" 2> "$output/codesign-details.txt"
# Ad-hoc signing validates package integrity, not developer identity or notarization.
if spctl --assess --type execute --verbose=4 "$app" > "$output/gatekeeper.txt" 2>&1; then gatekeeper=accepted; else gatekeeper=not-accepted; fi
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$temporary/mount" > "$output/dmg-mount.txt"
mounted=1
mounted_app="$temporary/mount/$product.app"
codesign --verify --deep --strict "$mounted_app" 2> "$output/dmg-codesign-verify.txt"
cmp "$binary" "$mounted_app/Contents/MacOS/$executable"
cmp "$plist" "$mounted_app/Contents/Info.plist"
hdiutil detach "$temporary/mount" -quiet; mounted=0
swiftc "$root/scripts/macos-window.swift" -o "$temporary/window-id"
started="$(date +%s)"
HOME="$temporary/home" CFFIXED_USER_HOME="$temporary/home" CODEX_HOME="$temporary/home/.codex" \
  "$binary" > "$output/native-launch.log" 2>&1 &
app_pid=$!
rendered=0
for attempt in $(seq 1 20); do
  sleep 3
  kill -0 "$app_pid" 2>/dev/null || { echo 'macOS app exited during startup.' >&2; exit 1; }
  window_id="$("$temporary/window-id" "$app_pid" || true)"
  [[ -n "$window_id" ]] || continue
  if screencapture -x -o -l "$window_id" "$output/native-launch.png" 2>> "$output/screenshot-errors.txt" && \
     node scripts/verify-native-screenshot.mjs "$output/native-launch.png" > "$output/screenshot-check.json"; then
    rendered=1; break
  fi
done
[[ "$rendered" == 1 ]] || { echo 'No rendered macOS app window was verified.' >&2; exit 1; }
lsof -nP -a -p "$app_pid" -iTCP -sTCP:LISTEN > "$output/startup-listeners.txt" || true
[[ ! -s "$output/startup-listeners.txt" ]] || { echo 'Unexpected TCP listener on a clean startup.' >&2; exit 1; }
HOME_DIR="$temporary/home" IDENTIFIER="$identifier" OUTPUT="$output" PID="$app_pid" STARTED="$started" GATEKEEPER="$gatekeeper" ARCH="$arch" python3 - <<'PY'
import json, os, pathlib, sqlite3, time
out=pathlib.Path(os.environ['OUTPUT'])
home=pathlib.Path(os.environ['HOME_DIR'])
db=home/'Library/Application Support'/os.environ['IDENTIFIER']/'xy-switch.db'
assert db.is_file(), 'Database was not created inside the isolated HOME'
connection=sqlite3.connect(f'file:{db.as_posix()}?mode=ro',uri=True)
assert connection.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
connection.close()
signature=(out/'codesign-details.txt').read_text()
report={'passed':True,'architecture':os.environ['ARCH'],'pid':int(os.environ['PID']),
        'aliveAtScreenshotSeconds':int(time.time())-int(os.environ['STARTED']),
        'databaseIntegrity':'ok','isolatedHome':True,'appAndDmgMatch':True,'tcpListeners':0,
        'codesignIntegrity':'verified','signing':'ad-hoc' if 'Signature=adhoc' in signature else 'certificate',
        'gatekeeper':os.environ['GATEKEEPER'],
        'screenshot':json.loads((out/'screenshot-check.json').read_text()),
        'scope':'Native app startup and rendered window on the CI host; not a full interactive or all-hardware test'}
(out/'native-verification.json').write_text(json.dumps(report,indent=2)+'\n')
PY
