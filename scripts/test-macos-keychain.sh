#!/usr/bin/env bash
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'This test requires macOS.' >&2; exit 1; }
# Changing the default keychain is only allowed on an explicitly disposable host.
[[ "${GITHUB_ACTIONS:-}" == true || "${XY_SWITCH_ISOLATED_KEYCHAIN_TEST:-}" == 1 ]] || {
  echo 'Run on a disposable macOS account with XY_SWITCH_ISOLATED_KEYCHAIN_TEST=1.' >&2; exit 1;
}
root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_root="${TMPDIR:-/tmp}"; tmp_root="${tmp_root%/}"
temporary="$(mktemp -d "$tmp_root/xy-switch-keychain.XXXXXX")"
keychain="$temporary/fixture.keychain-db"
previous_default="$(security default-keychain -d user | python3 -c 'import shlex,sys; print(shlex.split(sys.stdin.read())[0])')"
previous_search=()
while IFS= read -r entry; do previous_search+=("$entry"); done < <(security list-keychains -d user | python3 -c 'import shlex,sys; print("\n".join(shlex.split(sys.stdin.read())))')
cleanup() {
  security default-keychain -d user -s "$previous_default" >/dev/null 2>&1 || true
  security list-keychains -d user -s "${previous_search[@]}" >/dev/null 2>&1 || true
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  [[ "$temporary" == "$tmp_root"/xy-switch-keychain.* ]] && rm -rf -- "$temporary"
}
trap cleanup EXIT
password="$(openssl rand -hex 24)"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
unset password
security default-keychain -d user -s "$keychain"
security list-keychains -d user -s "$keychain"
cd "$root"
cargo test --manifest-path src-tauri/Cargo.toml --locked native_keychain_round_trip -- --ignored --test-threads=1
