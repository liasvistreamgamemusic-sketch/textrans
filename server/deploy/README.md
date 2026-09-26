# server/deploy — サーバー機の構築(列D)

対象機: `tomoya@192.168.11.10`(Ubuntu, RTX 2070 SUPER 8GB)。正本は `docs/design.md` §4.6・§4.8・§8.2。
このディレクトリのスクリプトはすべて **sudo 不要**。sudo が要る手順だけ `setup-root.sh` に分離してある。

## ファイル一覧

| ファイル | 役割 |
| --- | --- |
| `deploy.env` | 全スクリプトが読む設定値(IP・パス・モデル名など)。ハードコードはここに集約 |
| `sync.sh` | mac から `server/` をサーバー機の `~/voice/app/` へ rsync するヘルパー(mac 側で実行) |
| `setup.sh` | サーバー機側のセットアップ本体(sudo不要、冪等・再実行可) |
| `setup-root.sh` | sudo が必要な手順(linger / ufw / スリープ無効化)。ユーザーが手動で1回実行 |
| `voice-llama.service` | systemd ユーザーユニット(テンプレート。`@@..@@` を setup.sh が置換) |
| `voice-gateway.service` | systemd ユーザーユニット(そのまま配置) |
| `gpu-peak.sh` | `nvidia-smi` のVRAMピークを記録するベンチ用ヘルパー |

## 手順

1. **mac側**: リポジトリルートで `server/deploy/sync.sh` を実行し、`server/` を `~/voice/app/` へ配置する。
2. **サーバー機側**: `~/voice/app/deploy/setup.sh` を実行する(sudo不要、何度でも再実行可)。
   - `server/` の配置前に実行しても、ディレクトリ作成・uv導入・Python 3.12取得・LLMモデル取得・
     faster-whisper venv構築・`voice-llama` の起動までは完了する(`uv sync` 以降は
     `pyproject.toml` が無ければ警告を出してスキップする)。
   - `server/` 配置後に再実行すると `uv sync --extra gpu --extra vad`・証明書生成・
     設定複製・`voice-gateway` の起動まで進む。
3. **サーバー機側(手動・1回だけ)**: `~/voice/app/deploy/setup-root.sh` を実行する(sudo パスワードが必要)。
   - `loginctl enable-linger tomoya`、ufw(LANサブネットのみ許可)、自動スリープ無効化を行う。
4. トークン発行: `~/voice/app/.venv/bin/voice-server token issue <端末名>` と
   `~/voice/app/.venv/bin/voice-server cert fingerprint` の値をクライアントに設定する
   (server 側の CLI 実装後)。

## 確認コマンド

```sh
systemctl --user status voice-llama voice-gateway
journalctl --user -u voice-llama -f
curl -k https://192.168.11.10:8765/healthz     # gateway 実装後
curl http://127.0.0.1:8081/v1/chat/completions -H 'Content-Type: application/json' \
    -d '{"messages":[{"role":"user","content":"こんにちは"}],"max_tokens":20}'
nvidia-smi --query-gpu=memory.used --format=csv,noheader
```

## 実機確認結果(2026-09-26、実測)

| 項目 | 内容 | 区分 |
| --- | --- | --- |
| LLM GGUF の実在 | `unsloth/Qwen3.5-4B-GGUF` の `Qwen3.5-4B-Q4_K_M.gguf` は実在(HF API `/api/models/...` で確認、`siblings` に列挙) | 実測 |
| LLM モデルサイズ | 2.6GB(ダウンロード実測) | 実測 |
| llama-server 起動 | 既存ビルド(2026-08-08版、`version 1 (69bf643)`)で Qwen3.5-4B GGUF がそのまま読めた。再ビルド不要 | 実測 |
| `--reasoning-budget 0` の思考タグ | **単独では不十分**。`<think>` の内容が `reasoning_content` に入り、`max_tokens` を使い切って本文(`content`)が空になるケースを確認(finish_reason=length)。`-rea off` を併用すると解消し、`content` に清書結果のみが入った | 実測・不具合再現 |
| Flash Attention (`-fa on` / `-fa off`) | 速度差はほぼ無し(約100〜103 tok/s、3回平均でどちらも誤差の範囲)。VRAMは `-fa on` の方が僅かに少ない(3211MiB vs 3346MiB)。**既定は `-fa on`** | 実測 |
| llama-server 応答時間(短文清書、20トークン) | 約0.2〜0.3秒(`predicted_ms` 実測 216〜253ms、約100 tok/s) | 実測 |
| VRAM: llama-server 単独 | 3211 MiB(`-fa on`、Qwen3.5-4B Q4_K_M、ctx=4096) | 実測 |
| VRAM: faster-whisper 単独 | 2139 MiB(モデルロード直後)〜2281 MiB(推論中)。`large-v3-turbo` / `compute_type=float16` | 実測 |
| VRAM: 両方同時ロード | 5265 MiB(ロード直後)〜**5407 MiB(whisper推論中のピーク)** | 実測 |
| VRAM予算判定 | 6.8GB以下の基準に対し実測ピーク 5407 MiB。**基準を満たす**(§4.6) | 実測 |
| faster-whisper の CUDA依存 | `nvidia-cublas-cu12` / `nvidia-cudnn-cu12` を venv に pip 導入するだけで動作した。`LD_LIBRARY_PATH` の明示設定は不要だった(ctranslate2側で解決している) | 実測 |
| faster-whisper transcribe | 3秒の合成音声(正弦波、無音ではない)に対し例外なく完了(0.37秒、モデルキャッシュ後) | 実測 |
| GPU ドライバ / CUDA | ドライバ595.84 / `nvcc` 12.4(既知の事実。design.md §1.1) | 既知(検証済み) |
| uv / Python 3.12 導入 | `~/.local/bin/uv` に導入、`uv python install 3.12` 完了 | 実測 |
| voice-llama.service 実起動 | `systemctl --user enable --now voice-llama` → `active (running)`。curl で応答確認済み。再実行(冪等性)も確認済み | 実測 |
| voice-gateway 実起動 | **未確認**。`server/` 未配置(server列の完成待ち)のため `pyproject.toml` が無く、`setup.sh` は警告を出してスキップした。`voice-server` CLI 完成後に `sync.sh` → `setup.sh` の再実行が必要 | 未確認(指示どおり) |

## 未解決・要フォローアップ

- Qwen3.5-4B の清書内容そのもの(意味保持・敬語化しないか等)は server 列の出力ガードで検証すること。
  今回の確認は「思考タグが混入しない」「応答が返る」までで、簡単な例文で軽い敬語化(「しました」→「いたしました」)が
  起きているのを目視で確認した。ガード条件・システムプロンプトの検証は server 列の担当。
- `voice-gateway.service` は ExecStart のパス・引数を実起動で確認できていない(`voice-server` 未実装のため)。
  server 列完成後に本 README の該当行を更新すること。
- ASR比較(6.1の候補)は未実施。初版既定の `faster_whisper` の単独動作のみ確認した。
