//! 入力欄への挿入 (design.md §5.5)。
//!
//! 既定はクリップボード経由の貼り付け。純粋な判定ロジック (前面アプリ一致・クリップボード復元判定)
//! はこのファイルの上部にまとめてテストする。OS 依存の実 I/O (前面アプリ取得・クリップボード操作・
//! キー送信) は下部の非同期関数で行い、cfg で macOS/Windows を分ける。
//!
//! macOS 優先: 前面アプリの一致確認はバンドル ID (`NSRunningApplication.bundleIdentifier`) を
//! 使う。design.md は「アプリとウィンドウID」を記録するとしているが、ウィンドウ単位の識別は
//! 標準 API では取得コストが高いため、初版はアプリ単位の比較にとどめる (README に明記)。

use std::collections::HashMap;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// クリップボード履歴に残らない指定 (macOS の慣習。design.md §5.5 の注記どおり保証はない)。
pub const NSPASTEBOARD_CONCEALED_TYPE: &str = "org.nspasteboard.ConcealedType";

/// 待機時間の既定値 (design.md §5.5)。
pub const DEFAULT_PASTE_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug, thiserror::Error)]
pub enum InsertError {
    #[error("クリップボード操作に失敗: {0}")]
    Clipboard(String),
    #[error("キー送信に失敗: {0}")]
    Keyboard(String),
    #[error("前面アプリの取得に失敗: {0}")]
    FrontApp(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    /// 前面アプリが押下時と異なるため挿入を見送った。通知から「コピー」できるようにする (§5.5-2)。
    SkippedFrontAppChanged,
}

// ---- 純粋な判定ロジック (テスト対象) ----

/// 押下時に記録した前面アプリと、`final` 受信時の前面アプリが同じか。
/// 違えば挿入しない (design.md §5.5 手順2)。
pub fn front_app_unchanged(recorded: &FrontApp, current: &FrontApp) -> bool {
    recorded == current
}

/// クリップボードを元に戻すべきか。
/// 「セット直後の変更カウンタ」と「待機後の変更カウンタ」が同じなら、
/// その間にユーザーが新たにコピーしていないので復元してよい (design.md §5.5 手順6)。
pub fn should_restore_clipboard(changecount_after_set: isize, changecount_after_wait: isize) -> bool {
    changecount_after_set == changecount_after_wait
}

/// アプリ別の待機時間上書き (design.md §5.5)。無ければ既定値。
pub fn paste_wait_for_app(app_id: &str, overrides: &HashMap<String, u32>) -> Duration {
    match overrides.get(app_id) {
        Some(ms) => Duration::from_millis(*ms as u64),
        None => DEFAULT_PASTE_WAIT,
    }
}

// ---- OS 依存の実 I/O ----

/// クリップボード貼り付けによる挿入。
///
/// 1. 前面アプリが押下時と同じか確認する。異なれば挿入せず `SkippedFrontAppChanged` を返す。
/// 2. 現在のクリップボード内容と変更カウンタを退避する。
/// 3. 結果テキストを (履歴に残らない指定つきで) セットする。
/// 4. Cmd+V / Ctrl+V を送る。
/// 5. `wait` の後、変更カウンタが変わっていなければ元の内容に戻す。
pub async fn insert_via_clipboard(
    text: &str,
    recorded_app: &FrontApp,
    wait: Duration,
) -> Result<InsertOutcome, InsertError> {
    let current_app = current_front_app()?;
    if !front_app_unchanged(recorded_app, &current_app) {
        return Ok(InsertOutcome::SkippedFrontAppChanged);
    }

    let mut clipboard = Clipboard::new().map_err(|e| InsertError::Clipboard(e.to_string()))?;
    let previous_text = clipboard.get_text().ok();

    set_clipboard_concealed(&mut clipboard, text)?;
    let changecount_after_set = clipboard_change_count()?;

    send_paste_shortcut()?;

    tokio::time::sleep(wait).await;

    let changecount_after_wait = clipboard_change_count()?;
    if should_restore_clipboard(changecount_after_set, changecount_after_wait) {
        // 復元に失敗しても発話の挿入自体は成功しているので致命的ではないが、
        // 起きたことは必ずログに残す (code-quality: エラーの黒握りつぶし禁止)。
        let restore_result = match previous_text {
            Some(previous) => clipboard.set_text(previous),
            None => clipboard.clear(),
        };
        if let Err(e) = restore_result {
            tracing::warn!("クリップボードの復元に失敗: {e}");
        }
    } else {
        tracing::debug!("ユーザーが貼り付け後に新たにコピーしたため、クリップボードは復元しない");
    }

    Ok(InsertOutcome::Inserted)
}

/// 直接キー送出による挿入 (貼り付け禁止の欄向け、design.md §5.5)。
pub async fn insert_via_direct_type(
    text: &str,
    recorded_app: &FrontApp,
) -> Result<InsertOutcome, InsertError> {
    let current_app = current_front_app()?;
    if !front_app_unchanged(recorded_app, &current_app) {
        return Ok(InsertOutcome::SkippedFrontAppChanged);
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| InsertError::Keyboard(e.to_string()))?;
    enigo
        .text(text)
        .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    Ok(InsertOutcome::Inserted)
}

/// 挿入直後に、挿入した範囲を選択状態にする (design.md §5.6、既定オフの設定がオンのときのみ)。
/// 貼り付け完了後に呼ぶ想定 (クリップボード貼り付け経由の挿入のみ対応)。
///
/// `char_count` は UTF-16 コード単位ではなく書記素/文字数 (呼び出し側で `text.chars().count()`
/// を渡す) —— 改行を含む発話でも Shift+← は行をまたいで戻るので、そのまま戻る回数として使える。
/// ⚠️ サロゲートペア (絵文字等) を含む文字は mac/Windows とも Shift+← 1回で戻るはずだが、
/// 実機での確認はしていない (README に明記)。
pub fn select_inserted_text(char_count: usize) -> Result<(), InsertError> {
    if char_count == 0 {
        return Ok(());
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| InsertError::Keyboard(e.to_string()))?;
    enigo
        .key(Key::Shift, Direction::Press)
        .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    for _ in 0..char_count {
        enigo
            .key(Key::LeftArrow, Direction::Click)
            .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    }
    enigo
        .key(Key::Shift, Direction::Release)
        .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    Ok(())
}

fn send_paste_shortcut() -> Result<(), InsertError> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| InsertError::Keyboard(e.to_string()))?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    enigo
        .key(modifier, Direction::Release)
        .map_err(|e| InsertError::Keyboard(e.to_string()))?;
    Ok(())
}

/// macOS: 通常の文字列タイプに加え `org.nspasteboard.ConcealedType` (空データ) も
/// 一緒に宣言する。クリップボード履歴ツールの一部はこの慣習を見て履歴に残さない
/// (design.md §5.5: 慣習であり全ツールが従う保証はない)。
#[cfg(target_os = "macos")]
fn set_clipboard_concealed(_clipboard: &mut Clipboard, text: &str) -> Result<(), InsertError> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSArray, NSData, NSString};

    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();

    let concealed_type = NSString::from_str(NSPASTEBOARD_CONCEALED_TYPE);
    // Safety: `NSPasteboardTypeString` は AppKit フレームワークが定義する不変の extern static。
    let string_type = unsafe { NSPasteboardTypeString };
    let types = NSArray::from_slice(&[string_type, &concealed_type]);
    // Safety: owner を渡さない (None) ので、呼び出し元がその型であることを保証する必要はない。
    unsafe {
        pasteboard.declareTypes_owner(&types, None);
    }

    let ns_text = NSString::from_str(text);
    if !pasteboard.setString_forType(&ns_text, string_type) {
        return Err(InsertError::Clipboard(
            "NSPasteboard へのテキスト設定に失敗".to_string(),
        ));
    }
    let empty = NSData::new();
    pasteboard.setData_forType(Some(&empty), &concealed_type);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_clipboard_concealed(clipboard: &mut Clipboard, text: &str) -> Result<(), InsertError> {
    clipboard
        .set_text(text)
        .map_err(|e| InsertError::Clipboard(e.to_string()))
}

#[cfg(target_os = "macos")]
fn clipboard_change_count() -> Result<isize, InsertError> {
    use objc2_app_kit::NSPasteboard;
    let pasteboard = NSPasteboard::generalPasteboard();
    Ok(pasteboard.changeCount())
}

/// Windows: `GetClipboardSequenceNumber` (design.md §5.5)。呼ぶたびにクリップボードの
/// 内容が変わっているかを検出できる単調増加カウンタ。
/// ⚠️ この関数を含む `cfg(target_os = "windows")` 配下は macOS 上のこのリポジトリでは
/// コンパイル・実行の確認ができていない (README に明記)。`windows` crate 0.61 系に
/// `Win32::System::DataExchange::GetClipboardSequenceNumber` が存在することは
/// crates.io 上の feature 一覧 (`Win32_System_DataExchange`) で確認済みだが、
/// 実際のシグネチャ・実機での動作は未検証。
#[cfg(target_os = "windows")]
fn clipboard_change_count() -> Result<isize, InsertError> {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    // Safety: 引数を取らず、グローバルなクリップボードの変更連番を読むだけの副作用のない呼び出し。
    let sequence = unsafe { GetClipboardSequenceNumber() };
    Ok(sequence as isize)
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn clipboard_change_count() -> Result<isize, InsertError> {
    Err(InsertError::Clipboard(
        "この OS ではクリップボード変更カウンタの取得は未対応".to_string(),
    ))
}

/// 現在の前面アプリを取得する (押下時のスナップショット用)。
pub fn snapshot_front_app() -> Result<FrontApp, InsertError> {
    current_front_app()
}

#[cfg(target_os = "macos")]
fn current_front_app() -> Result<FrontApp, InsertError> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost = workspace
        .frontmostApplication()
        .ok_or_else(|| InsertError::FrontApp("frontmostApplication が無い".to_string()))?;
    let app_id = frontmost
        .bundleIdentifier()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Ok(FrontApp { app_id })
}

#[cfg(target_os = "windows")]
fn current_front_app() -> Result<FrontApp, InsertError> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // 初版最小実装: ウィンドウハンドルの数値を識別子として使う (プロセス名までは解決しない)。
    // design.md 5.7 の「管理者権限のウィンドウへは送信できない」は insert 実行時のエラーとして表れる。
    let hwnd = unsafe { GetForegroundWindow() };
    Ok(FrontApp {
        app_id: format!("{:?}", hwnd.0),
    })
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn current_front_app() -> Result<FrontApp, InsertError> {
    Err(InsertError::FrontApp("この OS では未対応".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_app_unchanged_true_when_equal() {
        let a = FrontApp {
            app_id: "com.apple.Terminal".to_string(),
        };
        let b = a.clone();
        assert!(front_app_unchanged(&a, &b));
    }

    #[test]
    fn front_app_unchanged_false_when_different() {
        let a = FrontApp {
            app_id: "com.apple.Terminal".to_string(),
        };
        let b = FrontApp {
            app_id: "com.apple.Safari".to_string(),
        };
        assert!(!front_app_unchanged(&a, &b));
    }

    #[test]
    fn restore_when_changecount_untouched() {
        assert!(should_restore_clipboard(5, 5));
    }

    #[test]
    fn do_not_restore_when_user_copied_something_new() {
        assert!(!should_restore_clipboard(5, 6));
    }

    #[test]
    fn paste_wait_uses_override_when_present() {
        let mut overrides = HashMap::new();
        overrides.insert("com.slow.app".to_string(), 800u32);
        assert_eq!(
            paste_wait_for_app("com.slow.app", &overrides),
            Duration::from_millis(800)
        );
    }

    #[test]
    fn paste_wait_uses_default_when_absent() {
        let overrides = HashMap::new();
        assert_eq!(paste_wait_for_app("com.other.app", &overrides), DEFAULT_PASTE_WAIT);
    }
}
