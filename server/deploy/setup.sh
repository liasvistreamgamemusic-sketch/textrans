#!/usr/bin/env bash
# サーバー機のセットアップ(sudo 不要、再実行可)。design.md §4.8 の手順1。
#
# 前提: このスクリプトはサーバー機(tomoya@192.168.11.10)上で実行する。
#       server/ の配置は事前に mac から server/deploy/sync.sh で rsync 済みであること。
# sudo が必要な手順(linger / ufw / スリープ無効化)は setup-root.sh に分離してあり、
# このスクリプトは呼ばない。
#
# 使い方(サーバー機上で):
#   ~/voice/app/deploy/setup.sh
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./deploy.env
source "$SCRIPT_DIR/deploy.env"

log() { echo "[setup] $*"; }
warn() { echo "[setup] WARNING: $*" >&2; }

# --- 1. ディレクトリ ---
log "ディレクトリ作成: $VOICE_HOME/{app,config,models,data,logs}"
mkdir -p "$APP_DIR" "$CONFIG_DIR" "$MODELS_DIR" "$DATA_DIR" "$LOGS_DIR"

# --- 2. uv ---
if ! command -v uv >/dev/null 2>&1 && [[ ! -x "$HOME/.local/bin/uv" ]]; then
    log "uv を導入"
    curl -LsSf https://astral.sh/uv/install.sh | sh
else
    log "uv は導入済み"
fi
export PATH="$HOME/.local/bin:$PATH"

# --- 3. Python 3.12 ---
log "Python 3.12 を uv で取得"
uv python install 3.12

# --- 4. アプリの配置確認 ---
if [[ ! -f "$APP_DIR/pyproject.toml" ]]; then
    warn "$APP_DIR/pyproject.toml が無い。先に mac 側で server/deploy/sync.sh を実行して配置すること。"
    warn "uv sync 以降の手順をスキップする。"
else
    # --- 5. uv sync ---
    log "uv sync --extra gpu --extra vad"
    (cd "$APP_DIR" && uv sync --extra gpu --extra vad)

fi

# --- 7. 設定ファイルの複製(既存があれば上書きしない) ---
if [[ -f "$APP_DIR/config/server.example.yaml" && ! -f "$CONFIG_DIR/server.yaml" ]]; then
    log "server.yaml を複製"
    cp "$APP_DIR/config/server.example.yaml" "$CONFIG_DIR/server.yaml"
    chmod 0600 "$CONFIG_DIR/server.yaml"
fi
if [[ -f "$APP_DIR/config/dictionary.example.yaml" && ! -f "$CONFIG_DIR/dictionary.yaml" ]]; then
    log "dictionary.yaml を複製"
    cp "$APP_DIR/config/dictionary.example.yaml" "$CONFIG_DIR/dictionary.yaml"
fi
[[ -f "$CONFIG_DIR/tokens.yaml" ]] || { touch "$CONFIG_DIR/tokens.yaml"; chmod 0600 "$CONFIG_DIR/tokens.yaml"; }

# --- 7b. 証明書(設定ファイルの複製後でないと cert ensure が失敗する)
if [[ -f "$APP_DIR/pyproject.toml" ]]; then
    # --- 6. 証明書・トークン(voice-server CLI が無ければ警告のみで続行) ---
    mkdir -p "$CONFIG_DIR/tls"
    chmod 0700 "$CONFIG_DIR/tls"
    VOICE_SERVER_BIN="$APP_DIR/.venv/bin/voice-server"
    if [[ -x "$VOICE_SERVER_BIN" ]]; then
        log "証明書を確認(無ければ生成)"
        if ! "$VOICE_SERVER_BIN" cert ensure --config "$CONFIG_DIR/server.yaml"; then
            warn "voice-server cert ensure に失敗(server 側の実装待ちの可能性)。手動確認が必要"
        fi
    else
        warn "$VOICE_SERVER_BIN が無い。証明書生成とトークン発行は server 側の実装待ち"
    fi
fi

# --- 8. LLM モデル取得(既にあればスキップ) ---
LLM_MODEL_PATH="$MODELS_DIR/$LLM_MODEL_FILE"
if [[ -f "$LLM_MODEL_PATH" ]]; then
    log "LLM モデルは既に取得済み: $LLM_MODEL_PATH"
else
    log "LLM モデルを取得: $LLM_MODEL_REPO / $LLM_MODEL_FILE"
    uv tool run --from huggingface_hub hf download "$LLM_MODEL_REPO" "$LLM_MODEL_FILE" --local-dir "$MODELS_DIR"
fi

# --- 9. faster-whisper 用 venv(ASRの初版既定。design.md §4.2, §6.1) ---
if [[ -x "$WHISPER_VENV_DIR/bin/python3" ]] && "$WHISPER_VENV_DIR/bin/python3" -c "import faster_whisper" 2>/dev/null; then
    log "faster-whisper venv は既にセットアップ済み"
else
    log "faster-whisper 用 venv を作成: $WHISPER_VENV_DIR"
    uv venv --python 3.12 "$WHISPER_VENV_DIR"
    # cuBLAS/cuDNN はドライバ595 / CUDA13.2環境向けの実機確認済み組み合わせ。
    # venv内に閉じるためシステムへの影響はない。
    "$WHISPER_VENV_DIR/bin/python3" -m pip install --quiet \
        faster-whisper nvidia-cublas-cu12 nvidia-cudnn-cu12
fi

# --- 10. systemd ユーザーユニットの配置(テンプレートの@@..@@をdeploy.envの値で置換) ---
log "systemd ユニットを配置: $SYSTEMD_USER_DIR"
mkdir -p "$SYSTEMD_USER_DIR"
sed \
    -e "s|@@LLM_MODEL_FILE@@|${LLM_MODEL_FILE}|g" \
    -e "s|@@LLAMA_HOST@@|${LLAMA_HOST}|g" \
    -e "s|@@LLAMA_PORT@@|${LLAMA_PORT}|g" \
    -e "s|@@LLAMA_NGL@@|${LLAMA_NGL}|g" \
    -e "s|@@LLAMA_CTX_SIZE@@|${LLAMA_CTX_SIZE}|g" \
    -e "s|@@LLAMA_FLASH_ATTN@@|${LLAMA_FLASH_ATTN}|g" \
    "$SCRIPT_DIR/voice-llama.service" > "$SYSTEMD_USER_DIR/voice-llama.service"
cp "$SCRIPT_DIR/voice-gateway.service" "$SYSTEMD_USER_DIR/voice-gateway.service"

# --- 11. 有効化・起動 ---
log "systemctl --user daemon-reload"
systemctl --user daemon-reload

log "voice-llama を起動"
systemctl --user enable --now voice-llama.service

if [[ -x "$APP_DIR/.venv/bin/voice-server" ]]; then
    log "voice-gateway を起動"
    if ! systemctl --user enable --now voice-gateway.service; then
        warn "voice-gateway の起動に失敗。journalctl --user -u voice-gateway で確認すること"
    fi
else
    warn "voice-server CLI が無いため voice-gateway は起動しない(server 側の実装待ち)"
    warn "server/ が揃ったら sync.sh 実行後にこのスクリプトを再実行すること"
fi

log "完了。状態確認: systemctl --user status voice-llama voice-gateway"
