# voice-server

音声入力システムの Gateway。ASR (音声認識) と LLM 清書を1プロセスの FastAPI アプリで提供する。
詳細仕様は `../docs/design.md` (§4) と `../protocol/README.md` が正本。

## セットアップ (開発機 / mac、GPU なし)

```sh
cd server
uv sync                 # dummy backend + Silero VAD なしで開発・テストできる範囲
uv run pytest
uv run ruff check
```

`gpu` (faster-whisper) と `vad` (Silero VAD, torch) は重い依存のため既定の `uv sync` には含めない。
実機 (GPU サーバー) では:

```sh
uv sync --extra gpu --extra vad
```

## 起動

```sh
voice-server serve --config ~/voice/config/server.yaml
```

初回起動時に TLS 証明書 (`server.tls_cert_path` / `tls_key_path`) が無ければ自動生成する。

## CLI

| コマンド | 用途 |
| --- | --- |
| `voice-server serve --config <path>` | Gateway を起動する |
| `voice-server token issue <端末名> [--config <path>]` | トークンを発行する (平文は1回だけ表示) |
| `voice-server token revoke <端末名> [--config <path>]` | トークンを失効させる |
| `voice-server token list [--config <path>]` | 発行済み端末名と発行日時を一覧する |
| `voice-server cert ensure [--config <path>]` | 自己署名証明書が無ければ生成する |
| `voice-server cert fingerprint [--config <path>]` | 証明書の SHA-256 フィンガープリントを表示する |
| `voice-server pair [--config <path>] [--ttl <秒>]` | ペアリング用の6桁コードを発行する (既定 TTL 300秒) |

`--config` の既定値は `config/server.yaml` (カレントディレクトリ基準)。

## ペアリング (トークンを手元で `token issue` せずに端末から取得する)

`pairing.mode` で挙動が変わる (既定 `"open"`)。

### `open` モード (既定)

コード不要で、端末は無認証で `POST /v1/pair` (`{"device": "<端末名>"}`。`code` は省略可・
渡しても無視される) を呼ぶだけで無条件にトークンが発行される。`{"device", "token", "fingerprint"}`
を受け取ってトークンと証明書フィンガープリントを保存する。同名 device での再要求は新トークンを
発行し、旧トークンは自動的に失効する (端末の再インストール時に再ペアリングできるようにするため)。
ログには device 名だけを記録する (INFO)。

**前提**: このモードは LAN 内を ufw 等で外部から閉じ、利用者が単一ユーザーである環境向け。
外出先から VPN 経由で Gateway に接続する場合は、認証なしでトークンを発行してしまうため
`pairing.mode: "code"` に切り替えること。

### `code` モード

1. Gateway 側で `voice-server pair --config ~/voice/config/server.yaml` を実行する。
   6桁の数字コードと有効期限が表示される (この1回だけ)。
2. クライアントでそのコードを入力する。クライアントは無認証で
   `POST /v1/pair` (`{"device": "<端末名>", "code": "<6桁コード>"}`) を呼び、
   `{"device", "token", "fingerprint"}` を受け取ってトークンと証明書フィンガープリントを保存する。
3. コードは成功時に1回だけ使用でき、以後同じコードでの `/v1/pair` は 403 になる
   (`pairing.yaml` を削除して使い切りにする)。有効期限切れ・コード不一致も同じ 403 で、
   区別しない (総当たり対策)。失敗時は `limits.pair_failure_delay_s` 秒待ってから応答する。

## 設定 (`config/server.example.yaml` を複製して編集する)

| セクション | 項目 | 既定値 | 説明 |
| --- | --- | --- | --- |
| `server` | `bind` / `port` | `0.0.0.0` / `8765` | Gateway の待ち受け |
| `server` | `tls_cert_path` / `tls_key_path` | `~/voice/config/tls/server.{crt,key}` | 自己署名証明書 |
| `tokens` | `path` | `~/voice/config/tokens.yaml` | トークンの SHA-256 ハッシュ保存先 (0600) |
| `dictionary` | `path` | `~/voice/config/dictionary.yaml` | 辞書の正本 |
| `pairing` | `path` | `~/voice/config/pairing.yaml` | `voice-server pair` が書くコードのハッシュ + 有効期限 (0600、使い切りで削除。`code` モードのみ使用) |
| `pairing` | `mode` | `open` | `open` (コード不要、LAN 内単一ユーザー向け) / `code` (6桁コード必須。VPN 経由等の外出先利用ではこちら) |
| `asr` | `backend` | `faster_whisper` | `dummy` (テスト用) / `faster_whisper` / (bench候補: 未実装) |
| `asr` | `model` / `model_path` / `compute_type` / `device` | `large-v3-turbo` / なし / `float16` / `cuda` | ASR モデル |
| `asr` | `strategy` | `segmented` | `segmented` / `whole` |
| `asr` | `context_chars` | `100` | 前セグメント末尾の文脈長 |
| `vad` | `silence_ms` / `max_segment_s` / `min_segment_s` | `600` / `15` / `1` | セグメント分割 |
| `llm` | `base_url` | `http://127.0.0.1:8081` | llama-server (OpenAI互換) |
| `llm` | `timeout_base_ms` / `timeout_per_char_ms` | `1500` / `15` | タイムアウト計算式 |
| `llm` | `temperature` / `model` | `0.1` / `Qwen3.5-4B-Q4_K_M.gguf` | 生成パラメータ |
| `limits` | `max_utterance_s` / `queue_size` / `end_grace_s` | `120` / `2` / `5` | 上限とキュー (同時受理数 = `queue_size + 1`) |
| `limits` | `max_dictionary_body_bytes` | `1048576` | `PUT /v1/dictionary` の本文サイズ上限 (超過は413)。`POST /v1/pair` も流用する |
| `limits` | `pair_failure_delay_s` | `1.0` | `POST /v1/pair` 失敗時のスリープ秒数 (総当たり抑止) |
| `recording` | `enabled` / `dir` | `false` / `~/voice/data` | bench 用録音 (既定オフ) |
| `logging` | `level` | `INFO` | 処理時間・モデル名・フラグのみ記録。本文は出さない |

## 未実装・未検証 (このリポジトリ内で確認できていないこと)

- `asr.backend`: `qwen3_asr` / `cohere_transcribe` は bench 用の比較候補で未実装 (`NotImplementedError`)。
- `llm_client.check_ready()` は `GET {base_url}/health` を叩く実装だが、llama.cpp server がこのパスを
  実際に提供するかは実機で未確認 (⚠️ assumed)。提供されない場合 `/healthz` は `llm: false` を返し続ける
  だけで Gateway 自体は起動する (フェイルセーフ)。
- `end_grace_s` の厳密な意味は設計書 §3.3 の記述がやや曖昧なため、「`end` 受信後、発話の最終処理
  (ASR完了) を待つ最大時間。超過したら空の `final` を返す」という解釈で実装した (assumed)。
- 実機 GPU 環境 (faster-whisper / Silero VAD) では未テスト。`uv sync --extra gpu --extra vad` を
  実機で実行し、`asr.backend: faster_whisper` での warmup / transcribe を確認する必要がある。
- `busy` (キュー満杯) は「サーバー全体で同時に完了待ちの発話数」として実装した。設計書 §3.3 の
  「処理中1件 + キュー `queue_size` 件」を同時受理数 `queue_size + 1` として `InFlightGate` に
  実装している。設計書は「別セッションが処理中」とだけ書いており、接続単位か全体単位かは
  明記されていない (assumed: 全体単位。GPU が単一のASRワーカースレッドで直列化されるため)。
