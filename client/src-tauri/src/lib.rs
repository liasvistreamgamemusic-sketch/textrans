//! voice-client (Tauri 2 + Rust) のライブラリ本体。
//!
//! モジュール構成 (design.md §5.1 の技術スタックに対応):
//! - [`protocol`][]: WebSocket メッセージ型・辞書型 (protocol_version = 1)
//! - [`ws`][]: 常時接続・証明書ピン留め・送信バッファ
//! - [`rest`][]: `GET/PUT /v1/dictionary`・`POST /v1/pair`
//! - [`audio`][]: 録音・リサンプリング・チャンク分割・RMS
//! - [`state`][]: 発話の状態機械
//! - [`insert`][]: クリップボード貼り付け・直接送出
//! - [`settings`][]: 設定の永続化・トークンのファイル保存 (0600)・履歴
//! - [`status`][]: React 側と共有する `Status` の一元管理 (`voice://status`)
//! - [`accessibility`][]: macOS アクセシビリティ権限の確認
//! - [`overlay`][]: 状態表示オーバーレイウィンドウ
//! - [`tray`][]: メニューバー (macOS) / タスクトレイ (Windows) の常駐アイコン
//! - [`orchestrator`][]: 上記をつなぐ実行ループ
//! - [`commands`][]: React (設定画面・辞書エディタ) から呼ばれる Tauri コマンド

pub mod accessibility;
pub mod audio;
pub mod commands;
pub mod insert;
pub mod orchestrator;
pub mod overlay;
pub mod protocol;
pub mod rest;
pub mod settings;
pub mod state;
pub mod status;
pub mod tray;
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use commands::AppState;
use orchestrator::{Orchestrator, OrchestratorEvent};
use settings::{History, Settings};
use status::StatusStore;
use ws::verifier::FingerprintVerifier;
use ws::PendingQueue;

/// 設定ファイル (`settings.json`) とトークンのフォールバックファイルが置かれるディレクトリ。
fn config_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .expect("設定ディレクトリを解決できない (OS の設定ディレクトリ API が失敗)")
}

/// 設定ファイルのパス。`app.path().app_config_dir()` 配下。
fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    config_dir(app).join("settings.json")
}

/// 既定ホットキー (design.md §5.2「両OSとも Ctrl+Shift+Space」)。
fn default_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
}

/// 自動ペアリングの再試行間隔 (実装計画: 「5秒→最大60秒のバックオフ」)。
const AUTO_PAIR_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
const AUTO_PAIR_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // RUST_LOG 未設定時の既定は info (from_default_env だと error のみになり、
    // 「未承認のため接続しない」「接続に失敗」等の warn が見えなかった)。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let outgoing = PendingQueue::new();
    let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel::<protocol::ServerMessage>();
    let (events_tx, events_rx) = mpsc::unbounded_channel::<OrchestratorEvent>();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler({
                    let events_tx = events_tx.clone();
                    move |_app, _shortcut, event| {
                        let outcome = match event.state() {
                            ShortcutState::Pressed => OrchestratorEvent::HotkeyPressed,
                            ShortcutState::Released => OrchestratorEvent::HotkeyReleased,
                        };
                        if let Err(e) = events_tx.send(outcome) {
                            tracing::warn!("ホットキーイベントの送信に失敗: {e}");
                        }
                    }
                })
                .build(),
        )
        .manage(AppState {
            settings: Arc::new(Mutex::new(Settings::default())),
            outgoing: outgoing.clone(),
            status: Arc::new(StatusStore::new(Settings::default().server_url, false, None)),
            history: Arc::new(Mutex::new(History::default())),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_dictionary,
            commands::put_dictionary,
            commands::probe_fingerprint,
            commands::approve_fingerprint,
            commands::has_token,
            commands::pair,
            commands::repair,
            commands::get_status,
            commands::get_history,
            commands::copy_history_item,
            commands::open_accessibility_settings,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            let path = settings_path(&handle);
            let loaded = settings::load(&path).unwrap_or_default();
            let dir = config_dir(&handle);

            // 未ペアリング (トークン未保存) 判定。トークンファイルの読み込み自体が失敗した場合も
            // 安全側 (未ペアリング扱い) にする。
            let paired = match settings::token::get(&dir) {
                Ok(token) => token.is_some(),
                Err(e) => {
                    tracing::warn!("トークンの確認に失敗: {e}。未ペアリングとして扱う");
                    false
                }
            };

            // macOS アクセシビリティ権限。起動時は `kAXTrustedCheckOptionPrompt=true` で確認し、
            // 未許可なら OS がシステム設定への誘導ダイアログを (非同期に) 出す。
            let ax_trusted = accessibility::is_trusted(true);

            {
                let state = app.state::<AppState>();
                state.status.set_server_url(&handle, loaded.server_url.clone());
                state.status.set_paired(&handle, paired);
                state.status.set_fingerprint(
                    &handle,
                    loaded.server_fingerprint_hex.as_deref().map(ws::verifier::to_colon_upper),
                );
                state.status.set_ax_trusted(&handle, ax_trusted);

                let settings_slot = state.settings.clone();
                tauri::async_runtime::block_on(async move {
                    *settings_slot.lock().await = loaded;
                });
            }

            app.global_shortcut().register(default_shortcut())?;

            overlay::ensure_overlay_window(&handle)?;

            // 常駐アプリ化 (実機フィードバック1・2): メニューバー/タスクトレイの常駐アイコンを
            // 常に用意し、メインウィンドウは ✕ で閉じても終了せず隠すだけにする
            // (再表示はトレイの「開く」・macOS の Dock 再クリック `RunEvent::Reopen` から)。
            // `TrayIcon` は参照カウント式で最後の1つが drop されるとアイコンも消える
            // (tauri のドキュメントどおり) ので、`app.manage` でアプリの生存期間ずっと保持する。
            app.manage(tray::build_tray(&handle)?);
            if let Some(window) = handle.get_webview_window("main") {
                let window_for_close = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Err(e) = window_for_close.hide() {
                            tracing::warn!("メインウィンドウの非表示に失敗: {e}");
                        }
                        #[cfg(target_os = "macos")]
                        {
                            let app_handle = window_for_close.app_handle();
                            if let Err(e) =
                                app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory)
                            {
                                tracing::warn!("Dock アイコンの非表示切り替えに失敗: {e}");
                            }
                        }
                    }
                });
            }

            // 未ペアリングで起動した場合は、常駐アプリとして隠れている前提のメインウィンドウを
            // 自動で前面に出す (手動ペアリング・サーバーが code モードの場合の導線として残す)。
            // 自動ペアリング (下記 `auto_pair_forever`) が成功すればすぐに閉じられる想定。
            if !paired {
                tray::show_main_window(&handle);
            }

            let orchestrator_settings = {
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(async { state.settings.lock().await.clone() })
            };
            let (status_arc, history_arc) = {
                let state = app.state::<AppState>();
                (state.status.clone(), state.history.clone())
            };
            let orchestrator = Orchestrator::new(
                handle.clone(),
                orchestrator_settings,
                outgoing.clone(),
                events_tx.clone(),
                status_arc,
                history_arc,
            );
            tauri::async_runtime::spawn(orchestrator.run(events_rx));

            // サーバーからのメッセージをオーケストレーターへ転送する。
            let forward_events_tx = events_tx.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(msg) = incoming_rx.recv().await {
                    if let Err(e) = forward_events_tx.send(OrchestratorEvent::ServerMessage(msg)) {
                        tracing::warn!("サーバーメッセージの転送に失敗: {e}");
                    }
                }
            });

            // 起動時の完全自動接続 (実装計画): トークン未保存なら、ユーザー操作ゼロで
            // ペアリング (code なし) を試み続ける。成功すれば常時接続ループがそのまま拾う。
            if !paired {
                tauri::async_runtime::spawn(auto_pair_forever(handle.clone()));
            }

            // 常時接続 (design.md §3.1)。設定スロットを共有し、承認・変更を再起動なしで拾う。
            let ws_settings = app.state::<AppState>().settings.clone();
            let status_for_connection = app.state::<AppState>().status.clone();
            tauri::async_runtime::spawn(run_connection_forever(
                handle.clone(),
                ws_settings,
                outgoing,
                incoming_tx,
                status_for_connection,
            ));

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("tauri アプリケーションの起動に失敗")
        .run(|app_handle, event| {
            // macOS: Dock アイコン (ウィンドウを閉じた後は非表示) を再クリックしたときの再表示。
            // ウィンドウを ✕ で閉じても終了しない (実機フィードバック1) ぶん、この経路が無いと
            // 常駐アプリなのに Dock からは二度と開けなくなる。
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app_handle);
            }
        });
}

/// 起動時の自動ペアリング (実装計画: 「code なしで自動実行する。失敗したら5秒→最大60秒の
/// バックオフで再試行し続ける」)。既にトークンがある場合は呼び出し元 (`run`) が起動しない。
async fn auto_pair_forever(app: tauri::AppHandle) {
    let mut backoff = AUTO_PAIR_INITIAL_BACKOFF;
    loop {
        let server_url = {
            let state = app.state::<AppState>();
            let guard = state.settings.lock().await;
            guard.server_url.clone()
        };
        match commands::perform_pair(&app, server_url, None, None).await {
            Ok(outcome) => {
                tracing::info!("起動時の自動ペアリングに成功した (fingerprint: {})", outcome.fingerprint_hex);
                return;
            }
            Err(e) => {
                tracing::warn!("起動時の自動ペアリングに失敗: {e}。{backoff:?} 後に再試行する");
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, AUTO_PAIR_MAX_BACKOFF);
    }
}

/// 承認待ちや設定不備のときに設定を読み直すまでの間隔。
const SETTINGS_RECHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// 常時接続の再接続ループ。設定 (URL・フィンガープリント・トークン) は接続を試みるたびに
/// 共有スロットから読み直すので、設定画面で承認・変更した内容は再起動なしで反映される。
/// フィンガープリント未承認 (`server_fingerprint_hex: None`) の間は接続を試みず待機する
/// (初回接続の承認 UI は React 側、design.md §5.8)。
async fn run_connection_forever(
    app: tauri::AppHandle,
    settings: Arc<Mutex<Settings>>,
    outgoing: PendingQueue,
    incoming_tx: mpsc::UnboundedSender<protocol::ServerMessage>,
    status: Arc<StatusStore>,
) {
    let mut backoff = std::time::Duration::from_secs(1);
    const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
    let mut announced_waiting = false;
    let dir = config_dir(&app);

    // `outgoing` (PendingQueue) は接続の有無に関わらず常に同じインスタンスを使う。
    // 未接続の間もキャップ付きで溜まり続け、再接続後は先頭 (古い方) から送信される。
    loop {
        let current = settings.lock().await.clone();
        let Some(fingerprint) = current.server_fingerprint_hex.clone() else {
            if !announced_waiting {
                tracing::warn!("証明書フィンガープリント未承認のため接続しない。設定画面で承認すると自動で接続する");
                announced_waiting = true;
            }
            tokio::time::sleep(SETTINGS_RECHECK_INTERVAL).await;
            continue;
        };
        announced_waiting = false;
        let Ok((host, port)) = rest::host_and_port(&current.server_url) else {
            tracing::error!("サーバー URL を解釈できない: {}", current.server_url);
            tokio::time::sleep(SETTINGS_RECHECK_INTERVAL).await;
            continue;
        };
        let token = match settings::token::get(&dir) {
            Ok(token) => token,
            Err(e) => {
                tracing::warn!("トークンの取得に失敗: {e}。トークン無しで接続を試みる");
                None
            }
        };
        let Some(token) = token else {
            // 自動ペアリング (auto_pair_forever) の完了を待つ。401 で無駄に叩かない
            tokio::time::sleep(SETTINGS_RECHECK_INTERVAL).await;
            continue;
        };

        match connect_once(&host, port, &fingerprint, Some(&token)).await {
            Ok(ws_stream) => {
                tracing::info!("サーバーに接続した ({host}:{port})");
                backoff = std::time::Duration::from_secs(1);
                status.set_connected(&app, true);
                status.set_last_error(&app, None);
                if let Err(e) = ws::drive_connection(ws_stream, &outgoing, &incoming_tx).await {
                    tracing::warn!("WebSocket 接続が切れた: {e}");
                    status.set_last_error(&app, Some(e.to_string()));
                }
                status.set_connected(&app, false);
            }
            Err(e) => {
                tracing::warn!("接続に失敗: {e}。{backoff:?} 後に再試行する");
                status.set_connected(&app, false);
                status.set_last_error(&app, Some(e));
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
    }
}

type PinnedTlsStream = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;

async fn connect_once(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
    token: Option<&str>,
) -> Result<tokio_tungstenite::WebSocketStream<PinnedTlsStream>, String> {
    let tcp = tokio::net::TcpStream::connect((host, port))
        .await
        .map_err(|e| e.to_string())?;
    let verifier = FingerprintVerifier::new(fingerprint_hex).map_err(|e| e.to_string())?;
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let server_name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| e.to_string())?;

    let mut request = format!("wss://{host}:{port}/v1/dictate")
        .into_client_request()
        .map_err(|e| e.to_string())?;
    if let Some(token) = token {
        let value = format!("Bearer {token}")
            .parse()
            .map_err(|e: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| e.to_string())?;
        request.headers_mut().insert("Authorization", value);
    }

    let (ws_stream, _response) = tokio_tungstenite::client_async(request, tls)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ws_stream)
}
