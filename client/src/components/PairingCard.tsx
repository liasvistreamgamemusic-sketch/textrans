import { useState } from "react";
import { pair } from "../api";
import { formatFingerprintColonUppercase } from "../fingerprint";
import { DEFAULT_SERVER_URL } from "../settingsDefaults";

interface PairingCardProps {
  initialServerUrl: string;
  onPaired: () => void;
}

// 初回セットアップの自動化 (実装計画): サーバー URL + 6桁コードだけでトークン発行から
// 保存までを完結させる。手動でのフィンガープリント承認 (FingerprintApproval) は
// 上級者向けとして別に残す。
export default function PairingCard({ initialServerUrl, onPaired }: PairingCardProps) {
  const [serverUrl, setServerUrl] = useState(initialServerUrl || DEFAULT_SERVER_URL);
  const [code, setCode] = useState("");
  const [status, setStatus] = useState("");
  const [fingerprintHex, setFingerprintHex] = useState<string | null>(null);
  const [pairing, setPairing] = useState(false);

  async function handlePair() {
    setPairing(true);
    setStatus("ペアリング中…");
    setFingerprintHex(null);
    try {
      const result = await pair(serverUrl, code);
      setFingerprintHex(result.fingerprint_hex);
      setStatus("接続設定完了。");
      onPaired();
    } catch (e) {
      const message = String(e);
      if (message.includes("403")) {
        setStatus(
          "コードが無効か期限切れ。サーバーで `voice-server pair` を実行し直してください。",
        );
      } else {
        setStatus(`ペアリングに失敗: ${message}`);
      }
    } finally {
      setPairing(false);
    }
  }

  return (
    <section style={{ border: "1px solid #888", borderRadius: 8, padding: "1rem", marginBottom: "1rem" }}>
      <h3>ペアリング (初回セットアップ)</h3>
      <p>
        サーバーで <code>voice-server pair</code> を実行して発行された6桁コードを入力してください。
      </p>
      <label>
        サーバー URL
        <input
          type="text"
          value={serverUrl}
          onChange={(e) => setServerUrl(e.target.value)}
          disabled={pairing}
        />
      </label>
      <label>
        6桁コード
        <input
          type="text"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          maxLength={6}
          inputMode="numeric"
          placeholder="123456"
          disabled={pairing}
        />
      </label>
      <div>
        <button onClick={handlePair} disabled={pairing || code.length === 0 || serverUrl.length === 0}>
          ペアリング
        </button>
        <span role="status">{status}</span>
      </div>
      {fingerprintHex && (
        <p>
          サーバーのフィンガープリント: <code>{formatFingerprintColonUppercase(fingerprintHex)}</code>
        </p>
      )}
    </section>
  );
}
