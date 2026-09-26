#!/usr/bin/env bash
# nvidia-smi のVRAM使用量ピークを記録するベンチ用ヘルパー(design.md §4.6)。
# voice-llama / voice-gateway (+ faster-whisper) を実際に動かした状態で実行し、
# 「ASR単独」「LLM単独」「両方同時」のピークをそれぞれ記録する用途。
#
# 使い方(サーバー機上で):
#   server/deploy/gpu-peak.sh                 # Ctrl-C まで1秒間隔でサンプリング
#   server/deploy/gpu-peak.sh 60               # 60秒だけサンプリング
#   server/deploy/gpu-peak.sh 60 0.5           # 60秒、0.5秒間隔
set -euo pipefail

DEFAULT_DURATION_SEC=0   # 0 = Ctrl-C まで無制限
DEFAULT_INTERVAL_SEC=1

DURATION_SEC="${1:-$DEFAULT_DURATION_SEC}"
INTERVAL_SEC="${2:-$DEFAULT_INTERVAL_SEC}"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./deploy.env
source "$SCRIPT_DIR/deploy.env"

LOG_FILE="${LOGS_DIR}/gpu-peak-$(date +%Y%m%d-%H%M%S).log"
mkdir -p "$LOGS_DIR"

if ! command -v nvidia-smi >/dev/null 2>&1; then
    echo "error: nvidia-smi が見つからない" >&2
    exit 1
fi

peak=0
start_ts=$(date +%s)
echo "sampling memory.used every ${INTERVAL_SEC}s (duration=${DURATION_SEC:-unlimited}s). log: $LOG_FILE"
trap 'echo "peak: ${peak} MiB" | tee -a "$LOG_FILE"' EXIT

while true; do
    used="$(nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits | head -n1)"
    ts="$(date +%H:%M:%S)"
    echo "${ts} used=${used}MiB" | tee -a "$LOG_FILE"
    if (( used > peak )); then
        peak="$used"
    fi
    if (( DURATION_SEC > 0 )); then
        now=$(date +%s)
        if (( now - start_ts >= DURATION_SEC )); then
            break
        fi
    fi
    sleep "$INTERVAL_SEC"
done
