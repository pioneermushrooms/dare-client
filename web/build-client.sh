#!/usr/bin/env bash
set -euo pipefail
if [[ $# -ne 1 ]]; then
  echo "Usage: $0 OUTPUT_DIRECTORY" >&2
  exit 2
fi
mkdir -p -- "$1"
output="$(cd -- "$1" && pwd)"
cd -- "$(dirname -- "$0")/.."
target="${CARGO_TARGET_DIR:-$PWD/target}"
export CARGO_TARGET_DIR="$target"
cargo build --locked --release --target wasm32-unknown-unknown -p oss-client --lib
wasm-bindgen --target web --out-dir "$output/pkg" \
  "$target/wasm32-unknown-unknown/release/oss_client.wasm"
wasm_bytes="$(wc -c < "$output/pkg/oss_client_bg.wasm" | tr -d ' ')"
printf '{"wasmBytes":%s}\n' "$wasm_bytes" > "$output/build.json"
install -m 0644 web/client/index.html web/client/start.mjs web/transport.mjs "$output/"
