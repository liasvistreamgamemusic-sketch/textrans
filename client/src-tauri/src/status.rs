//! React 側と共有する `Status` (UI 契約 §3) の一元管理。
//!
//! 接続状態・フェーズ・AX 権限などの更新箇所は全てここを経由し、値が変わるたびに
//! `voice://status` イベントを発行する (React 側はこのイベント経由で常に最新状態を持てる)。
//! [`overlay`](crate::overlay) と同様、実 I/O (イベント発行) は呼び出し側から渡された
//! `AppHandle` を都度使う (`StatusStore` 自体は `AppHandle` を保持しない)。

use serde::{Deserialize, Serialize};
use std::sync::Mutex as StdMutex;
use tauri::{AppHandle, Emitter};

use crate::state::UiPhase;

pub const STATUS_EVENT: &str = "voice://status";

/// UI 契約の `phase` (`"idle"|"recording"|"waiting"|"inserting"`)。
/// `Deserialize` はトレイ (`crate::tray`) が `voice://status` の payload を読み戻すために必要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Recording,
    Waiting,
    Inserting,
}

impl From<UiPhase> for Phase {
    fn from(value: UiPhase) -> Self {
        match value {
            UiPhase::Idle => Phase::Idle,
            UiPhase::Recording => Phase::Recording,
            UiPhase::Waiting => Phase::Waiting,
            UiPhase::Inserting => Phase::Inserting,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub connected: bool,
    pub paired: bool,
    pub server_url: String,
    pub device_name: Option<String>,
    /// コロン区切り・大文字 (例 `8C:4D:...`)。内部表現 (コロン無し・小文字) からの変換は
    /// [`crate::ws::verifier::to_colon_upper`] を使う。
    pub fingerprint: Option<String>,
    pub ax_trusted: bool,
    pub phase: Phase,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct StatusFields {
    connected: bool,
    paired: bool,
    server_url: String,
    device_name: Option<String>,
    fingerprint: Option<String>,
    ax_trusted: bool,
    phase: Phase,
    last_error: Option<String>,
}

impl StatusFields {
    fn snapshot(&self) -> Status {
        Status {
            connected: self.connected,
            paired: self.paired,
            server_url: self.server_url.clone(),
            device_name: self.device_name.clone(),
            fingerprint: self.fingerprint.clone(),
            ax_trusted: self.ax_trusted,
            phase: self.phase,
            last_error: self.last_error.clone(),
        }
    }
}

/// [`Status`] の実体。各フィールドの更新は専用メソッド経由でのみ行い、値が実際に変わった
/// ときだけ `voice://status` を発行する (無変化での再エミットを避ける)。
pub struct StatusStore {
    fields: StdMutex<StatusFields>,
}

impl StatusStore {
    pub fn new(server_url: String, paired: bool, fingerprint: Option<String>) -> Self {
        Self {
            fields: StdMutex::new(StatusFields {
                connected: false,
                paired,
                server_url,
                device_name: None,
                fingerprint,
                ax_trusted: false,
                phase: Phase::Idle,
                last_error: None,
            }),
        }
    }

    pub fn snapshot(&self) -> Status {
        self.lock().snapshot()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StatusFields> {
        self.fields.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn emit(&self, app: &AppHandle) {
        let snapshot = self.snapshot();
        if let Err(e) = app.emit(STATUS_EVENT, snapshot) {
            tracing::warn!("{STATUS_EVENT} イベントの発行に失敗: {e}");
        }
    }

    pub fn set_connected(&self, app: &AppHandle, connected: bool) {
        {
            let mut f = self.lock();
            if f.connected == connected {
                return;
            }
            f.connected = connected;
        }
        self.emit(app);
    }

    pub fn set_paired(&self, app: &AppHandle, paired: bool) {
        {
            let mut f = self.lock();
            if f.paired == paired {
                return;
            }
            f.paired = paired;
        }
        self.emit(app);
    }

    pub fn set_server_url(&self, app: &AppHandle, server_url: String) {
        {
            let mut f = self.lock();
            if f.server_url == server_url {
                return;
            }
            f.server_url = server_url;
        }
        self.emit(app);
    }

    pub fn set_device_name(&self, app: &AppHandle, device_name: Option<String>) {
        {
            let mut f = self.lock();
            if f.device_name == device_name {
                return;
            }
            f.device_name = device_name;
        }
        self.emit(app);
    }

    pub fn set_fingerprint(&self, app: &AppHandle, fingerprint: Option<String>) {
        {
            let mut f = self.lock();
            if f.fingerprint == fingerprint {
                return;
            }
            f.fingerprint = fingerprint;
        }
        self.emit(app);
    }

    pub fn set_ax_trusted(&self, app: &AppHandle, ax_trusted: bool) {
        {
            let mut f = self.lock();
            if f.ax_trusted == ax_trusted {
                return;
            }
            f.ax_trusted = ax_trusted;
        }
        self.emit(app);
    }

    pub fn set_phase(&self, app: &AppHandle, phase: Phase) {
        {
            let mut f = self.lock();
            if f.phase == phase {
                return;
            }
            f.phase = phase;
        }
        self.emit(app);
    }

    pub fn set_last_error(&self, app: &AppHandle, last_error: Option<String>) {
        {
            let mut f = self.lock();
            if f.last_error == last_error {
                return;
            }
            f.last_error = last_error;
        }
        self.emit(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reflects_constructor_values() {
        let store = StatusStore::new("wss://example:8765".to_string(), true, Some("AA:BB".to_string()));
        let snapshot = store.snapshot();
        assert!(!snapshot.connected);
        assert!(snapshot.paired);
        assert_eq!(snapshot.server_url, "wss://example:8765");
        assert_eq!(snapshot.fingerprint, Some("AA:BB".to_string()));
        assert!(!snapshot.ax_trusted);
        assert_eq!(snapshot.phase, Phase::Idle);
        assert!(snapshot.last_error.is_none());
    }

    #[test]
    fn ui_phase_maps_to_status_phase() {
        assert_eq!(Phase::from(UiPhase::Idle), Phase::Idle);
        assert_eq!(Phase::from(UiPhase::Recording), Phase::Recording);
        assert_eq!(Phase::from(UiPhase::Waiting), Phase::Waiting);
        assert_eq!(Phase::from(UiPhase::Inserting), Phase::Inserting);
    }
}
