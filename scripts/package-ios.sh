#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
mode="${1:-simulator}"
[[ "$(uname -s)" == Darwin ]] || { echo 'iOS builds require macOS and Xcode; use the GitHub Actions iOS job from Windows/Linux.' >&2; exit 1; }
[[ "$mode" == simulator || "$mode" == device ]] || { echo 'Usage: bash scripts/package-ios.sh simulator|device' >&2; exit 1; }
for command in node pnpm cargo rustup xcodebuild xcrun pod ditto; do
  command -v "$command" >/dev/null || { echo "Missing build prerequisite: $command" >&2; exit 1; }
done
version="$(node -p "require('./package.json').version")"
product="$(node -p "require('./src-tauri/tauri.conf.json').productName")"
if [[ "$mode" == simulator ]]; then
  if [[ "$(uname -m)" == arm64 ]]; then target=aarch64-sim; rust_target=aarch64-apple-ios-sim; arch=arm64
  else target=x86_64; rust_target=x86_64-apple-ios; arch=x64; fi
  args=(--no-sign)
else
  : "${APPLE_DEVELOPMENT_TEAM:?Set APPLE_DEVELOPMENT_TEAM for a signed device build}"
  method="${IOS_EXPORT_METHOD:-app-store-connect}"
  case "$method" in app-store-connect|release-testing|debugging) ;; *) echo 'Invalid IOS_EXPORT_METHOD' >&2; exit 1;; esac
  if [[ -n "${IOS_CERTIFICATE:-}" || -n "${IOS_CERTIFICATE_PASSWORD:-}" || -n "${IOS_MOBILE_PROVISION:-}" ]]; then
    : "${IOS_CERTIFICATE:?Supply the base64 signing certificate}"
    : "${IOS_CERTIFICATE_PASSWORD:?Supply the certificate password}"
    : "${IOS_MOBILE_PROVISION:?Supply the base64 provisioning profile}"
  fi
  target=aarch64; rust_target=aarch64-apple-ios; arch=arm64
  args=(--export-method "$method")
fi
rustup target add "$rust_target"
if [[ ! -d src-tauri/gen/apple ]]; then pnpm exec tauri ios init --ci; fi
pnpm exec tauri ios build --ci --target "$target" "${args[@]}" -- --locked
output="$root/release/$version/ios-$arch-$mode"
mkdir -p "$output"
if [[ "$mode" == simulator ]]; then
  apps=()
  while IFS= read -r -d '' candidate; do apps+=("$candidate"); done < <(
    find "$root/src-tauri/gen/apple" -type d -name "$product.app" -not -path '*.xcarchive/*' -print0 -prune
  )
  [[ "${#apps[@]}" == 1 ]] || { echo "Expected exactly one exported simulator app, found ${#apps[@]}" >&2; exit 1; }
  platform="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleSupportedPlatforms:0' "${apps[0]}/Info.plist")"
  [[ "$platform" == iPhoneSimulator ]] || { echo 'Refusing to label a device app as a simulator build' >&2; exit 1; }
  name="XY-Switch-$version-ios-$arch-simulator.zip"
  ditto -c -k --sequesterRsrc --keepParent "${apps[0]}" "$output/$name"
  printf '%s\n' 'Simulator build only. Not an installable iPhone/iPad IPA.' > "$output/BUILD-TYPE.txt"
else
  ipas=()
  while IFS= read -r -d '' candidate; do ipas+=("$candidate"); done < <(find "$root/src-tauri/gen/apple" -type f -name '*.ipa' -print0)
  [[ "${#ipas[@]}" == 1 ]] || { echo "Expected exactly one exported signed IPA, found ${#ipas[@]}" >&2; exit 1; }
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/xy-switch-ipa.XXXXXX")"
  [[ "$temporary" == "${TMPDIR:-/tmp}"/xy-switch-ipa.* ]] || exit 1
  trap 'rm -rf -- "$temporary"' EXIT
  ditto -x -k "${ipas[0]}" "$temporary"
  app="$temporary/Payload/$product.app"
  [[ -f "$app/embedded.mobileprovision" ]] || { echo 'IPA has no provisioning profile' >&2; exit 1; }
  codesign --verify --deep --strict "$app"
  codesign -dvv "$app" 2>&1 | grep -q '^Authority=' || { echo 'Ad-hoc/unsigned IPA is not an installable release' >&2; exit 1; }
  name="XY-Switch-$version-ios-arm64-$method.ipa"
  cp "${ipas[0]}" "$output/$name"
  printf 'Signed IPA; export method: %s. Installation is subject to the provisioning profile.\n' "$method" > "$output/BUILD-TYPE.txt"
fi
(cd "$output" && shasum -a 256 "$name" > SHA256SUMS.txt)
printf 'iOS %s artifact: %s/%s\n' "$mode" "$output" "$name"
