#!/bin/bash
# Builds Dare.app: the oss-client release binary, its bundled graphics and sounds,
# and an icon made from the game's Warbird sprite.
#
# usage: mac/make-app.sh [destination folder]   (default: /Applications)
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-/Applications}"
APP="$DEST/Dare.app"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cd "$REPO"
"${CARGO:-$HOME/.cargo/bin/cargo}" build --release -p oss-client

# Icon: 1024px master, then the sizes macOS wants, then .icns.
python3 -I mac/make_icon.py assets/classic/graphics/ships.bm2 "$WORK/icon.png" 0 >/dev/null
ICONSET="$WORK/Dare.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$WORK/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$WORK/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$WORK/Dare.icns"

VERSION="$(grep -m1 '^version' crates/oss-client/Cargo.toml | cut -d'"' -f2 || true)"
VERSION="${VERSION:-0.1.0}"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/oss-client "$APP/Contents/MacOS/oss-client"
# The client looks for assets/classic beside its executable.
cp -R assets "$APP/Contents/MacOS/assets"
cp "$WORK/Dare.icns" "$APP/Contents/Resources/Dare.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Dare</string>
  <key>CFBundleDisplayName</key><string>Dare</string>
  <key>CFBundleIdentifier</key><string>zone.dare.client</string>
  <key>CFBundleExecutable</key><string>oss-client</string>
  <key>CFBundleIconFile</key><string>Dare</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.action-games</string>
</dict>
</plist>
PLIST

# Ad-hoc signature so Apple Silicon runs it; not notarised.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1
echo "Built $APP"
