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
///
/// 実装計画: macOS の ad-hoc 署名アプリでは keychain アクセスが拒否される事例があるため、
/// keyring アクセスが失敗したら理由を warn ログに出したうえで、設定ファイルと同じディレクトリの
/// `token` ファイル (0600、ファイル所有者のみ読み書き可) へフォールバックする。
pub mod token {
    use std::path::{Path, PathBuf};

    use keyring::Entry;

    const SERVICE: &str = "textrans-voice-client";
    const ACCOUNT: &str = "voice-server-token";
    const FALLBACK_FILE_NAME: &str = "token";

    #[derive(Debug, thiserror::Error)]
    pub enum TokenError {
        #[error("キーチェーン/資格情報マネージャーへのアクセスに失敗: {0}")]
        Keyring(#[from] keyring::Error),
        #[error("トークンファイルの読み込みに失敗: {0}")]
        FileRead(#[source] std::io::Error),
        #[error("トークンファイルの書き込みに失敗: {0}")]
        FileWrite(#[source] std::io::Error),
    }

    fn entry() -> Result<Entry, TokenError> {
        Ok(Entry::new(SERVICE, ACCOUNT)?)
    }

    fn fallback_path(settings_dir: &Path) -> PathBuf {
        settings_dir.join(FALLBACK_FILE_NAME)
    }

    fn read_fallback(settings_dir: &Path) -> Result<Option<String>, TokenError> {
        let path = fallback_path(settings_dir);
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let trimmed = raw.trim();
                Ok(if trimmed.is_empty() { None } else { Some(trimmed.to_string()) })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(TokenError::FileRead(e)),
        }
    }

    fn write_fallback(settings_dir: &Path, token: &str) -> Result<(), TokenError> {
        let path = fallback_path(settings_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(TokenError::FileWrite)?;
        }
        std::fs::write(&path, token).map_err(TokenError::FileWrite)?;
        restrict_to_owner(&path);
        Ok(())
    }

    #[cfg(unix)]
    fn restrict_to_owner(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!("トークンファイルの権限設定 (0600) に失敗: {e}");
        }
    }

    #[cfg(not(unix))]
    fn restrict_to_owner(_path: &Path) {}

    fn delete_fallback(settings_dir: &Path) -> Result<(), TokenError> {
        let path = fallback_path(settings_dir);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(TokenError::FileWrite(e)),
        }
    }

    /// `settings_dir` は設定ファイル (`settings.json`) と同じディレクトリ (フォールバック用)。
    /// keyring アクセスが失敗した場合は理由を warn ログに出し、フォールバックファイルを試す。
    pub fn get(settings_dir: &Path) -> Result<Option<String>, TokenError> {
        let keyring_result = entry().and_then(|e| match e.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.into()),
        });
        match keyring_result {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::warn!(
                    "keyring からのトークン取得に失敗: {e}。設定ディレクトリのフォールバックファイルを試す"
                );
                read_fallback(settings_dir)
            }
        }
    }

    pub fn set(settings_dir: &Path, token: &str) -> Result<(), TokenError> {
        let keyring_result = entry().and_then(|e| e.set_password(token).map_err(Into::into));
        match keyring_result {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::warn!(
                    "keyring へのトークン保存に失敗: {e}。設定ディレクトリへフォールバックファイル (0600) として保存する"
                );
                write_fallback(settings_dir, token)
            }
        }
    }

    pub fn delete(settings_dir: &Path) -> Result<(), TokenError> {
        let keyring_result = entry().and_then(|e| match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.into()),
        });
        if let Err(e) = &keyring_result {
            tracing::warn!("keyring からのトークン削除に失敗: {e}");
        }
        // フォールバックファイルが残っていれば (keyring 利用時でも過去にフォールバックへ
        // 書いた形跡があれば) 一緒に削除する。
        delete_fallback(settings_dir)?;
        keyring_result
    }
}

/// UNIX エポック (UTC) からの経過時間を ISO 8601 (`YYYY-MM-DDTHH:MM:SS.mmmZ`) へ変換する。
/// `rest` モジュールが素の HTTP を組み立てて依存を増やさない方針を取っているのと同様、
/// 日時系クレート (chrono/time 等) を新設せず、うるう年を含め正しく変換できる公知のアルゴリズム
/// (Howard Hinnant の `civil_from_days`、public domain) をそのまま使う。
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

pub fn to_iso8601_utc(system_time: std::time::SystemTime) -> String {
    let duration = system_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = duration.as_secs() as i64;
    let millis = duration.subsec_millis();
    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// UI 契約の `HistoryItem`。直近 [`HISTORY_CAPACITY`] 件をメモリ上だけで保持する
/// (design.md §5.8、終了時に消える。設定ファイルには永続化しない)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryItem {
    pub id: String,
    pub at: String,
    pub mode: crate::protocol::Mode,
    pub text: String,
    pub raw_text: String,
    pub flags: Vec<crate::protocol::Flag>,
    pub timings: crate::protocol::Timings,
    pub inserted: bool,
}

#[derive(Debug, Default)]
pub struct History {
    entries: std::collections::VecDeque<HistoryItem>,
}

impl History {
    pub fn push(&mut self, item: HistoryItem) {
        self.entries.push_back(item);
        while self.entries.len() > HISTORY_CAPACITY {
            self.entries.pop_front();
        }
    }

    /// `id` に一致する項目が実際に挿入されたことを記録する (挿入は final 受信より後に完了するため)。
    pub fn mark_inserted(&mut self, id: &str) {
        if let Some(item) = self.entries.iter_mut().find(|e| e.id == id) {
            item.inserted = true;
        }
    }

    pub fn get(&self, id: &str) -> Option<&HistoryItem> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// `get_history` コマンド用。新しい順ではなく発話が起きた順 (古い→新しい) のまま返す。
    pub fn items(&self) -> Vec<HistoryItem> {
        self.entries.iter().cloned().collect()
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

    fn dummy_history_item(id: &str) -> HistoryItem {
        HistoryItem {
            id: id.to_string(),
            at: "2026-01-01T00:00:00.000Z".to_string(),
            mode: crate::protocol::Mode::Clean,
            text: format!("text-{id}"),
            raw_text: format!("raw-{id}"),
            flags: vec![],
            timings: crate::protocol::Timings { asr_ms: 1, llm_ms: 1, total_ms: 2 },
            inserted: false,
        }
    }

    #[test]
    fn history_keeps_only_last_n_entries() {
        let mut history = History::default();
        for i in 0..(HISTORY_CAPACITY + 5) {
            history.push(dummy_history_item(&format!("entry-{i}")));
        }
        assert_eq!(history.len(), HISTORY_CAPACITY);
        let first = history.items().into_iter().next().unwrap();
        assert_eq!(first.id, "entry-5");
    }

    #[test]
    fn history_mark_inserted_updates_matching_item_only() {
        let mut history = History::default();
        history.push(dummy_history_item("a"));
        history.push(dummy_history_item("b"));

        history.mark_inserted("a");

        assert!(history.get("a").unwrap().inserted);
        assert!(!history.get("b").unwrap().inserted);
    }

    #[test]
    fn history_get_returns_none_for_unknown_id() {
        let history = History::default();
        assert!(history.get("missing").is_none());
    }

    #[test]
    fn to_iso8601_utc_formats_unix_epoch() {
        assert_eq!(to_iso8601_utc(std::time::UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn to_iso8601_utc_formats_known_date_with_millis() {
        // 2000-01-01T00:00:00.500Z = 946684800.5 秒 (うるう年判定・世紀境界を含む既知の値)。
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(946_684_800_500);
        assert_eq!(to_iso8601_utc(t), "2000-01-01T00:00:00.500Z");
    }

    #[test]
    fn to_iso8601_utc_handles_leap_day() {
        // 2024-02-29T12:00:00Z = 1709208000 秒。
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_709_208_000);
        assert_eq!(to_iso8601_utc(t), "2024-02-29T12:00:00.000Z");
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
