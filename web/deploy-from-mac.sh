#!/usr/bin/env bash
set -euo pipefail
core="${DARE_CORE_CHECKOUT:-$HOME/oss-src/oss-core}"
core_revision=eae56a0016474ef9883eb74d47ba8864f019f5e8
repo=https://github.com/pioneermushrooms/dare-client.git
ref=foundation-browser-parity
host=root@5.161.238.245

scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" true
git clone -q --single-branch --branch "$ref" "$repo" "$scratch/dare-client"
mkdir "$scratch/oss-core"
git -C "$core" archive "$core_revision" | tar -xf - -C "$scratch/oss-core"
rustup target add wasm32-unknown-unknown
if [[ "$(wasm-bindgen --version 2>/dev/null || true)" != 'wasm-bindgen 0.2.130' ]]; then
  cargo install wasm-bindgen-cli --version 0.2.130 --locked --root "$scratch/tools"
  export PATH="$scratch/tools/bin:$PATH"
fi
"$scratch/dare-client/web/build-client.sh" "$scratch/build"
cp "$scratch/dare-client/web/install-foundation.sh" "$scratch/install.sh"
tar -czf "$scratch/upload.tar.gz" -C "$scratch" build install.sh

remote="$(ssh "$host" 'mktemp -d /tmp/foundation-play.XXXXXXXX')"
case "$remote" in
  /tmp/foundation-play.*) ;;
  *) echo "Unexpected staging directory." >&2; exit 1 ;;
esac
scp "$scratch/upload.tar.gz" "$host:$remote/upload.tar.gz"
ssh "$host" "cd '$remote' && tar -xzf upload.tar.gz && bash install.sh build"
ssh "$host" "rm -rf -- '$remote'"
