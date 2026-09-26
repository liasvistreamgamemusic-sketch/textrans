// Rust 側 (`src-tauri/src/settings/mod.rs`, `protocol/mod.rs`, `commands.rs`) の型と対応する。
// フィールド名・enum の文字列表現 (snake_case) は Rust の serde 設定と一致させる。

export type OutputMode = "clean" | "raw" | "translate_en";
export type OperationMode = "push_to_talk" | "toggle";
export type InsertMethod = "clipboard_paste" | "direct_type";

export interface Settings {
  server_url: string;
  server_fingerprint_hex: string | null;
  hotkey: string;
  operation_mode: OperationMode;
  output_mode: OutputMode;
  insert_method: InsertMethod;
  mic_device: string | null;
  always_open_mic: boolean;
  paste_wait_overrides: Record<string, number>;
  select_after_insert: boolean;
}

export interface DictionaryTerm {
  surface: string;
  aliases: string[];
  replace: boolean;
}

export interface Dictionary {
  terms: DictionaryTerm[];
}

// `pair` コマンド (src-tauri/src/commands.rs) の応答。
export interface PairOutcome {
  fingerprint_hex: string;
}

// クライアントの動作フェーズ (design.md §5.3 の状態遷移と対応)。
export type Phase = "idle" | "recording" | "waiting" | "inserting";

// `get_status` の応答 / `voice://status` イベントのペイロード。
export interface Status {
  connected: boolean;
  paired: boolean;
  server_url: string;
  device_name: string;
  fingerprint: string | null;
  ax_trusted: boolean;
  phase: Phase;
  last_error: string | null;
}

export interface Timings {
  asr_ms: number;
  llm_ms: number;
  total_ms: number;
}

// 挿入失敗の理由 (design.md §5.5 の前面ウィンドウ不一致など)。
export type InsertFailedReason = "accessibility" | "front_app_changed" | "other";

// `get_history` の応答要素 / `voice://result` イベントのペイロード。
export interface HistoryItem {
  id: string;
  at: string;
  mode: OutputMode;
  text: string;
  raw_text: string;
  flags: string[];
  timings: Timings;
  inserted: boolean;
}

// `voice://level` イベントのペイロード (録音中 50ms ごと)。
export interface LevelEvent {
  rms: number;
}

// `voice://insert_failed` イベントのペイロード。
export interface InsertFailedEvent {
  id: string;
  reason: InsertFailedReason;
  message: string;
}
