# voice-client

`docs/design.md` §5「プロジェクト2:voice-client」の実装。Tauri 2 + Rust (`src-tauri/`) + React (`src/`)。

## 構成

```
client/
├── src-tauri/          Rust コア
│   ├── src/
│   │   ├── protocol/    WebSocket メッセージ型 (protocol_version = 1、protocol/ 配下の JSON Schema と同期)
│   │   ├── ws/          常時接続・30秒 ping・自動再接続・証明書フィンガープリント固定
│   │   ├── rest/        GET/PUT /v1/dictionary の最小 HTTPS クライアント
│   │   ├── audio/       cpal 録音 → rubato で 16kHz mono PCM16LE、100ms チャンク、300ms プリロール
│   │   ├── state/       発話の状態機械 (押下/解放/Esc/final/エラー、発話順キュー、リピート押下無視)
│   │   ├── insert/      クリップボード貼り付け・直接送出、前面アプリ一致判定、復元判定
│   │   ├── settings/    設定の JSON 永続化、トークンの keyring 保存
│   │   ├── overlay/     状態表示オーバーレイウィンドウ
│   │   ├── orchestrator.rs  上記をつなぐ実行ループ
│   │   └── commands.rs  React から呼ぶ Tauri コマンド
│   └── tauri.conf.json
└── src/                 React (設定画面・辞書エディタ・証明書承認)
```

## ビルド

前提: rustc 1.92 / cargo、node 22、pnpm 10 (macOS で確認済み)。

```sh
cd client
pnpm install
pnpm build            # tsc --noEmit (strict) + vite build → dist/
cd src-tauri
cargo build            # フロントエンドの dist/ が必要 (先に pnpm build すること)
cargo test              # protocol fixtures ラウンドトリップ・状態機械・insert 判定・ws モック接続
cargo clippy --all-targets -- -D warnings
```

開発時に GUI を起動する場合は `cargo tauri dev` (別途 `@tauri-apps/cli` 経由、`pnpm tauri dev`) を使う。

## 初回セットアップ (ペアリング)

初回起動時 (トークン未保存の間) は常駐アプリでもメインウィンドウが自動で前面に出る。設定画面
先頭の「ペアリング」カードで、以下の手順だけで接続設定が完了する:

1. サーバー側で `voice-server pair` を実行し、6桁のペアリングコードを発行する。
2. voice-client の設定画面で「サーバー URL」(既定 `wss://192.168.11.10:8765`) と、発行された
   6桁コードを入力して「ペアリング」ボタンを押す。
3. 内部では (a) TOFU でサーバー証明書のフィンガープリントを取得 → (b) そのフィンガープリントで
   固定した TLS 越しに `POST /v1/pair` を呼ぶ → (c) 応答のフィンガープリントが (a) と一致することを
   検証 (不一致ならなりすましの可能性として保存を中止) → (d) 発行された token を OS の
   キーチェーン/資格情報マネージャーへ、フィンガープリントとサーバー URL を設定ファイルへ保存する。
4. 成功すると「接続設定完了。サーバーのフィンガープリント: XX:XX:...」と表示され、常時接続が
   自動的に始まる (再起動不要、設定スロットを共有しているため)。
5. コードが無効・期限切れの場合は 403 が返り、「コードが無効か期限切れ。サーバーで
   `voice-server pair` を実行し直してください」と表示される。サーバーで再度 `voice-server pair`
   を実行してやり直す。

device 名 (サーバー側の識別名) は既定でこのマシンのホスト名から自動生成される (英数字・`-`・`_`
以外の文字は `-` に置換し、64 文字を超える分は切り捨てる)。

証明書だけを個別に確認・承認したい場合 (上級者向け) は、「サーバー証明書」タブの折りたたみ内に
従来通りの手動経路 (取得→目視確認→承認、または `voice-server cert fingerprint` の出力を直接貼り付け)
が残っている。

## macOS の権限

- マイク: `src-tauri/Info.plist` に `NSMicrophoneUsageDescription` を記載済み。Tauri の macOS バンドラーが
  `src-tauri/Info.plist` をアプリの Info.plist へマージする前提 (`cargo build` では確認できず、
  `cargo tauri build` でのバンドル時に確認が必要。未検証)。
- アクセシビリティ権限: キー送信 (enigo) に必要。初回起動時に設定画面へ誘導する導線は未実装
  (settings 画面に案内文を追加する形で拡張できる)。
- 署名: 権限は署名 ID に紐づく。配布時は固定の署名 ID で署名する (本実装では未設定)。

## Windows

`cfg(target_os = "windows")` で最小実装にとどめている箇所:

- 前面アプリ判定 (`insert::current_front_app`): `GetForegroundWindow` のハンドル値を識別子にするのみで、
  プロセス名までは解決しない。
- クリップボード変更カウンタ (`insert::clipboard_change_count`): `windows` crate 0.61 系の
  `Win32::System::DataExchange::GetClipboardSequenceNumber` を呼ぶ実装に変更した。
  **⚠️ この関数を含む `cfg(target_os = "windows")` 配下は、macOS 上のこのリポジトリでは
  コンパイル・実機動作の確認が一切できていない。** `windows` crate に
  `Win32_System_DataExchange` フィーチャー (および `GetClipboardSequenceNumber`) が存在すること自体は
  crates.io / docs.rs で確認済みだが、実際のシグネチャ一致や実機での戻り値の妥当性は未検証。

ビルド確認のみが目的のため、`cargo build --target x86_64-pc-windows-msvc` 等は本セッションでは実行していない
(クロスコンパイルのツールチェイン未導入。README のとおりコードは `cfg` で分岐しており、Windows 実機でのビルド確認が必要)。

## 既知の簡略化事項 (design.md からの差分)

- **前面ウィンドウの識別**: design.md §5.5 は「アプリとウィンドウID」を記録するとしているが、
  標準 API でウィンドウ単位まで識別するコストが高いため、初版は macOS のバンドル ID 単位の比較にとどめている。
- **クリップボード履歴除外 (`org.nspasteboard.ConcealedType`)**: design.md 自身が明記する通り慣習であり、
  全ての履歴管理ツールが従う保証はない。
- **初回フィンガープリント承認**: `probe_fingerprint` コマンドは検証を一切行わない Trust-On-First-Use の
  取得専用。ユーザーが `voice-server cert fingerprint` の出力と目で見比べて `approve_fingerprint` を呼ぶまで、
  常時接続は開始しない (`server_fingerprint_hex` が `None` の間は接続ループが待機する)。
  `voice-server cert fingerprint` の表示形式 (コロン区切り・大文字) とクライアントの内部形式
  (コロン無し・小文字) は `normalize_fingerprint` (Rust: `ws::verifier`、TS: `src/fingerprint.ts`) で
  両方揃える。承認 UI には「取得した値をそのまま承認」と「手動で貼り付けて承認」の2経路があり、
  どちらも正規化を経由する。
- **常時接続中のマイク**: 「マイクを常時開いておく」設定時は、アプリ起動時に (常時接続の確立前でも)
  マイクを1本だけ開き、押している間だけそのチャンクをサーバーへの配送対象に切り替える。
- **設定変更の反映タイミング**: `Orchestrator` は起動時に設定のスナップショットを持つだけで、
  設定画面で保存した変更 (ホットキー・マイクデバイス・常時オープン等) を実行中に取り込む仕組みは
  まだ無い。反映には再起動が必要 (次の改修候補として認識している既知の制約)。

## テスト内容と検証結果

`cargo test` (2026-09-27、macOS / rustc 1.92 / cargo 1.92 で実行、64 tests):

- `protocol::tests`: `protocol/fixtures/valid/*.json` の全件を `ClientMessage`/`ServerMessage`/`Dictionary` へ
  デシリアライズ→シリアライズ→再デシリアライズしてラウンドトリップを確認。`protocol/fixtures/invalid/*.json` は
  全件デシリアライズ失敗を確認 (ファイル名の `_` 前までをスキーマ名として自動判定)。
- `state::tests`: 押下/解放/Esc/final(空)/final(非空)/エラー・タイムアウトの遷移、リピート押下の無視、
  発話順キュー (後から始めた発話の final が先着しても順番を守る)、`final` 待ちタイムアウト
  (`on_timeout`) が `WaitingFinal` のセッションだけを諦め、既に `ReadyToInsert` になったセッションを
  横取りして消さないことを検証。
- `insert::tests`: 前面アプリ一致判定、クリップボード復元判定 (変更カウンタ比較)、アプリ別待機時間の解決を
  純粋関数として検証 (実際の OS API 呼び出し部分はテスト対象外、下記参照)。
- `ws::verifier::tests`: `fingerprint_hex`/`normalize_fingerprint` の正規化 (コロン区切り・大文字と
  コロン無し・小文字が同じ値に揃うこと、不正な長さ・非16進文字の拒否)、`FingerprintVerifier` が
  コロン区切り大文字の承認値でも実際の証明書と一致すること、不一致の拒否、`CapturingVerifier` の TOFU 動作を検証。
- `ws::tests`: `rcgen` で自己署名証明書を生成し、`tokio-rustls` + `tokio-tungstenite` のローカル TLS
  モックサーバーを立てて `start → PCM(3,200バイト) → end → final` の往復を実施。`PendingQueue` が
  未接続中も120秒キャップを保ちつつ FIFO で取り出せること (レビュー指摘4)、`pop()` が push を待てること、
  pong (または他の受信) が無いまま `PONG_TIMEOUT` を超えたら `drive_connection` がエラーで抜けて
  再接続ループに戻れること (レビュー指摘5、間隔を短縮したテスト用呼び出しで検証) を確認。
- `audio::tests`: ダウンミックス、PCM16LE エンコード (クリップ含む)、100ms チャンク分割と端数の持ち越し、
  300ms プリロールバッファの保持範囲、rubato による 48kHz→16kHz リサンプル (フレーム数が概ね1/3になること)、
  `MonoResampler` の再利用バッファが呼び出しごとに正しく更新されること (レビュー指摘7) を検証。
- `orchestrator::tests`: マイクの実配線 (レビュー指摘2) のうち cpal/AppHandle を含まない純粋部分
  (`route_audio_chunk`/`snapshot_and_start_routing`/`stop_routing`) — 録音していない間はプリロールに
  積むだけで配送しない、押下後は配送を始める (かつ押下前のプリロールを返す)、解放後は配送を止めるが
  プリロールへの蓄積は続くこと — を検証。
- `settings::tests`: 既定値が design.md §5.8 と一致すること、設定ファイルの保存・読込のラウンドトリップ、
  履歴の直近20件保持を検証。
- `rest::tests`: `wss://host:port` の解析、HTTP レスポンスのステータス/ボディ解析 (Content-Length あり/なし) を検証。

`cargo build` / `pnpm build` (`tsc --noEmit` strict + `vite build`) は成功。`cargo clippy --all-targets -- -D warnings`
は警告ゼロ。

### tauri-plugin-global-shortcut の押下/解放イベント (§8.2 の確認事項)

`tauri-plugin-global-shortcut` 2.3.2 の `ShortcutState` enum (docs.rs で確認) は `Pressed` / `Released` の
両バリアントを持ち、`Builder::with_handler` のハンドラで `ShortcutEvent::state()` から両方を受け取れる
(型定義を確認済み、`lib.rs` の `with_handler` 実装もこの前提で書いている)。**ただし実機のホットキー押下による
動作確認はしていない** (GUI を操作するテストはこのセッションでは実行不可)。設計書 8.2 の懸念は API レベルでは
解消しているが、実機確認は未完了として残る。

## 未実装・未検証項目 (正直な報告)

- 実機での動作確認 (ホットキー押下→挿入、クリップボード復元、フォーカス不一致時の通知、
  マイクの実配線を含む音声パイプライン全体) は一切していない。サーバー (`server/`) も
  本セッションでは未完成のため、実サーバーに対する接続確認もしていない。
- Windows: 前面アプリ判定・クリップボード変更カウンタ (`GetClipboardSequenceNumber` 呼び出しに変更済み) が
  macOS 上のこのリポジトリではコンパイル・実機動作の確認が一切できていない (上記「Windows」節)。
  ビルド確認自体も macOS 上でのクロスコンパイルは行っていない。
- アクセシビリティ権限への誘導 UI (macOS) は未実装。
- 署名 (固定の署名 ID) は未設定。
- `tauri build` によるアプリバンドル生成 (署名・実際のインストーラ作成) はまだ実行していない。
  アイコン自体は `src-tauri/icons/source.svg` (マイクをモチーフにした 1024×1024 SVG) から
  `pnpm tauri icon src-tauri/icons/source.svg` で生成済みで、`tauri.conf.json` の
  `bundle.icon` に列挙し、macOS 用 `icon.icns` の生成も確認済み。プレースホルダーの単色 PNG は
  置き換えた。
- 「挿入後に挿入範囲を選択状態にする」設定 (§5.6) は設定項目として保持しているが、実際に選択状態にする
  Shift+←の送出ロジックは未実装 (`insert` モジュールに追加する形で拡張できる)。
- IME 未確定文字が残った状態での貼り付け挙動 (design.md 8.2 の指摘15) は手動 E2E 前提のため未確認。
- **設定のライブ反映**: 実行中に設定画面で保存した値 (ホットキー・マイクデバイス・常時オープン等) は
  `Orchestrator` に届かず、次回起動まで反映されない (今回のレビュー対応で見つけた既知の制約。
  依頼された8項目には含まれていなかったため、README に明記するだけで今回は直していない)。
- TypeScript 側の `normalizeFingerprint` (`src/fingerprint.ts`) は Rust 側 `normalize_fingerprint` と
  同じ規則で実装したが、プロジェクトに JS 側のテストランナー (vitest 等) が無いため、
  型チェック (`tsc --noEmit` strict) 以外の自動テストは書けていない。ロジックは Rust 側のテストで
  規則自体は検証済みだが、TS 実装そのものの単体テストは無い。
