#!/bin/sh
set -eu
verify_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export CARGO_TARGET_DIR="$verify_directory/target/linux"
evidence_directory="$verify_directory/target/evidence"
mkdir -p "$evidence_directory"
evidence_log="$evidence_directory/rust-tests.log"
{
    rustc --version
    cargo --version
} > "$evidence_log"
if cargo test --locked --offline --manifest-path "$verify_directory/Cargo.toml" -- --test-threads=2 >> "$evidence_log" 2>&1; then
    cat "$evidence_log"
else
    verification_exit=$?
    cat "$evidence_log"
    exit "$verification_exit"
fi
