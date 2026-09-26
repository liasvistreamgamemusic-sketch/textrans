#!/usr/bin/env bash
# mac の開発機からサーバー機へ server/ を配置するヘルパー(design.md §4.8)。
# setup.sh 自体はサーバー機上で実行するため、rsync による配置はこのスクリプトが
# 事前に(mac 側から)行う。
#
# 使い方: リポジトリルートで実行する
#   server/deploy/sync.sh
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=./deploy.env
source "$SCRIPT_DIR/deploy.env"

if [[ ! -d "$REPO_ROOT/server" ]]; then
    echo "error: $REPO_ROOT/server が見つからない(リポジトリルートで実行しているか確認)" >&2
    exit 1
fi

echo "sync: $REPO_ROOT/server -> ${REMOTE_HOST}:~/voice/app/"
rsync -az --delete \
    --exclude .venv \
    --exclude __pycache__ \
    --exclude '*.pyc' \
    --exclude .pytest_cache \
    --exclude .ruff_cache \
    -e "ssh -i ${REMOTE_SSH_KEY}" \
    "$REPO_ROOT/server/" "${REMOTE_HOST}:voice/app/"

# protocol/ は server が実行時に読む (server/ の上位ディレクトリを辿って探す)
echo "sync: $REPO_ROOT/protocol -> ${REMOTE_HOST}:~/voice/protocol/"
rsync -az --delete -e "ssh -i ${REMOTE_SSH_KEY}" "$REPO_ROOT/protocol/" "${REMOTE_HOST}:voice/protocol/"
echo "done."
