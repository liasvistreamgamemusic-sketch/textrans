import { invoke } from "@tauri-apps/api/core";
import type { Dictionary, Settings } from "./types";

// Rust 側の `#[tauri::command]` (src-tauri/src/commands.rs) を薄く包む。
// エラーは Rust 側で `Result<T, String>` として返るので、そのまま invoke の reject になる。

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function saveSettings(settings: Settings): Promise<void> {
  return invoke("save_settings", { settings });
}

export function probeFingerprint(): Promise<string> {
  return invoke("probe_fingerprint");
}

export function approveFingerprint(fingerprintHex: string): Promise<void> {
  return invoke("approve_fingerprint", { fingerprintHex });
}

export function getDictionary(): Promise<Dictionary> {
  return invoke("get_dictionary");
}

export function putDictionary(dictionary: Dictionary): Promise<void> {
  return invoke("put_dictionary", { dictionary });
}
