# protocol — voice-server / voice-client 共有プロトコル

設計書 §3 の正本。サーバー (`server/`) とクライアント (`client/`) はここだけを参照し、自前でメッセージ形を再定義しない。

## 版数表

| protocol_version | 状態 | 変更 |
| --- | --- | --- |
| 1 | 現行 | 初版 |

サーバーは現行と1つ前の版を受け付ける。`start.protocol_version` が範囲外なら `error(code=unsupported_version)` を返して切断する。

## フレーミング規則

- WebSocket エンドポイント: `wss://<host>:8765/v1/dictate`。ヘッダー `Authorization: Bearer <token>` 必須 (失敗は HTTP 401 でアップグレード拒否)。
- テキストフレーム = UTF-8 JSON、1 フレーム 1 メッセージ。`type` フィールドでスキーマ (`v1/<type>.schema.json`) を選ぶ。
- バイナリフレーム = PCM 16bit little-endian、16kHz、モノラル。直前の `start` のセッションに属する。100ms (3,200 バイト) 単位が目安で、サイズは固定ではない。
- `start` 前・`end` 後のバイナリフレームは `error(code=invalid_message)` (retryable=false)。
- 1 接続で同時に有効な `start` は 1 つ。前の `end` の後、`final` を待たずに次の `start` を送ってよい (サーバーは発話順に `final` を返す)。
- スキーマは `additionalProperties: false`。未知フィールドは拒否する。

## fixtures

`fixtures/valid/<type>*.json` は `v1/<type>.schema.json` に通らなければならない。
`fixtures/invalid/<type>_*.json` は `v1/<type>.schema.json` で拒否されなければならない。
両側の CI はこの規則で契約テストを回す (ファイル名の先頭 `_` までがスキーマ名)。
