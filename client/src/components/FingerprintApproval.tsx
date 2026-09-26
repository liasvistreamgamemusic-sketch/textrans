import { useState } from "react";
import { approveFingerprint, probeFingerprint } from "../api";
import { normalizeFingerprint } from "../fingerprint";

// 初回接続時の証明書フィンガープリント承認 (design.md §3.1, §5.8)。
// `voice-server cert fingerprint` の出力とここに表示された値を、ユーザー自身の目で比較する。
export default function FingerprintApproval() {
  const [probed, setProbed] = useState<string | null>(null);
  const [manualInput, setManualInput] = useState("");
  const [status, setStatus] = useState("");

  async function handleProbe() {
    setStatus("サーバーへ接続してフィンガープリントを取得中…");
    try {
      const fingerprint = await probeFingerprint();
      setProbed(fingerprint);
      setStatus("下の値と `voice-server cert fingerprint` の出力を見比べて、一致すれば承認してください。");
    } catch (e) {
      setStatus(`取得に失敗: ${e}`);
    }
  }

  async function handleApprove() {
    if (!probed) return;
    try {
      await approveFingerprint(probed);
      setStatus("承認しました。以後この値で証明書をピン留めします。");
    } catch (e) {
      setStatus(`承認の保存に失敗: ${e}`);
    }
  }

  // `voice-server cert fingerprint` の出力 (コロン区切り・大文字) をそのまま貼り付けても、
  // クライアント側の計算値 (コロン無し・小文字) と同じ形に正規化してから承認する。
  // Rust 側 (`normalize_fingerprint`) と同じ規則なので、ここで通れば後段でも一致する。
  async function handleApproveManualInput() {
    let normalized: string;
    try {
      normalized = normalizeFingerprint(manualInput);
    } catch (e) {
      setStatus(`入力形式が不正: ${e instanceof Error ? e.message : e}`);
      return;
    }
    try {
      await approveFingerprint(normalized);
      setStatus(`承認しました (正規化後: ${normalized})。以後この値で証明書をピン留めします。`);
    } catch (e) {
      setStatus(`承認の保存に失敗: ${e}`);
    }
  }

  return (
    <section>
      <h2>サーバー証明書の承認</h2>
      <p>
        自宅LAN内の自己署名証明書のため、初回だけ手動で確認します。サーバー側で
        <code>voice-server cert fingerprint</code> を実行した結果と、下のボタンで取得した値が
        一致することを確認してから承認してください。
      </p>
      <button onClick={handleProbe}>サーバーに接続してフィンガープリントを取得</button>
      {probed && (
        <div>
          <p>
            取得した値: <code>{probed}</code>
          </p>
          <button onClick={handleApprove}>この値を承認する</button>
        </div>
      )}

      <hr />
      <h3>手動入力で承認する</h3>
      <p>
        <code>voice-server cert fingerprint</code> の出力 (コロン区切り・大文字でも構わない) を
        そのまま貼り付けてください。
      </p>
      <label>
        <input
          type="text"
          value={manualInput}
          onChange={(e) => setManualInput(e.target.value)}
          placeholder="8C:4D:17:...:88 または 8c4d17...88"
        />
      </label>
      <button onClick={handleApproveManualInput}>この入力を承認する</button>

      <p role="status">{status}</p>
    </section>
  );
}
