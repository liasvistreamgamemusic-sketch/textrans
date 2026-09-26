import { useState } from "react";
import SettingsPanel from "./components/SettingsPanel";
import DictionaryEditor from "./components/DictionaryEditor";
import FingerprintApproval from "./components/FingerprintApproval";

type Tab = "settings" | "dictionary" | "fingerprint";

export default function App() {
  const [tab, setTab] = useState<Tab>("settings");

  return (
    <main style={{ fontFamily: "sans-serif", padding: "1rem", maxWidth: 720 }}>
      <h1>voice-client</h1>
      <nav style={{ display: "flex", gap: "0.5rem", marginBottom: "1rem" }}>
        <button onClick={() => setTab("settings")} disabled={tab === "settings"}>
          設定
        </button>
        <button onClick={() => setTab("dictionary")} disabled={tab === "dictionary"}>
          辞書
        </button>
        <button onClick={() => setTab("fingerprint")} disabled={tab === "fingerprint"}>
          サーバー証明書
        </button>
      </nav>
      {tab === "settings" && <SettingsPanel />}
      {tab === "dictionary" && <DictionaryEditor />}
      {tab === "fingerprint" && <FingerprintApproval />}
    </main>
  );
}
