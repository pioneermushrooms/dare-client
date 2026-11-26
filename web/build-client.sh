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
# The sounds, which the game no longer builds in: one file the page fetches after the game
# starts. Each entry is a 1-byte name length, the name ("sound/<file>"), a 4-byte
# little-endian size and the file.
python3 - "assets/classic/sound" "$output/pkg/sounds.pack" <<'PY'
import os, struct, sys
folder, out = sys.argv[1], sys.argv[2]
with open(out, "wb") as pack:
    for name in sorted(os.listdir(folder)):
        path = os.path.join(folder, name)
        if not os.path.isfile(path):
            continue
        key = f"sound/{name.lower()}".encode()
        data = open(path, "rb").read()
        pack.write(bytes([len(key)]) + key + struct.pack("<I", len(data)) + data)
PY
wasm_bytes="$(wc -c < "$output/pkg/oss_client_bg.wasm" | tr -d ' ')"
printf '{"wasmBytes":%s}\n' "$wasm_bytes" > "$output/build.json"
install -m 0644 web/client/index.html web/client/start.mjs web/transport.mjs "$output/"
