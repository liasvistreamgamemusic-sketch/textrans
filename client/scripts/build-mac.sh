#!/usr/bin/env bash
# macOS の .app / .dmg を手元でビルドし、~/Applications に置く (固定 ID があればそれで署名、無ければ ad-hoc)。
# 使い方: client/scripts/build-mac.sh
set -euo pipefail

CLIENT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="${VOICE_CLIENT_INSTALL_DIR:-$HOME/Applications}"
BUNDLE_DIR="$CLIENT_DIR/src-tauri/target/release/bundle"

# 固定の署名 ID があればそれで署名する (無ければ ad-hoc)。ad-hoc はビルドごとに
# 別アプリ扱いになりアクセシビリティ権限が外れるため、scripts/make-signing-identity.sh で作る。
SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-voice-client Dev}"
if security find-identity -v -p codesigning | grep -q "\"$SIGNING_IDENTITY\""; then
    export APPLE_SIGNING_IDENTITY="$SIGNING_IDENTITY"
    echo "signing with: $SIGNING_IDENTITY"
else
    unset APPLE_SIGNING_IDENTITY
    echo "WARN: 署名 ID '$SIGNING_IDENTITY' が無いので ad-hoc 署名 (権限がビルドごとに外れる)。scripts/make-signing-identity.sh を実行してください" >&2
fi

cd "$CLIENT_DIR"
pnpm install --frozen-lockfile
pnpm tauri build --bundles app,dmg

APP="$BUNDLE_DIR/macos/voice-client.app"
mkdir -p "$INSTALL_DIR"
rm -rf "$INSTALL_DIR/voice-client.app"
cp -R "$APP" "$INSTALL_DIR/"
echo "installed: $INSTALL_DIR/voice-client.app"
ls "$BUNDLE_DIR/dmg/"*.dmg 2>/dev/null || true
