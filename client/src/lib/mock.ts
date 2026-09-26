// ブラウザ単体 (Tauri 無し) で UI を確認するためのモック実装。
// `window.__TAURI_INTERNALS__` が無い環境でのみ api.ts から使われる。
// 実データは持たず、実際の Rust コマンド/イベントの形をそのまま模倣する。

import type {
  Dictionary,
  DictionaryTerm,
  HistoryItem,
  InsertFailedEvent,
  LevelEvent,
  PairOutcome,
  Phase,
  Settings,
  Status,
} from "../types";

type Listener<T> = (payload: T) => void;

const listeners = {
  status: new Set<Listener<Status>>(),
  level: new Set<Listener<LevelEvent>>(),
  result: new Set<Listener<HistoryItem>>(),
  insert_failed: new Set<Listener<InsertFailedEvent>>(),
};

let idCounter = 0;
function nextId(): string {
  idCounter += 1;
  return `mock-${idCounter}`;
}

const MOCK_SETTINGS: Settings = {
  server_url: "wss://192.168.11.10:8765",
  server_fingerprint_hex:
    "54128135e12236ae0c9a3493ef85a5a83fec44bd8a226e4e70f9c036346fa654",
  hotkey: "Ctrl+Shift+Space",
  operation_mode: "push_to_talk",
  output_mode: "clean",
  insert_method: "clipboard_paste",
  mic_device: null,
  always_open_mic: false,
  paste_wait_overrides: {},
  select_after_insert: false,
};

let settings: Settings = { ...MOCK_SETTINGS };

let status: Status = {
  connected: true,
  paired: true,
  server_url: settings.server_url,
  device_name: "MacBook Pro (モック)",
  fingerprint: settings.server_fingerprint_hex,
  ax_trusted: true,
  phase: "idle",
  last_error: null,
};

function emitStatus(patch: Partial<Status>) {
  status = { ...status, ...patch };
  for (const fn of listeners.status) fn(status);
}

const SAMPLE_RESULTS: Array<{ mode: HistoryItem["mode"]; text: string; raw: string; flags: string[] }> = [
  {
    mode: "clean",
    text: "来週の定例会議の議事録は、私の方でまとめて共有します。",
    raw: "えーと来週の定例の議事録は自分の方でまとめて共有します",
    flags: ["dictionary_applied"],
  },
  {
    mode: "translate_en",
    text: "I'll finish the API review by tomorrow morning.",
    raw: "APIのレビューを明日の朝までに終わらせます",
    flags: [],
  },
  {
    mode: "clean",
    text: "voice-server の再起動後、フィンガープリントの再承認が必要です。",
    raw: "ボイスサーバーの再起動後フィンガープリントの再承認が必要です",
    flags: ["dictionary_applied"],
  },
  {
    mode: "raw",
    text: "とりあえずここまでで一旦切ります",
    raw: "とりあえずここまでで一旦切ります",
    flags: [],
  },
  {
    mode: "clean",
    text: "辞書に「Kubernetes」を追加して、誤認識の「クバネティス」を紐づけました。",
    raw: "辞書にクバネティスを追加して誤認識のクバネティスを紐付けました",
    flags: ["dictionary_applied", "low_confidence"],
  },
];

function makeHistoryItem(index: number, inserted: boolean): HistoryItem {
  const sample = SAMPLE_RESULTS[index % SAMPLE_RESULTS.length];
  const minutesAgo = (SAMPLE_RESULTS.length - index) * 6;
  return {
    id: nextId(),
    at: new Date(Date.now() - minutesAgo * 60_000).toISOString(),
    mode: sample.mode,
    text: sample.text,
    raw_text: sample.raw,
    flags: sample.flags,
    timings: {
      asr_ms: 420 + index * 30,
      llm_ms: 380 + index * 45,
      total_ms: 900 + index * 70,
    },
    inserted,
  };
}

let history: HistoryItem[] = SAMPLE_RESULTS.map((_, i) => makeHistoryItem(i, i !== 1));

let dictionary: Dictionary = {
  terms: [
    { surface: "Kubernetes", aliases: ["クバネティス", "クーバネティス"], replace: true },
    { surface: "voice-server", aliases: ["ボイスサーバー", "ヴォイスサーバー"], replace: true },
    { surface: "textrans", aliases: ["テキストランス", "テクストランス"], replace: false },
    { surface: "Raycast", aliases: ["レイキャスト"], replace: true },
    { surface: "Tauri", aliases: ["トーリ", "タウリ"], replace: false },
    { surface: "rubato", aliases: ["ルバート"], replace: false },
    { surface: "enigo", aliases: ["エニグモ"], replace: false },
    { surface: "faster-whisper", aliases: ["ファスターウィスパー"], replace: true },
  ],
};

// 起動直後に一度だけ、待機 → 録音 → 待機 → 挿入完了 のサイクルを自動再生して
// 状態カード・波形・履歴追加のトランジションを目視確認できるようにする。
let cycleStarted = false;
function startPhaseCycle() {
  if (cycleStarted) return;
  cycleStarted = true;

  const reduced =
    typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  async function runOnce() {
    await sleep(2600);
    if (reduced) return; // reduced-motion では自動サイクルを回さず idle のまま保つ
    setPhase("recording");
    const levelTimer = setInterval(() => {
      const rms = 0.08 + Math.random() * 0.5;
      for (const fn of listeners.level) fn({ rms });
    }, 50);
    await sleep(2200);
    clearInterval(levelTimer);
    setPhase("waiting");
    await sleep(700);
    setPhase("inserting");
    await sleep(500);
    setPhase("idle");
    const item = makeHistoryItem(history.length, true);
    history = [item, ...history];
    for (const fn of listeners.result) fn(item);
  }

  function loop() {
    runOnce().finally(() => {
      setTimeout(loop, 9000);
    });
  }
  loop();
}

function setPhase(phase: Phase) {
  emitStatus({ phase });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// api.ts の invoke() 呼び出しをコマンド名で分岐して模倣する。
export function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  startPhaseCycle();
  switch (cmd) {
    case "get_status":
      return Promise.resolve(status as unknown as T);
    case "get_history":
      return Promise.resolve(history as unknown as T);
    case "copy_history_item":
      return Promise.resolve(undefined as unknown as T);
    case "open_accessibility_settings":
      return Promise.resolve(undefined as unknown as T);
    case "get_settings":
      return Promise.resolve(settings as unknown as T);
    case "save_settings":
      settings = args?.settings as Settings;
      emitStatus({ server_url: settings.server_url });
      return Promise.resolve(undefined as unknown as T);
    case "has_token":
      return Promise.resolve(status.paired as unknown as T);
    case "repair": {
      const serverUrl = (args?.serverUrl as string) ?? settings.server_url;
      settings = { ...settings, server_url: serverUrl };
      emitStatus({ server_url: serverUrl, connected: true });
      return Promise.resolve(status as unknown as T);
    }
    case "pair": {
      const serverUrl = (args?.serverUrl as string) ?? settings.server_url;
      const fingerprint =
        "54128135e12236ae0c9a3493ef85a5a83fec44bd8a226e4e70f9c036346fa654";
      settings = { ...settings, server_url: serverUrl, server_fingerprint_hex: fingerprint };
      emitStatus({ server_url: serverUrl, paired: true, connected: true, fingerprint });
      return Promise.resolve({ fingerprint_hex: fingerprint } as PairOutcome as unknown as T);
    }
    case "probe_fingerprint":
      return Promise.resolve(
        "54128135e12236ae0c9a3493ef85a5a83fec44bd8a226e4e70f9c036346fa654" as unknown as T,
      );
    case "approve_fingerprint":
      emitStatus({ fingerprint: args?.fingerprintHex as string });
      return Promise.resolve(undefined as unknown as T);
    case "get_dictionary":
      return Promise.resolve(dictionary as unknown as T);
    case "put_dictionary":
      dictionary = args?.dictionary as Dictionary;
      return Promise.resolve(undefined as unknown as T);
    default:
      return Promise.reject(new Error(`モック未対応のコマンド: ${cmd}`));
  }
}

// api.ts の listen() 呼び出しをイベント名で分岐して模倣する。
export function mockListen<T>(event: string, handler: Listener<T>): Promise<() => void> {
  startPhaseCycle();
  const key = event.replace("voice://", "") as keyof typeof listeners;
  const set = listeners[key] as Set<Listener<T>> | undefined;
  if (!set) {
    return Promise.resolve(() => {});
  }
  set.add(handler);
  if (key === "status") {
    handler(status as unknown as T);
  }
  return Promise.resolve(() => {
    set.delete(handler);
  });
}

export type { DictionaryTerm };
