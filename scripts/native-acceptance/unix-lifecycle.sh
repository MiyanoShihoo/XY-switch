#!/usr/bin/env bash
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]] || {
  echo 'Native lifecycle acceptance requires a disposable GitHub-hosted runner' >&2; exit 1;
}
[[ $# == 4 && -d "${RUNNER_TEMP:-}" ]] || { echo 'Specify platform, current artifacts, previous packages and output' >&2; exit 1; }
platform="$1"; artifacts="$2"; previous="$3"; output="$4"
case "$platform:$(uname -s)" in linux:Linux|macos:Darwin) ;; *) echo 'Native lifecycle platform mismatch' >&2; exit 1;; esac
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
temporary="$(mktemp -d "$RUNNER_TEMP/xy-unix-lifecycle.XXXXXX")"
mkdir -p "$temporary/home" "$temporary/codex" "$temporary/runtime" "$temporary/tmp" "$temporary/data" "$temporary/config" "$temporary/cache" "$temporary/state"
chmod 700 "$temporary" "$temporary/runtime"
version="$(node -p "require('./package.json').version")"
case "$platform" in linux) package_platform=linux-x64;; macos) package_platform=macos-universal;; esac
node scripts/native-acceptance/package-input.mjs "$artifacts" "$package_platform" > "$output/package-input.json"
driver="$root/scripts/native-acceptance/unix-lifecycle.py"
if [[ "$platform" == linux ]]; then
  env HOME="$temporary/home" CODEX_HOME="$temporary/codex" XDG_DATA_HOME="$temporary/data" \
    XDG_CONFIG_HOME="$temporary/config" XDG_CACHE_HOME="$temporary/cache" XDG_STATE_HOME="$temporary/state" \
    XDG_RUNTIME_DIR="$temporary/runtime" TMPDIR="$temporary/tmp" LANG=C.UTF-8 LC_ALL=C.UTF-8 \
    LIBGL_ALWAYS_SOFTWARE=1 APPIMAGE_EXTRACT_AND_RUN=1 \
    xvfb-run --auto-servernum --server-args='-screen 0 1440x1080x24' dbus-run-session -- \
    bash -euo pipefail -c '
      printf "%s\n" "xy-native-lifecycle-throwaway" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
      exec python3 "$@"
    ' -- "$driver" linux "$output/package-input.json" "$previous" "$output" "$temporary"
else
  mkdir -p "$temporary/home/Library/Preferences" "$temporary/home/Library/Keychains"
  swiftc scripts/native-acceptance/macos-gui.swift -o "$temporary/macos-gui"
  env HOME="$temporary/home" CFFIXED_USER_HOME="$temporary/home" CODEX_HOME="$temporary/codex" \
    TMPDIR="$temporary/tmp" python3 "$driver" macos "$output/package-input.json" "$previous" "$output" "$temporary"
fi
