import { useEffect, useState } from "react";
import { getSettings } from "./api";
import Shell from "./components/layout/Shell";
import type { ViewId } from "./components/layout/Sidebar";
import { useVoiceState } from "./lib/useVoiceState";
import { DEFAULT_SERVER_URL } from "./settingsDefaults";
import type { Settings } from "./types";
import DetailsView from "./views/DetailsView";
import DictionaryView from "./views/DictionaryView";
import HomeView from "./views/HomeView";
import SettingsView from "./views/SettingsView";

const VIEW_IDS: ViewId[] = ["home", "dictionary", "settings", "details"];

function initialView(): ViewId {
  if (typeof window === "undefined") return "home";
  const param = new URLSearchParams(window.location.search).get("view");
  return VIEW_IDS.includes(param as ViewId) ? (param as ViewId) : "home";
}

const FALLBACK_SETTINGS: Settings = {
  server_url: DEFAULT_SERVER_URL,
  server_fingerprint_hex: null,
  hotkey: "Ctrl+Shift+Space",
  operation_mode: "push_to_talk",
  output_mode: "clean",
  insert_method: "clipboard_paste",
  mic_device: null,
  always_open_mic: false,
  paste_wait_overrides: {},
  select_after_insert: false,
};

export default function App() {
  const [view, setView] = useState<ViewId>(initialView);
  const [settings, setSettings] = useState<Settings>(FALLBACK_SETTINGS);
  const voice = useVoiceState();

  // ホーム画面のホットキー表示・ヘッダーの出力モード表示に必要な設定値。
  // 設定画面を離れて戻ってきたときに変更を反映できるよう、表示対象のタブに来るたび読み直す。
  useEffect(() => {
    if (view === "home" || view === "settings") {
      getSettings().then(setSettings).catch(() => undefined);
    }
  }, [view]);

  return (
    <Shell view={view} onChangeView={setView} status={voice.status} outputMode={settings.output_mode}>
      {view === "home" && <HomeView voice={voice} hotkey={settings.hotkey} />}
      {view === "dictionary" && <DictionaryView />}
      {view === "settings" && <SettingsView />}
      {view === "details" && <DetailsView voice={voice} />}
    </Shell>
  );
}
