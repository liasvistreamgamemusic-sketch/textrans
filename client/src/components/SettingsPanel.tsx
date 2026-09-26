import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "../api";
import type { Settings } from "../types";

const DEFAULT_SETTINGS: Settings = {
  server_url: "wss://192.168.11.10:8765",
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

export default function SettingsPanel() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [status, setStatus] = useState<string>("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((e) => setStatus(`読み込みに失敗: ${e}`))
      .finally(() => setLoading(false));
  }, []);

  async function handleSave() {
    setStatus("保存中…");
    try {
      await saveSettings(settings);
      setStatus("保存しました");
    } catch (e) {
      setStatus(`保存に失敗: ${e}`);
    }
  }

  if (loading) {
    return <p>読み込み中…</p>;
  }

  return (
    <section>
      <h2>設定</h2>
      <label>
        サーバー URL
        <input
          type="text"
          value={settings.server_url}
          onChange={(e) => setSettings({ ...settings, server_url: e.target.value })}
        />
      </label>
      <label>
        ホットキー
        <input
          type="text"
          value={settings.hotkey}
          onChange={(e) => setSettings({ ...settings, hotkey: e.target.value })}
        />
      </label>
      <label>
        操作モード
        <select
          value={settings.operation_mode}
          onChange={(e) =>
            setSettings({ ...settings, operation_mode: e.target.value as Settings["operation_mode"] })
          }
        >
          <option value="push_to_talk">押している間だけ録音</option>
          <option value="toggle">トグル</option>
        </select>
      </label>
      <label>
        出力モード
        <select
          value={settings.output_mode}
          onChange={(e) => setSettings({ ...settings, output_mode: e.target.value as Settings["output_mode"] })}
        >
          <option value="clean">清書</option>
          <option value="raw">生テキスト</option>
          <option value="translate_en">英訳</option>
        </select>
      </label>
      <label>
        挿入方式
        <select
          value={settings.insert_method}
          onChange={(e) =>
            setSettings({ ...settings, insert_method: e.target.value as Settings["insert_method"] })
          }
        >
          <option value="clipboard_paste">クリップボード貼り付け</option>
          <option value="direct_type">文字の直接送出</option>
        </select>
      </label>
      <label>
        <input
          type="checkbox"
          checked={settings.always_open_mic}
          onChange={(e) => setSettings({ ...settings, always_open_mic: e.target.checked })}
        />
        マイクを常時開いておく
      </label>
      <label>
        <input
          type="checkbox"
          checked={settings.select_after_insert}
          onChange={(e) => setSettings({ ...settings, select_after_insert: e.target.checked })}
        />
        挿入後に挿入範囲を選択状態にする
      </label>
      <div>
        <button onClick={handleSave}>保存</button>
        <span role="status">{status}</span>
      </div>
    </section>
  );
}
