#!/bin/sh
# Builds the script engine for the browser: core/otw-script-wasm → <out-dir> (wasm-bindgen,
# --target web). The wasm-bindgen CLI must match the crate version pinned in Cargo.lock, so it
# is (re)installed from that version when it differs.
# Usage: build-wasm.sh <core-dir> <out-dir>
set -eu
core=$1
out=$2
version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$core/Cargo.lock")
if [ -z "$version" ]; then
  echo "wasm-bindgen not found in $core/Cargo.lock" >&2
  exit 1
fi
if [ "$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }')" != "$version" ]; then
  cargo install wasm-bindgen-cli --version "$version" --locked
fi
cargo build --manifest-path "$core/Cargo.toml" -p otw-script-wasm --release --target wasm32-unknown-unknown
target=${CARGO_TARGET_DIR:-$core/target}
wasm-bindgen --target web --out-dir "$out" "$target/wasm32-unknown-unknown/release/otw_script_wasm.wasm"
