//! フォーカスを奪わない小型オーバーレイウィンドウ (design.md §5.1、録音中/処理中/失敗)。
//!
//! Tauri のウィンドウ機能に依存するため、実ウィンドウ操作 (作成・表示・イベント送信) は
//! 単体テストの対象外。状態の enum と (テスト可能な) 表示文言のマッピングだけを分離する。

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const OVERLAY_LABEL: &str = "voice-overlay";
const OVERLAY_WIDTH: f64 = 220.0;
const OVERLAY_HEIGHT: f64 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayStatus {
    Recording,
    Processing,
    Failed,
}

impl OverlayStatus {
    /// オーバーレイ (`overlay.html`) に表示する短い日本語ラベル。
    pub fn label(&self) -> &'static str {
        match self {
            OverlayStatus::Recording => "録音中",
            OverlayStatus::Processing => "処理中",
            OverlayStatus::Failed => "失敗",
        }
    }
}

/// オーバーレイウィンドウを (無ければ) 作る。フォーカスを奪わないよう常に非フォーカス・
/// 最前面・タスクバー非表示にする。
pub fn ensure_overlay_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("overlay.html".into()))
        .title("")
        .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        // クリックしてもフォーカスを奪わない (実機フィードバック3: `.focused(false)` は
        // 作成時点の初期フォーカス状態だけなので、これも合わせて無効にする)。
        .focusable(false)
        .visible(false)
        .shadow(false)
        .transparent(true)
        // WebView2 (Windows) は既定で白背景を描くため、透明を明示する
        // (実機フィードバック3。macOS は `transparent(true)` だけで十分だが害はない)。
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .build()?;
    Ok(())
}

/// 状態を表示する。オーバーレイ未作成なら何もしない (呼び出し側が先に `ensure_overlay_window` する)。
pub fn show_status(app: &AppHandle, status: OverlayStatus) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        window.emit("voice-overlay://status", status)?;
        window.show()?;
    }
    Ok(())
}

/// オーバーレイを隠す (Idle 状態)。
pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        window.hide()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_distinct_and_non_empty() {
        let labels = [
            OverlayStatus::Recording.label(),
            OverlayStatus::Processing.label(),
            OverlayStatus::Failed.label(),
        ];
        assert!(labels.iter().all(|l| !l.is_empty()));
        assert_ne!(labels[0], labels[1]);
        assert_ne!(labels[1], labels[2]);
    }
}
