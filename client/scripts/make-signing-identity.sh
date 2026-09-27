#!/usr/bin/env bash
# macOS 用の固定コード署名 ID (自己署名) をログインキーチェーンに作る。1 台につき 1 回。
#
# なぜ必要か: ad-hoc 署名 (署名 ID なし) の .app はビルドごとに別アプリ扱いになり、
# アクセシビリティ権限が毎回外れる。自己署名でも ID が固定なら権限は維持される
# (docs/design.md §5.7、§8 指摘 27)。配布用の Apple Developer ID とは別物。
#
# 使い方: client/scripts/make-signing-identity.sh [ID 名]   (既定: "voice-client Dev")
set -euo pipefail

IDENTITY="${1:-voice-client Dev}"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-identity -v -p codesigning | grep -q "\"$IDENTITY\""; then
    echo "already exists: $IDENTITY"
    exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

cat > ext.cnf <<EOF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$IDENTITY
[ext]
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
basicConstraints=critical,CA:false
subjectKeyIdentifier=hash
EOF
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -keyout key.pem -out cert.pem -config ext.cnf
# macOS の security import は OpenSSL 3 既定の PKCS12 (AES) を読めないため -legacy
PASS="$(openssl rand -hex 16)"
openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -out id.p12 -passout "pass:$PASS" -name "$IDENTITY"
security import id.p12 -k "$KEYCHAIN" -P "$PASS" -T /usr/bin/codesign -T /usr/bin/security
security add-trusted-cert -p codeSign -k "$KEYCHAIN" cert.pem

security find-identity -v -p codesigning | grep "\"$IDENTITY\""
echo "created: $IDENTITY"
