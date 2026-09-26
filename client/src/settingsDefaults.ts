// Rust 側の既定値 (src-tauri/src/settings/mod.rs の DEFAULT_SERVER_URL) と一致させる。
// SettingsPanel (既定設定) と PairingCard (未ペアリング時の入力初期値) の両方から使うので、
// リテラルの重複を避けてここに集約する。
export const DEFAULT_SERVER_URL = "wss://192.168.11.10:8765";
