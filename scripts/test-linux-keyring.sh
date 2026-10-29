#!/usr/bin/env bash
set -euo pipefail
[[ "$(uname -s)" == Linux ]] || { echo 'Requires Linux with Secret Service' >&2; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
# Preserve the installed Rust toolchain while isolating all credential data.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
base="${TMPDIR:-/tmp}"
sandbox="$(mktemp -d "$base/xy-switch-keyring.XXXXXX")"
[[ "$sandbox" == "$base"/xy-switch-keyring.* ]] || exit 1
trap 'rm -rf -- "$sandbox"' EXIT
export HOME="$sandbox/home"
export XDG_DATA_HOME="$sandbox/data"
export XDG_CONFIG_HOME="$sandbox/config"
export XDG_RUNTIME_DIR="$sandbox/runtime"
mkdir -p "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
unset DBUS_SESSION_BUS_ADDRESS GNOME_KEYRING_CONTROL
# The throwaway password protects an empty, temporary keyring, never the user's keyring.
dbus-run-session -- bash -euo pipefail -c '
  printf "%s\n" "xy-switch-ephemeral-test-keyring" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
  cargo test --manifest-path src-tauri/Cargo.toml --locked native_keychain_round_trip -- --ignored --test-threads=1
'
