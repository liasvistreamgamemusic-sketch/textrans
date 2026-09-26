import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { mockInvoke, mockListen } from "./lib/mock";
import type {
  Dictionary,
  HistoryItem,
  InsertFailedEvent,
  LevelEvent,
  PairOutcome,
  Settings,
  Status,
} from "./types";

// Rust 側の `#[tauri::command]` (src-tauri/src/commands.rs) を薄く包む。
// エラーは Rust 側で `Result<T, String>` として返るので、そのまま invoke の reject になる。
//
// `window.__TAURI_INTERNALS__` が無いブラウザ単体環境 (pnpm dev をブラウザで開いた場合) では
// 全呼び出しをモック実装 (./lib/mock.ts) に差し替える。スクリーンショット確認・見た目の調整を
// Tauri 無しで行えるようにするための切り替えで、実装(Rust 呼び出し)には一切影響しない。
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return isTauri() ? tauriInvoke<T>(cmd, args) : mockInvoke<T>(cmd, args);
}

function listenEvent<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (isTauri()) {
    return tauriListen<T>(event, (e) => handler(e.payload));
  }
  return mockListen<T>(event, handler);
}

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

export function hasToken(): Promise<boolean> {
  return invoke("has_token");
}

export function pair(serverUrl: string, code?: string, deviceName?: string): Promise<PairOutcome> {
  return invoke("pair", { serverUrl, code, deviceName });
}

export function repair(serverUrl: string): Promise<Status> {
  return invoke("repair", { serverUrl });
}

export function getDictionary(): Promise<Dictionary> {
  return invoke("get_dictionary");
}

export function putDictionary(dictionary: Dictionary): Promise<void> {
  return invoke("put_dictionary", { dictionary });
}

export function getStatus(): Promise<Status> {
  return invoke("get_status");
}

export function getHistory(): Promise<HistoryItem[]> {
  return invoke("get_history");
}

export function copyHistoryItem(id: string): Promise<void> {
  return invoke("copy_history_item", { id });
}

export function openAccessibilitySettings(): Promise<void> {
  return invoke("open_accessibility_settings");
}

export function onStatus(handler: (status: Status) => void): Promise<UnlistenFn> {
  return listenEvent("voice://status", handler);
}

export function onLevel(handler: (level: LevelEvent) => void): Promise<UnlistenFn> {
  return listenEvent("voice://level", handler);
}

export function onResult(handler: (item: HistoryItem) => void): Promise<UnlistenFn> {
  return listenEvent("voice://result", handler);
}

export function onInsertFailed(handler: (event: InsertFailedEvent) => void): Promise<UnlistenFn> {
  return listenEvent("voice://insert_failed", handler);
}
