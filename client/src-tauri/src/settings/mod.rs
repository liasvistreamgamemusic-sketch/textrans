//! 設定項目 (design.md §5.8)。JSON でファイルに保存し、トークンだけは keyring に保存する。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::protocol::Mode;

pub const DEFAULT_SERVER_URL: &str = "wss://192.168.11.10:8765";
pub const DEFAULT_HOTKEY: &str = "Ctrl+Shift+Space";
pub const HISTORY_CAPACITY: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationMode {
    /// 押している間だけ録音 (既定)。
    PushToTalk,
    /// 1回目で開始、2回目で終了。
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertMethod {
    /// クリップボード貼り付け (既定)。
    ClipboardPaste,
    /// 文字の直接送出。
    DirectType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server_url: String,
    /// 初回接続時にユーザーが承認した SHA-256 フィンガープリント (16進)。承認前は `None`。
    pub server_fingerprint_hex: Option<String>,
    pub hotkey: String,
    pub operation_mode: OperationMode,
    pub output_mode: Mode,
    pub insert_method: InsertMethod,
    /// `None` は OS既定のマイクを使う。
    pub mic_device: Option<String>,
    pub always_open_mic: bool,
    /// アプリ (`bundle id` 等) ごとの貼り付け後待機時間 (ms) の上書き。design.md §5.5。
    pub paste_wait_overrides: HashMap<String, u32>,
    /// 挿入後に挿入範囲を選択状態にするか。design.md §5.6 (既定オフ)。
    pub select_after_insert: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER_URL.to_string(),
            server_fingerprint_hex: None,
            hotkey: DEFAULT_HOTKEY.to_string(),
            operation_mode: OperationMode::PushToTalk,
            output_mode: Mode::Clean,
            insert_method: InsertMethod::ClipboardPaste,
            mic_device: None,
            always_open_mic: false,
            paste_wait_overrides: HashMap::new(),
            select_after_insert: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("設定ファイルの読み込みに失敗: {0}")]
    Read(#[source] std::io::Error),
    #[error("設定ファイルの書き込みに失敗: {0}")]
    Write(#[source] std::io::Error),
    #[error("設定ファイルの JSON 解析に失敗: {0}")]
    Parse(#[from] serde_json::Error),
}

/// 設定ファイルを読む。存在しなければ既定値を返す (初回起動時のエラーにしない)。
pub fn load(path: &Path) -> Result<Settings, SettingsError> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(serde_json::from_str(&raw)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(SettingsError::Read(e)),
    }
}

/// 設定ファイルへ保存する。親ディレクトリが無ければ作る。
pub fn save(path: &Path, settings: &Settings) -> Result<(), SettingsError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(SettingsError::Write)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, json).map_err(SettingsError::Write)
}

/// トークン (secrets) は keyring 経由で OS のキーチェーン/資格情報マネージャーに保存する
/// (design.md §5.1)。設定 JSON には絶対に書かない。
pub mod token {
    use keyring::Entry;

    const SERVICE: &str = "textrans-voice-client";
    const ACCOUNT: &str = "voice-server-token";

    #[derive(Debug, thiserror::Error)]
    pub enum TokenError {
        #[error("キーチェーン/資格情報マネージャーへのアクセスに失敗: {0}")]
        Keyring(#[from] keyring::Error),
    }

    fn entry() -> Result<Entry, TokenError> {
        Ok(Entry::new(SERVICE, ACCOUNT)?)
    }

    pub fn get() -> Result<Option<String>, TokenError> {
        match entry()?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set(token: &str) -> Result<(), TokenError> {
        entry()?.set_password(token)?;
        Ok(())
    }

    pub fn delete() -> Result<(), TokenError> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// 直近の挿入結果を再貼り付け用にメモリ上だけで保持する履歴 (design.md §5.8、終了時に消える)。
#[derive(Debug, Default)]
pub struct History {
    entries: std::collections::VecDeque<String>,
}

impl History {
    pub fn push(&mut self, text: String) {
        self.entries.push_back(text);
        while self.entries.len() > HISTORY_CAPACITY {
            self.entries.pop_front();
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &String> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile_shim::TempDir;

    #[test]
    fn defaults_match_design_doc() {
        let s = Settings::default();
        assert_eq!(s.server_url, "wss://192.168.11.10:8765");
        assert_eq!(s.hotkey, "Ctrl+Shift+Space");
        assert_eq!(s.operation_mode, OperationMode::PushToTalk);
        assert_eq!(s.output_mode, Mode::Clean);
        assert_eq!(s.insert_method, InsertMethod::ClipboardPaste);
        assert!(!s.always_open_mic);
        assert!(!s.select_after_insert);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = TempDir::new();
        let path = dir.path().join("does-not-exist.json");
        let loaded = load(&path).expect("欠落ファイルはエラーにしない");
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new();
        let path = dir.path().join("nested").join("settings.json");
        let mut paste_wait_overrides = HashMap::new();
        paste_wait_overrides.insert("com.slack.app".to_string(), 500);
        let settings = Settings {
            server_url: "wss://10.0.0.5:8765".to_string(),
            paste_wait_overrides,
            ..Settings::default()
        };

        save(&path, &settings).expect("保存できること");
        let loaded = load(&path).expect("読み込めること");
        assert_eq!(loaded, settings);
    }

    #[test]
    fn history_keeps_only_last_n_entries() {
        let mut history = History::default();
        for i in 0..(HISTORY_CAPACITY + 5) {
            history.push(format!("entry-{i}"));
        }
        assert_eq!(history.len(), HISTORY_CAPACITY);
        let first = history.entries().next().unwrap();
        assert_eq!(first, "entry-5");
    }

    /// `tempfile` crate 相当を新たに依存追加せず、テスト専用の最小 TempDir を用意する。
    mod tempfile_shim {
        use std::path::{Path, PathBuf};

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new() -> Self {
                let mut dir = std::env::temp_dir();
                dir.push(format!(
                    "voice-client-test-{}-{}",
                    std::process::id(),
                    uuid::Uuid::new_v4()
                ));
                std::fs::create_dir_all(&dir).expect("temp dir 作成");
                Self(dir)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
