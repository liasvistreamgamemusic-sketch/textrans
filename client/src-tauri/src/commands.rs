//! React (設定画面・辞書エディタ・フィンガープリント承認 UI) から呼ばれる Tauri コマンド。

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;

use crate::protocol::Dictionary;
use crate::rest;
use crate::settings::{self, Settings};
use crate::ws::verifier::normalize_fingerprint;
use crate::ws::PendingQueue;

pub struct AppState {
    pub settings: Arc<Mutex<Settings>>,
    pub outgoing: PendingQueue,
}

fn settings_file_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("設定ディレクトリの取得に失敗: {e}"))?;
    Ok(dir.join("settings.json"))
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
#[tauri::command]
pub async fn approve_fingerprint(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    fingerprint_hex: String,
) -> Result<(), String> {
    let normalized = normalize_fingerprint(&fingerprint_hex).map_err(|e| e.to_string())?;
    let path = settings_file_path(&app)?;
    let mut current = state.settings.lock().await;
    current.server_fingerprint_hex = Some(normalized);
    settings::save(&path, &current).map_err(|e| e.to_string())?;
    Ok(())
}

async fn dictionary_endpoint(state: &tauri::State<'_, AppState>) -> Result<(String, u16, String, String), String> {
    let settings = state.settings.lock().await.clone();
    let fingerprint = settings
        .server_fingerprint_hex
        .clone()
        .ok_or_else(|| "証明書フィンガープリントが未承認".to_string())?;
    let (host, port) = rest::host_and_port(&settings.server_url).map_err(|e| e.to_string())?;
    let token = settings::token::get()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "トークンが未設定".to_string())?;
    Ok((host, port, fingerprint, token))
}

#[tauri::command]
pub async fn get_dictionary(state: tauri::State<'_, AppState>) -> Result<Dictionary, String> {
    let (host, port, fingerprint, token) = dictionary_endpoint(&state).await?;
    rest::get_dictionary(&host, port, &fingerprint, &token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn put_dictionary(state: tauri::State<'_, AppState>, dictionary: Dictionary) -> Result<(), String> {
    let (host, port, fingerprint, token) = dictionary_endpoint(&state).await?;
    rest::put_dictionary(&host, port, &fingerprint, &token, &dictionary)
        .await
        .map_err(|e| e.to_string())
}
