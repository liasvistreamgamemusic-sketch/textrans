//! voice-client (Tauri 2 + Rust) のライブラリ本体。
//!
//! モジュール構成 (design.md §5.1 の技術スタックに対応):
//! - [`protocol`][]: WebSocket メッセージ型・辞書型 (protocol_version = 1)
//! - [`ws`][]: 常時接続・証明書ピン留め・送信バッファ
//! - [`rest`][]: `GET/PUT /v1/dictionary`
//! - [`audio`][]: 録音・リサンプリング・チャンク分割
//! - [`state`][]: 発話の状態機械
//! - [`insert`][]: クリップボード貼り付け・直接送出
//! - [`settings`][]: 設定の永続化・トークンの keyring 保存
//! - [`overlay`][]: 状態表示オーバーレイウィンドウ
//! - [`orchestrator`][]: 上記をつなぐ実行ループ
//! - [`commands`][]: React (設定画面・辞書エディタ) から呼ばれる Tauri コマンド

pub mod audio;
pub mod commands;
pub mod insert;
pub mod orchestrator;
pub mod overlay;
pub mod protocol;
pub mod rest;
pub mod settings;
pub mod state;
pub mod ws;

use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use commands::AppState;
use orchestrator::{Orchestrator, OrchestratorEvent};
use settings::Settings;
use ws::verifier::FingerprintVerifier;
use ws::PendingQueue;

/// 設定ファイルのパス。`app.path().app_config_dir()` 配下。
fn settings_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .expect("設定ディレクトリを解決できない (OS の設定ディレクトリ API が失敗)");
    dir.join("settings.json")
}

/// 既定ホットキー (design.md §5.2「両OSとも Ctrl+Shift+Space」)。
fn default_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
}

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
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            let path = settings_path(&handle);
            let loaded = settings::load(&path).unwrap_or_default();
            {
                let state = app.state::<AppState>();
                let settings_slot = state.settings.clone();
                tauri::async_runtime::block_on(async move {
                    *settings_slot.lock().await = loaded.clone();
                });
            }

            app.global_shortcut().register(default_shortcut())?;

            overlay::ensure_overlay_window(&handle)?;

            // 未ペアリング (トークン未保存) で起動した場合は、常駐アプリとして隠れている
            // 前提のメインウィンドウを自動で前面に出す (実装計画: ペアリングカードに
            // 気づけないまま常時接続が待機し続ける状態を避ける)。keyring アクセス自体が
            // 失敗した場合も安全側 (未ペアリング扱い) にする。
            let has_token = match settings::token::get() {
                Ok(token) => token.is_some(),
                Err(e) => {
                    tracing::warn!("トークンの確認に失敗: {e}。未ペアリングとして扱い、メインウィンドウを表示する");
                    false
                }
            };
            if !has_token {
                if let Some(window) = handle.get_webview_window("main") {
                    if let Err(e) = window.show() {
                        tracing::warn!("メインウィンドウの表示に失敗: {e}");
                    }
                    if let Err(e) = window.set_focus() {
                        tracing::warn!("メインウィンドウのフォーカスに失敗: {e}");
                    }
                }
            }

            let orchestrator_settings = {
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(async { state.settings.lock().await.clone() })
            };
            let orchestrator =
                Orchestrator::new(handle.clone(), orchestrator_settings, outgoing.clone(), events_tx.clone());
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

            // 常時接続 (design.md §3.1)。設定スロットを共有し、承認・変更を再起動なしで拾う。
            let ws_settings = app.state::<AppState>().settings.clone();
            tauri::async_runtime::spawn(run_connection_forever(ws_settings, outgoing, incoming_tx));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri アプリケーションの起動に失敗");
}

/// 承認待ちや設定不備のときに設定を読み直すまでの間隔。
const SETTINGS_RECHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// 常時接続の再接続ループ。設定 (URL・フィンガープリント・トークン) は接続を試みるたびに
/// 共有スロットから読み直すので、設定画面で承認・変更した内容は再起動なしで反映される。
/// フィンガープリント未承認 (`server_fingerprint_hex: None`) の間は接続を試みず待機する
/// (初回接続の承認 UI は React 側、design.md §5.8)。
async fn run_connection_forever(
    settings: Arc<Mutex<Settings>>,
    outgoing: PendingQueue,
    incoming_tx: mpsc::UnboundedSender<protocol::ServerMessage>,
) {
    let mut backoff = std::time::Duration::from_secs(1);
    const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
    let mut announced_waiting = false;

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
        let token = settings::token::get().ok().flatten();
        if token.is_none() {
            tracing::warn!("トークンが未設定。設定画面で保存すると次の接続試行から使われる");
        }

        match connect_once(&host, port, &fingerprint, token.as_deref()).await {
            Ok(ws_stream) => {
                tracing::info!("サーバーに接続した ({host}:{port})");
                backoff = std::time::Duration::from_secs(1);
                if let Err(e) = ws::drive_connection(ws_stream, &outgoing, &incoming_tx).await {
                    tracing::warn!("WebSocket 接続が切れた: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("接続に失敗: {e}。{backoff:?} 後に再試行する");
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
