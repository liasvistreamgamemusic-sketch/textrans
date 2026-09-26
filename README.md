# textrans — 音声入力システム (voice-server / voice-client)

カーソルがある入力欄でホットキーを押して話すと、清書済みの日本語 (または英訳・生テキスト) がそのまま入力される仕組み。自宅 LAN 内の GPU サーバーと各 PC の常駐クライアントに分かれる。

設計書: [docs/design.md](docs/design.md) (正本)。

| ディレクトリ | 内容 |
| --- | --- |
| [protocol/](protocol/) | サーバー・クライアント共有の WebSocket プロトコル (JSON Schema + fixtures)。両側はここだけを参照する |
| [server/](server/) | voice-server: FastAPI Gateway (VAD + ASR 内包) と llama-server を systemd ユーザーサービスで動かす。Python 3.12 + uv |
| [server/deploy/](server/deploy/) | サーバー機 (Linux, RTX 2070 SUPER) の構築スクリプトと systemd ユニット |
| [client/](client/) | voice-client: Tauri 2 + Rust + React の常駐アプリ (macOS / Windows) |
| [docs/](docs/) | 設計書 |

## 開発の流れ

```bash
# server (mac でも GPU なしでテストできる)
cd server && uv sync && uv run pytest && uv run ruff check .

# サーバー機へ配置 (初回は server/deploy/README.md の手順)
bash server/deploy/sync.sh

# client
cd client && pnpm install && pnpm tauri dev
```

リリースタグは `server-v1.2.0` / `client-v1.4.0` のように分ける。
