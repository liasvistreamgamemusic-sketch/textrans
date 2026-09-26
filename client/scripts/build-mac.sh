#!/usr/bin/env bash
# macOS の .app / .dmg を手元でビルドし、~/Applications に置く (CI と同じ ad-hoc 署名)。
# 使い方: client/scripts/build-mac.sh
set -euo pipefail

CLIENT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="${VOICE_CLIENT_INSTALL_DIR:-$HOME/Applications}"
BUNDLE_DIR="$CLIENT_DIR/src-tauri/target/release/bundle"

cd "$CLIENT_DIR"
pnpm install --frozen-lockfile
pnpm tauri build --bundles app,dmg

APP="$BUNDLE_DIR/macos/voice-client.app"
mkdir -p "$INSTALL_DIR"
rm -rf "$INSTALL_DIR/voice-client.app"
cp -R "$APP" "$INSTALL_DIR/"
echo "installed: $INSTALL_DIR/voice-client.app"
ls "$BUNDLE_DIR/dmg/"*.dmg 2>/dev/null || true
