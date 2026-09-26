import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "../api";
import { FormGroup, FormRow, TextInput } from "../components/settings/FormGroup";
import Button from "../components/ui/Button";
import SegmentedControl from "../components/ui/SegmentedControl";
import Toggle from "../components/ui/Toggle";
import { DEFAULT_SERVER_URL } from "../settingsDefaults";
import type { Settings } from "../types";
import styles from "./SettingsView.module.css";

const DEFAULT_SETTINGS: Settings = {
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

export default function SettingsView() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((e) => setStatus(`読み込みに失敗: ${e}`))
      .finally(() => setLoading(false));
  }, []);

  async function handleSave() {
    setSaving(true);
    setStatus("");
    try {
      await saveSettings(settings);
      setStatus("保存しました");
    } catch (e) {
      setStatus(`保存に失敗: ${e}`);
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return <p>読み込み中…</p>;
  }

  return (
    <div className={styles.view}>
      <div className={styles.header}>
        <p className={styles.title}>設定</p>
        <span className={styles.status} role="status">
          {status}
        </span>
      </div>

      <FormGroup title="接続">
        <FormRow title="サーバー URL" hint="voice-server の WebSocket エンドポイント">
          <TextInput
            value={settings.server_url}
            onChange={(e) => setSettings({ ...settings, server_url: e.target.value })}
          />
        </FormRow>
      </FormGroup>

      <FormGroup title="操作">
        <FormRow title="ホットキー">
          <TextInput
            value={settings.hotkey}
            onChange={(e) => setSettings({ ...settings, hotkey: e.target.value })}
          />
        </FormRow>
        <FormRow title="操作モード" hint="押している間だけ録音するか、トグルで開始/終了するか">
          <SegmentedControl
            aria-label="操作モード"
            value={settings.operation_mode}
            onChange={(operation_mode) => setSettings({ ...settings, operation_mode })}
            options={[
              { value: "push_to_talk", label: "押している間" },
              { value: "toggle", label: "トグル" },
            ]}
          />
        </FormRow>
      </FormGroup>

      <FormGroup title="出力">
        <FormRow title="出力モード">
          <SegmentedControl
            aria-label="出力モード"
            value={settings.output_mode}
            onChange={(output_mode) => setSettings({ ...settings, output_mode })}
            options={[
              { value: "clean", label: "清書" },
              { value: "raw", label: "生テキスト" },
              { value: "translate_en", label: "英訳" },
            ]}
          />
        </FormRow>
        <FormRow title="挿入方式">
          <SegmentedControl
            aria-label="挿入方式"
            value={settings.insert_method}
            onChange={(insert_method) => setSettings({ ...settings, insert_method })}
            options={[
              { value: "clipboard_paste", label: "クリップボード" },
              { value: "direct_type", label: "直接送出" },
            ]}
          />
        </FormRow>
        <FormRow title="挿入後に選択状態にする" hint="範囲選択でIME再変換しやすくする (既定オフ)">
          <Toggle
            label="挿入後に選択状態にする"
            checked={settings.select_after_insert}
            onChange={(select_after_insert) => setSettings({ ...settings, select_after_insert })}
          />
        </FormRow>
      </FormGroup>

      <FormGroup title="マイク">
        <FormRow title="マイクデバイス" hint="空欄は OS 既定のマイクを使用">
          <TextInput
            value={settings.mic_device ?? ""}
            placeholder="OS既定"
            onChange={(e) => setSettings({ ...settings, mic_device: e.target.value || null })}
          />
        </FormRow>
        <FormRow title="マイクを常時開いておく" hint="冒頭が欠けやすい環境向け (常時点灯インジケーターに注意)">
          <Toggle
            label="マイクを常時開いておく"
            checked={settings.always_open_mic}
            onChange={(always_open_mic) => setSettings({ ...settings, always_open_mic })}
          />
        </FormRow>
      </FormGroup>

      <Button variant="primary" onClick={handleSave} disabled={saving}>
        {saving ? "保存中…" : "保存する"}
      </Button>
    </div>
  );
}
