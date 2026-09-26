//! macOS アクセシビリティ権限 (`AXIsProcessTrustedWithOptions`) の確認 (design.md 8.2 の
//! 「アクセシビリティ権限への誘導 UI は未実装」への対応)。
//!
//! キー送信 (enigo)・前面アプリ取得 (insert モジュール) はこの権限が無いと機能しない。
//! `objc2-application-services` (既存の objc2 系依存 0.6.4/0.3.2 と同ファミリー、crates.io で
//! 実在バージョンを確認済み) を使う。他 OS ではこの概念自体が無いため常に許可扱いにする。

/// 現在のプロセスがアクセシビリティ権限を許可されているかを確認する。
///
/// `prompt` が `true` の場合、未許可であれば OS がシステム設定への誘導ダイアログを
/// 非同期に表示する (`kAXTrustedCheckOptionPrompt`)。ダイアログの表示自体は非同期なので、
/// この呼び出しの戻り値 (現在の許可状態) には影響しない (Apple のドキュメント通り)。
#[cfg(target_os = "macos")]
pub fn is_trusted(prompt: bool) -> bool {
    use objc2_application_services::{kAXTrustedCheckOptionPrompt, AXIsProcessTrustedWithOptions};
    use objc2_core_foundation::{kCFBooleanTrue, CFBoolean, CFDictionary, CFString};

    if !prompt {
        // Safety: 引数無し・プロセスの権限状態を読むだけの副作用のない呼び出し。
        return unsafe { objc2_application_services::AXIsProcessTrusted() };
    }

    // Safety: `kAXTrustedCheckOptionPrompt` / `kCFBooleanTrue` はいずれも
    // ApplicationServices/CoreFoundation フレームワークが定義する不変の extern static。
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let Some(value): Option<&CFBoolean> = (unsafe { kCFBooleanTrue }) else {
        tracing::warn!("kCFBooleanTrue を取得できなかった。プロンプト無しの確認にフォールバックする");
        return unsafe { objc2_application_services::AXIsProcessTrusted() };
    };
    let options = CFDictionary::from_slices(&[key], &[value]);
    // Safety: `options` は正しい型 (`CFString` キー・`CFBoolean` 値) の CFDictionary。
    // `AXIsProcessTrustedWithOptions` はキー・値の型を問わない `CFDictionary` (opaque) を取る。
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

#[cfg(not(target_os = "macos"))]
pub fn is_trusted(_prompt: bool) -> bool {
    true
}

/// OS のアクセシビリティ設定画面を開く (UI 契約 `open_accessibility_settings`)。
/// macOS: `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`。
/// 対応しない OS では何もしない (呼び出し元でエラー表示はしない = 誘導ボタン自体を出さない想定)。
#[cfg(target_os = "macos")]
pub fn open_settings() -> Result<(), String> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("システム設定を開けなかった: {e}"))
}

#[cfg(not(target_os = "macos"))]
pub fn open_settings() -> Result<(), String> {
    Ok(())
}
