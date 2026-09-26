// Rust 側 (`src-tauri/src/settings/mod.rs`, `protocol/mod.rs`) の型と対応する。
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
