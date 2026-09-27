//! メニューバー (macOS) / タスクトレイ (Windows) の常駐アイコン (実機フィードバック2)。
//!
//! [`status`](crate::status) と同様、`TrayIcon`/`MenuItem` 自体は状態を持たず、
//! `voice://status` を購読して状態行の文言とアイコン (録音中は赤い点付き) を更新するだけ。
//! ウィンドウの表示/フォーカスは [`show_main_window`] に集約し、トレイのメニュー・クリックと
//! [`crate::run`] 側の `RunEvent::Reopen` (macOS の Dock 再クリック) の両方から呼ぶ。

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};

use crate::status::{Phase, Status};

const OPEN_ITEM_ID: &str = "voice-tray-open";
const STATUS_ITEM_ID: &str = "voice-tray-status";
const QUIT_ITEM_ID: &str = "voice-tray-quit";

// アイコンは `pnpm tauri icon` (librsvg 経由) で `icons/tray/*.svg` から生成した PNG を
// ビルド時に埋め込む (バンドルのリソース解決に依存しないほうが確実、かつファイル数が少ないので
// サイズの心配もない)。macOS はテンプレート画像 (黒+アルファのみ、OS がメニューバーの配色に
// 合わせて塗り直す) を使い、録音中の赤い点は非テンプレート画像に切り替えて表示する。
// Windows/Linux は常にカラー PNG。
#[cfg(target_os = "macos")]
const IDLE_ICON_MAC: &[u8] = include_bytes!("../icons/tray/idle-template.png");
#[cfg(target_os = "macos")]
const RECORDING_ICON_MAC: &[u8] = include_bytes!("../icons/tray/recording-mac.png");
#[cfg(not(target_os = "macos"))]
const IDLE_ICON_OTHER: &[u8] = include_bytes!("../icons/tray/idle-color.png");
#[cfg(not(target_os = "macos"))]
const RECORDING_ICON_OTHER: &[u8] = include_bytes!("../icons/tray/recording-color.png");

/// (アイコンの PNG バイト列, macOS でテンプレート画像として扱うか)。
fn icon_bytes_for(recording: bool) -> (&'static [u8], bool) {
    #[cfg(target_os = "macos")]
    {
        if recording {
            (RECORDING_ICON_MAC, false)
        } else {
            (IDLE_ICON_MAC, true)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if recording {
            (RECORDING_ICON_OTHER, false)
        } else {
            (IDLE_ICON_OTHER, false)
        }
    }
}

/// トレイメニューの無効項目 (状態行) に表示する文言。
fn status_line(status: &Status) -> &'static str {
    if !status.connected {
        return "切断";
    }
    match status.phase {
        Phase::Idle => "接続中 · 待機中",
        Phase::Recording => "録音中",
        Phase::Waiting => "接続中 · 処理中",
        Phase::Inserting => "接続中 · 挿入中",
    }
}

/// メインウィンドウを表示・前面化する。macOS は Dock アイコンを一時的に戻す
/// (`ActivationPolicy::Accessory` の間は Cmd+Tab で戻れないため)。
pub fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!("メインウィンドウが見つからない (open_main_window)");
        return;
    };
    if let Err(e) = window.show() {
        tracing::warn!("メインウィンドウの表示に失敗: {e}");
    }
    if let Err(e) = window.set_focus() {
        tracing::warn!("メインウィンドウのフォーカスに失敗: {e}");
    }
    #[cfg(target_os = "macos")]
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
        tracing::warn!("Dock アイコンの表示切り替えに失敗: {e}");
    }
}

/// トレイアイコンを作る。戻り値の `TrayIcon` は呼び出し側 (`lib.rs`) が `app.manage()` で
/// アプリの生存期間ずっと保持する (参照カウント式で最後の1つが drop されるとアイコンも消えるため)。
/// 以後の更新は `voice://status` の購読だけで自律的に行う (呼び出し側は保持するだけでよい)。
pub fn build_tray(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let open_item = MenuItem::with_id(app, OPEN_ITEM_ID, "voice-client を開く", true, None::<&str>)?;
    let status_item = MenuItem::with_id(app, STATUS_ITEM_ID, "切断", false, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, QUIT_ITEM_ID, "終了", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_item,
            &PredefinedMenuItem::separator(app)?,
            &status_item,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;

    let (idle_bytes, idle_is_template) = icon_bytes_for(false);
    let icon = Image::from_bytes(idle_bytes)?;

    let tray = TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(idle_is_template)
        .menu(&menu)
        // 既定 (true) のまま: mac はクリックでメニューを開く唯一の手段なので必須。
        // Windows は右クリックでメニュー、左クリックは下の on_tray_icon_event で
        // ウィンドウを開く動作を別途追加する (メニューも同時に開くが実害はない)。
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == OPEN_ITEM_ID {
                show_main_window(app);
            } else if id == QUIT_ITEM_ID {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Windows 以外では `tray` を使わない (下記 `#[cfg(windows)]` 参照)。
            let _ = &tray;
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // design.md 実機フィードバック: 「左クリックでもウィンドウを開く (Windows)」。
                // macOS は上の show_menu_on_left_click で十分なので、二重にウィンドウを
                // 出さないよう Windows 限定にする。
                #[cfg(windows)]
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    let tray_for_status = tray.clone();
    app.listen(crate::status::STATUS_EVENT, move |event| {
        let status = match serde_json::from_str::<Status>(event.payload()) {
            Ok(status) => status,
            Err(e) => {
                tracing::warn!("トレイ更新用に {} の payload 解析に失敗: {e}", crate::status::STATUS_EVENT);
                return;
            }
        };

        if let Err(e) = status_item.set_text(status_line(&status)) {
            tracing::warn!("トレイの状態行更新に失敗: {e}");
        }

        let (icon_bytes, is_template) = icon_bytes_for(status.phase == Phase::Recording);
        match Image::from_bytes(icon_bytes) {
            Ok(icon) => {
                if let Err(e) = tray_for_status.set_icon_with_as_template(Some(icon), is_template) {
                    tracing::warn!("トレイアイコンの更新に失敗: {e}");
                }
            }
            Err(e) => tracing::warn!("トレイアイコンの読み込みに失敗: {e}"),
        }
    });

    Ok(tray)
}
