//! React (設定画面・辞書エディタ・フィンガープリント承認・ペアリング UI) から呼ばれる Tauri コマンド。

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::protocol::Dictionary;
use crate::rest;
use crate::settings::{self, History, HistoryItem, Settings};
use crate::status::{Status, StatusStore};
use crate::ws::verifier::normalize_fingerprint;
use crate::ws::PendingQueue;

pub struct AppState {
    pub settings: Arc<Mutex<Settings>>,
    pub outgoing: PendingQueue,
    pub status: Arc<StatusStore>,
    pub history: Arc<Mutex<History>>,
}

fn settings_dir_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("設定ディレクトリの取得に失敗: {e}"))
}

fn settings_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(settings_dir_path(app)?.join("settings.json"))
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    let path = settings_file_path(&app)?;
    settings::save(&path, &settings).map_err(|e| e.to_string())?;
    state.status.set_server_url(&app, settings.server_url.clone());
    state.status.set_fingerprint(
        &app,
        settings.server_fingerprint_hex.as_deref().map(crate::ws::verifier::to_colon_upper),
    );
    *state.settings.lock().await = settings;
    Ok(())
}

/// 初回接続 (TOFU) 用にサーバー証明書のフィンガープリントを取得する。まだ何も検証しない
/// (design.md §3.1, §5.8)。ユーザーが `voice-server cert fingerprint` の表示と見比べて
/// 一致を確認した後、[`approve_fingerprint`] で確定させる。
#[tauri::command]
pub async fn probe_fingerprint(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let server_url = state.settings.lock().await.server_url.clone();
    let (host, port) = rest::host_and_port(&server_url).map_err(|e| e.to_string())?;
    crate::ws::probe_fingerprint(&host, port)
        .await
        .map_err(|e| e.to_string())
}

/// 初回接続時にユーザーが承認したフィンガープリントを保存する (design.md §3.1, §5.8)。
/// 手動経路 (サーバーが code モードのとき、または証明書だけを個別に承認したいとき) 用に残す。
#[tauri::command]
pub async fn approve_fingerprint(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    fingerprint_hex: String,
) -> Result<(), String> {
    let normalized = normalize_fingerprint(&fingerprint_hex).map_err(|e| e.to_string())?;
    let path = settings_file_path(&app)?;
    let mut current = state.settings.lock().await;
    current.server_fingerprint_hex = Some(normalized.clone());
    settings::save(&path, &current).map_err(|e| e.to_string())?;
    state.status.set_fingerprint(&app, Some(crate::ws::verifier::to_colon_upper(&normalized)));
    Ok(())
}

/// 未ペアリング判定用 (React 起動時、設定画面先頭のペアリングカードの表示条件)。
/// トークンファイルの読み込み自体が失敗した場合もエラーにせず「未ペアリング」として扱う
/// (安全側 = ペアリングカードを出すだけで、既存の接続や辞書アクセスを止めるわけではない)。
#[tauri::command]
pub async fn has_token(app: tauri::AppHandle) -> Result<bool, String> {
    let dir = settings_dir_path(&app)?;
    match settings::token::get(&dir) {
        Ok(token) => Ok(token.is_some()),
        Err(e) => {
            tracing::warn!("トークンの確認に失敗: {e}。未ペアリングとして扱う");
            Ok(false)
        }
    }
}

/// [`pair`] コマンドの成功応答。React 側の表示用にフィンガープリントだけを返す
/// (トークンはファイル (0600) に保存済みで、React 側が保持する必要はない)。
#[derive(Debug, Clone, Serialize)]
pub struct PairOutcome {
    pub fingerprint_hex: String,
}

/// device 名の既定値 (実装計画: 「device_name の既定はホスト名」)。
/// サーバー契約の device は英数字・`-`・`_` のみ、1〜64 文字。それ以外の文字は `-` に
/// 置換し、64 文字を超える分は切り捨てる。空になった場合の既定値も用意する。
fn sanitize_device_name(input: &str) -> String {
    let mut sanitized: String = input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    sanitized.truncate(64);
    if sanitized.is_empty() {
        sanitized = "voice-client".to_string();
    }
    sanitized
}

/// ホスト名から device 名を作る。ホスト名が UTF-8 として解釈できない場合も
/// `to_string_lossy` で落とさずに済ませ、`sanitize_device_name` に委ねる。
pub(crate) fn default_device_name() -> String {
    let hostname = gethostname::gethostname();
    sanitize_device_name(&hostname.to_string_lossy())
}

/// `pair` 応答の `fingerprint` が TOFU (`ws::probe_fingerprint`) で取得した値と一致するかを
/// 検証する。一致すれば正規化済みの値を返す。ここで確定してから初めて設定へ保存するので、
/// 途中で証明書が入れ替わっていた (なりすまし) 場合は保存せずに失敗させられる。
fn verify_pair_fingerprint(probed_fingerprint_hex: &str, response_fingerprint: &str) -> Result<String, String> {
    let probed = normalize_fingerprint(probed_fingerprint_hex).map_err(|e| e.to_string())?;
    let returned = normalize_fingerprint(response_fingerprint).map_err(|e| e.to_string())?;
    if probed != returned {
        return Err(format!(
            "サーバーが返したフィンガープリントが TOFU で取得した値と一致しない (期待: {probed}, 実際: {returned})。\
             なりすましの可能性があるため保存を中止した"
        ));
    }
    Ok(probed)
}

/// [`pair`] コマンド本体のうち、設定・トークンファイルへの書き込みを含まない部分
/// (手順 a: TOFU でフィンガープリントを取得 → b: その TLS で `POST /v1/pair` → c: 応答の
/// フィンガープリントが a と一致することを検証)。トークンファイルへ書き込む手順 d は副作用が大きく
/// (実際の OS キーチェーンを変更してしまう) テストしにくいため、ここでは分離して
/// モック TLS サーバーだけで検証できるようにしている。
///
/// `code` はサーバーがペアリングコードを要求するモードのときだけ渡す。サーバーが
/// コード不要 (`{"device": str}` だけで 200 を返す) 運用になった現在は既定で `None`。
async fn pair_and_verify(
    host: &str,
    port: u16,
    device: &str,
    code: Option<&str>,
) -> Result<rest::PairResponse, String> {
    let probed_fingerprint = crate::ws::probe_fingerprint(host, port)
        .await
        .map_err(|e| e.to_string())?;
    let response = rest::pair(host, port, &probed_fingerprint, device, code)
        .await
        .map_err(|e| e.to_string())?;
    let verified_fingerprint = verify_pair_fingerprint(&probed_fingerprint, &response.fingerprint)?;
    Ok(rest::PairResponse {
        device: response.device,
        token: response.token,
        fingerprint: verified_fingerprint,
    })
}

/// ペアリング本体: (a) TOFU でフィンガープリントを取得 → (b) そのフィンガープリントで固定した
/// TLS で `POST /v1/pair` → (c) 応答のフィンガープリントが (a) と一致することを検証 →
/// (d) token をファイル (0600)、fingerprint と server_url を設定へ保存する。
/// `device_name` を省略した場合はホスト名から既定値を作る。
///
/// [`pair`] (手動/UI 契約コマンド) と、起動時の自動ペアリング (`lib.rs`) の両方から呼ばれる
/// (実装計画: 「起動時に code なしで自動実行する」)。
pub(crate) async fn perform_pair(
    app: &tauri::AppHandle,
    server_url: String,
    code: Option<String>,
    device_name: Option<String>,
) -> Result<PairOutcome, String> {
    let state = app.state::<AppState>();
    let (host, port) = rest::host_and_port(&server_url).map_err(|e| e.to_string())?;
    let device = device_name.unwrap_or_else(default_device_name);

    let response = pair_and_verify(&host, port, &device, code.as_deref()).await?;

    let dir = settings_dir_path(app)?;
    settings::token::set(&dir, &response.token).map_err(|e| e.to_string())?;

    let path = settings_file_path(app)?;
    {
        let mut current = state.settings.lock().await;
        current.server_url = server_url.clone();
        current.server_fingerprint_hex = Some(response.fingerprint.clone());
        settings::save(&path, &current).map_err(|e| e.to_string())?;
    }

    state.status.set_server_url(app, server_url);
    state.status.set_device_name(app, Some(device));
    state.status.set_fingerprint(app, Some(crate::ws::verifier::to_colon_upper(&response.fingerprint)));
    state.status.set_paired(app, true);
    state.status.set_last_error(app, None);

    Ok(PairOutcome {
        fingerprint_hex: response.fingerprint,
    })
}

#[tauri::command]
pub async fn pair(
    app: tauri::AppHandle,
    server_url: String,
    code: Option<String>,
    device_name: Option<String>,
) -> Result<PairOutcome, String> {
    perform_pair(&app, server_url, code, device_name).await
}

/// サーバー変更時などにトークンを捨てて再ペアリングする (UI 契約 `repair`)。
/// フィンガープリントも捨てる (サーバーが変わっているはずなので TOFU をやり直す)。
#[tauri::command]
pub async fn repair(app: tauri::AppHandle, server_url: String) -> Result<Status, String> {
    let state = app.state::<AppState>();
    let dir = settings_dir_path(&app)?;
    if let Err(e) = settings::token::delete(&dir) {
        tracing::warn!("再ペアリング前のトークン削除に失敗: {e}");
    }
    {
        let mut current = state.settings.lock().await;
        current.server_fingerprint_hex = None;
    }
    state.status.set_paired(&app, false);
    state.status.set_fingerprint(&app, None);

    perform_pair(&app, server_url, None, None).await?;
    Ok(state.status.snapshot())
}

/// UI 契約 `get_status`。
#[tauri::command]
pub async fn get_status(state: tauri::State<'_, AppState>) -> Result<Status, String> {
    Ok(state.status.snapshot())
}

/// UI 契約 `get_history`: 直近 [`crate::settings::HISTORY_CAPACITY`] 件。
#[tauri::command]
pub async fn get_history(state: tauri::State<'_, AppState>) -> Result<Vec<HistoryItem>, String> {
    Ok(state.history.lock().await.items())
}

/// UI 契約 `copy_history_item`: 挿入失敗時の再利用向けにクリップボードへコピーする。
#[tauri::command]
pub async fn copy_history_item(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let text = {
        let history = state.history.lock().await;
        history.get(&id).map(|item| item.text.clone())
    };
    let text = text.ok_or_else(|| format!("履歴に id={id} の項目が見つからない"))?;
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())?;
    Ok(())
}

/// UI 契約 `open_accessibility_settings`。
#[tauri::command]
pub async fn open_accessibility_settings() -> Result<(), String> {
    crate::accessibility::open_settings()
}

async fn dictionary_endpoint(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
) -> Result<(String, u16, String, String), String> {
    let settings = state.settings.lock().await.clone();
    let fingerprint = settings
        .server_fingerprint_hex
        .clone()
        .ok_or_else(|| "証明書フィンガープリントが未承認".to_string())?;
    let (host, port) = rest::host_and_port(&settings.server_url).map_err(|e| e.to_string())?;
    let dir = settings_dir_path(app)?;
    let token = settings::token::get(&dir)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "トークンが未設定".to_string())?;
    Ok((host, port, fingerprint, token))
}

#[tauri::command]
pub async fn get_dictionary(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<Dictionary, String> {
    let (host, port, fingerprint, token) = dictionary_endpoint(&app, &state).await?;
    rest::get_dictionary(&host, port, &fingerprint, &token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn put_dictionary(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    dictionary: Dictionary,
) -> Result<(), String> {
    let (host, port, fingerprint, token) = dictionary_endpoint(&app, &state).await?;
    rest::put_dictionary(&host, port, &fingerprint, &token, &dictionary)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_device_name_replaces_disallowed_characters() {
        assert_eq!(sanitize_device_name("My Mac.local"), "My-Mac-local");
    }

    #[test]
    fn sanitize_device_name_keeps_allowed_characters() {
        assert_eq!(sanitize_device_name("nitanda-macbook_pro"), "nitanda-macbook_pro");
    }

    #[test]
    fn sanitize_device_name_truncates_to_64_chars() {
        let long = "a".repeat(100);
        let sanitized = sanitize_device_name(&long);
        assert_eq!(sanitized.chars().count(), 64);
        assert!(sanitized.chars().all(|c| c == 'a'));
    }

    #[test]
    fn sanitize_device_name_falls_back_when_input_is_empty() {
        assert_eq!(sanitize_device_name(""), "voice-client");
    }

    #[test]
    fn verify_pair_fingerprint_accepts_matching_values_in_different_forms() {
        // TOFU 側は生の16進小文字、サーバー応答側はコロン区切り大文字 (実際のサーバー契約通り)。
        let hex = "8c4d170000000000000000000000000000000000000000000000000000000088";
        let colon_upper = "8C:4D:17:00:00:00:00:00:00:00:00:00:00:00:00:00:\
                            00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:88";
        let result = verify_pair_fingerprint(hex, colon_upper).expect("一致するはず");
        assert_eq!(result, hex);
    }

    #[test]
    fn verify_pair_fingerprint_rejects_mismatched_values() {
        let probed = "00".repeat(32);
        let returned = "11".repeat(32);
        let err = verify_pair_fingerprint(&probed, &returned).unwrap_err();
        assert!(err.contains("一致しない"));
    }

    #[test]
    fn verify_pair_fingerprint_rejects_malformed_response_fingerprint() {
        let probed = "00".repeat(32);
        let err = verify_pair_fingerprint(&probed, "not-hex").unwrap_err();
        assert!(!err.is_empty());
    }

    /// `pair` コマンド本体のうち settings/トークンファイルへの書き込みを含まない部分
    /// (a: TOFU 取得 → b: `POST /v1/pair` → c: フィンガープリント一致検証) が、
    /// ローカルの TLS モックサーバー越しに一連の流れとして通ることを確認する結合テスト。
    /// サーバーが code 不要 (`code: None`) の運用になったことを前提に検証する。
    #[tokio::test]
    async fn pair_and_verify_succeeds_end_to_end_over_pinned_tls() {
        use rcgen::{generate_simple_self_signed, CertifiedKey};
        use rustls::pki_types::PrivatePkcs8KeyDer;
        use rustls::ServerConfig;
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio_rustls::TlsAcceptor;

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("自己署名証明書の生成");
        let cert_der = cert.der().clone();
        let fingerprint_lower = crate::ws::verifier::fingerprint_hex(&cert_der);
        let key_der = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("サーバー TLS 設定");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        // 応答の fingerprint はサーバー CLI 表示形式 (コロン区切り・大文字) で返す —
        // 実際の証明書と同じ値であることを一致検証で確認する。
        let response_fingerprint_colon_upper: String = fingerprint_lower
            .as_bytes()
            .chunks(2)
            .map(|pair| std::str::from_utf8(pair).unwrap().to_uppercase())
            .collect::<Vec<_>>()
            .join(":");

        let server_task = tokio::spawn(async move {
            // 1本目: probe_fingerprint (TOFU、TLS を張って即切断されるだけ)。
            let (tcp, _) = listener.accept().await.expect("accept (probe)");
            let tls = acceptor.accept(tcp).await.expect("tls accept (probe)");
            drop(tls);

            // 2本目: POST /v1/pair。
            let (tcp, _) = listener.accept().await.expect("accept (pair)");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept (pair)");
            let mut raw = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let n = tls.read(&mut chunk).await.expect("読み取り");
                raw.extend_from_slice(&chunk[..n]);
                if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&raw).to_string();
            assert!(request_text.starts_with("POST /v1/pair HTTP/1.1"));
            assert!(
                !request_text.contains("\"code\""),
                "code 不要のはずなのに code フィールドが送られている: {request_text}"
            );

            let body = format!(
                r#"{{"device":"test-device","token":"tok-xyz","fingerprint":"{response_fingerprint_colon_upper}"}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            tls.write_all(response.as_bytes()).await.expect("応答ヘッダー送信");
            tls.write_all(body.as_bytes()).await.expect("応答ボディ送信");
            tls.shutdown().await.expect("TLS シャットダウン");
        });

        let host = addr.ip().to_string();
        let result = pair_and_verify(&host, addr.port(), "test-device", None).await;

        let response = result.expect("pair_and_verify が成功すること");
        assert_eq!(response.device, "test-device");
        assert_eq!(response.token, "tok-xyz");
        assert_eq!(response.fingerprint, fingerprint_lower);

        server_task.await.expect("server task panicked");
    }

    /// 応答のフィンガープリントが TOFU で取得した値と異なる場合は保存せずに失敗すること
    /// (なりすまし対策)。
    #[tokio::test]
    async fn pair_and_verify_fails_when_response_fingerprint_does_not_match_tofu() {
        use rcgen::{generate_simple_self_signed, CertifiedKey};
        use rustls::pki_types::PrivatePkcs8KeyDer;
        use rustls::ServerConfig;
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio_rustls::TlsAcceptor;

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("自己署名証明書の生成");
        let cert_der = cert.der().clone();
        let key_der = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("サーバー TLS 設定");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept (probe)");
            let tls = acceptor.accept(tcp).await.expect("tls accept (probe)");
            drop(tls);

            let (tcp, _) = listener.accept().await.expect("accept (pair)");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept (pair)");
            let mut chunk = [0u8; 4096];
            let mut raw = Vec::new();
            loop {
                let n = tls.read(&mut chunk).await.expect("読み取り");
                raw.extend_from_slice(&chunk[..n]);
                if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }

            // 実際の証明書とは無関係な (=不一致になるはずの) fingerprint をわざと返す。
            let body = r#"{"device":"test-device","token":"tok-xyz","fingerprint":"00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            tls.write_all(response.as_bytes()).await.expect("応答ヘッダー送信");
            tls.write_all(body.as_bytes()).await.expect("応答ボディ送信");
            tls.shutdown().await.expect("TLS シャットダウン");
        });

        let host = addr.ip().to_string();
        let result = pair_and_verify(&host, addr.port(), "test-device", None).await;
        let err = result.expect_err("フィンガープリント不一致で失敗するはず");
        assert!(err.contains("一致しない"), "予期しないエラー内容: {err}");

        server_task.await.expect("server task panicked");
    }
}
