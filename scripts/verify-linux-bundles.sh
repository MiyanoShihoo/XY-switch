#!/usr/bin/env bash
set -euo pipefail
[[ "$(uname -s)" == Linux ]] || { echo 'Linux package verification requires Linux' >&2; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(node -p "require('./package.json').version")"
arch="$(node -p process.arch)"
case "$arch" in x64) deb_arch=amd64;; arm64) deb_arch=arm64;; *) echo 'Unsupported architecture' >&2; exit 1;; esac
output="$root/release/$version/linux-$arch"
deb="$output/XY-Switch-$version-linux-$arch.deb"
appimage="$output/XY-Switch-$version-linux-$arch.AppImage"
[[ -f "$deb" && -f "$appimage" ]] || { echo 'Build both Linux packages first' >&2; exit 1; }
(cd "$output" && sha256sum -c SHA256SUMS.txt)
[[ "$(dpkg-deb --field "$deb" Version)" == "$version" ]] || exit 1
[[ "$(dpkg-deb --field "$deb" Architecture)" == "$deb_arch" ]] || exit 1
dpkg-deb --field "$deb" Depends | grep -Eq '(^|, )[[:space:]]*(libayatana-appindicator3-1|libappindicator3-1)([ ,(]|$)'
parent="$(realpath "${TMPDIR:-/tmp}")"
temporary="$(mktemp -d "$parent/xy-switch-bundles.XXXXXX")"
[[ "$(realpath "$temporary")" == "$parent"/xy-switch-bundles.* ]] || exit 1
cleanup() { [[ "$(realpath "$temporary")" == "$parent"/xy-switch-bundles.* ]] && rm -rf -- "$temporary"; }
trap cleanup EXIT
dpkg-deb --extract "$deb" "$temporary/deb"
chmod u+x "$appimage"
(cd "$temporary" && "$appimage" --appimage-extract > "$output/appimage-extract.txt")
appdir="$temporary/squashfs-root"
# libappindicator is loaded dynamically, so an ELF dependency scan alone misses it.
[[ -f "$appdir/usr/lib/libayatana-appindicator3.so.1" || -f "$appdir/usr/lib/libappindicator3.so.1" ]] || {
  echo 'AppImage omitted the dynamically loaded tray library; check the Linux tauri/tray-icon build feature' >&2; exit 1;
}
for format in deb appimage; do
  isolated="$temporary/$format-home"
  mkdir -p "$isolated/home/.codex" "$isolated/data" "$isolated/config" "$isolated/cache" "$isolated/state" "$isolated/runtime" "$isolated/tmp"
  chmod 700 "$isolated/runtime"
  if [[ "$format" == deb ]]; then executable="$temporary/deb/usr/bin/xy-switch"; else executable="$appdir/AppRun"; fi
  env -i PATH=/usr/bin:/bin HOME="$isolated/home" CODEX_HOME="$isolated/home/.codex" XDG_DATA_HOME="$isolated/data" XDG_CONFIG_HOME="$isolated/config" XDG_CACHE_HOME="$isolated/cache" XDG_STATE_HOME="$isolated/state" XDG_RUNTIME_DIR="$isolated/runtime" TMPDIR="$isolated/tmp" LANG=C.UTF-8 LC_ALL=C.UTF-8 LIBGL_ALWAYS_SOFTWARE=1 xvfb-run --auto-servernum --server-args='-screen 0 1280x800x24' dbus-run-session -- python3 - "$executable" "$output" "$format" <<'PY'
from pathlib import Path
import json, os, subprocess, sys, time
executable, directory, kind = sys.argv[1:]
output = Path(directory)
result = {"format": kind, "passed": False, "scope": "Unpacked package startup in isolated Xvfb/DBus/home; not an interactive install test"}
process = None
try:
    with (output / f"native-{kind}-launch.log").open("wb") as log:
        process = subprocess.Popen([executable], stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        started = time.monotonic()
        while time.monotonic() - started < 8:
            if process.poll() is not None:
                raise RuntimeError(f"Application exited early with status {process.returncode}")
            time.sleep(0.5)
        database = Path(os.environ["XDG_DATA_HOME"]) / "com.xyswitch.desktop/xy-switch.db"
        if not database.is_file():
            raise RuntimeError("No database was created in the isolated data directory")
        result.update(passed=True, processAliveSeconds=round(time.monotonic() - started, 2), isolatedDatabaseCreated=True)
except Exception as error:
    result["error"] = str(error)
finally:
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    (output / f"native-{kind}-verification.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2))
if not result["passed"]:
    raise SystemExit(1)
PY
done
printf 'Debian metadata, AppImage tray dependency, and both isolated native launches passed.\n'
