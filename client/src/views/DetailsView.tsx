import { useEffect, useState } from "react";
import pkg from "../../package.json";
import { approveFingerprint, pair, probeFingerprint, repair } from "../api";
import { FormGroup, FormRow, TextInput } from "../components/settings/FormGroup";
import Button from "../components/ui/Button";
import Icon from "../components/ui/Icon";
import { formatFingerprintColonUppercase, normalizeFingerprint } from "../fingerprint";
import type { VoiceState } from "../lib/useVoiceState";
import styles from "./DetailsView.module.css";

interface DetailsViewProps {
  voice: VoiceState;
}

// Rust 側から想定外の形式のフィンガープリントが来ても描画自体は落とさない
// (フォーマット不能なら生の値をそのまま表示する)。
function safeFormatFingerprint(value: string): string {
  try {
    return formatFingerprintColonUppercase(value);
  } catch {
    return value;
  }
}

export default function DetailsView({ voice }: DetailsViewProps) {
  const { status } = voice;
  const [serverUrl, setServerUrl] = useState(status?.server_url ?? "");
  const [reconnecting, setReconnecting] = useState(false);
  const [reconnectStatus, setReconnectStatus] = useState("");
  const [copied, setCopied] = useState(false);

  const [code, setCode] = useState("");
  const [pairing, setPairing] = useState(false);
  const [pairStatus, setPairStatus] = useState("");

  const [probed, setProbed] = useState<string | null>(null);
  const [manualInput, setManualInput] = useState("");
  const [manualStatus, setManualStatus] = useState("");

  // status は useVoiceState 内で非同期に取得されるため、マウント直後は null。
  // 未入力 (初期値のまま) の間だけ取得済みの server_url を反映する
  // (ユーザーが既に編集を始めていたら上書きしない)。
  useEffect(() => {
    if (status?.server_url && serverUrl === "") {
      setServerUrl(status.server_url);
    }
  }, [status?.server_url, serverUrl]);

  async function handleCopyFingerprint() {
    if (!status?.fingerprint) return;
    try {
      await navigator.clipboard.writeText(safeFormatFingerprint(status.fingerprint));
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      // クリップボード API が使えない環境では黙ってスキップする (表示自体は読めるため実害は小さい)。
    }
  }

  async function handleReconnect() {
    setReconnecting(true);
    setReconnectStatus("再接続を試みています…");
    try {
      await repair(serverUrl);
      setReconnectStatus("再接続しました。");
    } catch (e) {
      setReconnectStatus(`再接続に失敗: ${e}`);
    } finally {
      setReconnecting(false);
    }
  }

  async function handlePair() {
    setPairing(true);
    setPairStatus("ペアリング中…");
    try {
      await pair(serverUrl, code);
      setPairStatus("ペアリングが完了しました。");
      setCode("");
    } catch (e) {
      const message = String(e);
      setPairStatus(
        message.includes("403")
          ? "コードが無効か期限切れです。サーバーで `voice-server pair` を実行し直してください。"
          : `ペアリングに失敗: ${message}`,
      );
    } finally {
      setPairing(false);
    }
  }

  async function handleProbe() {
    setManualStatus("サーバーへ接続してフィンガープリントを取得中…");
    try {
      const fingerprint = await probeFingerprint();
      setProbed(fingerprint);
      setManualStatus("下の値と `voice-server cert fingerprint` の出力を見比べて、一致すれば承認してください。");
    } catch (e) {
      setManualStatus(`取得に失敗: ${e}`);
    }
  }

  async function handleApprove(fingerprint: string) {
    try {
      await approveFingerprint(fingerprint);
      setManualStatus("承認しました。以後この値で証明書をピン留めします。");
    } catch (e) {
      setManualStatus(`承認の保存に失敗: ${e}`);
    }
  }

  async function handleApproveManualInput() {
    let normalized: string;
    try {
      normalized = normalizeFingerprint(manualInput);
    } catch (e) {
      setManualStatus(`入力形式が不正: ${e instanceof Error ? e.message : e}`);
      return;
    }
    await handleApprove(normalized);
  }

  return (
    <div className={styles.view}>
      <p className={styles.title}>詳細</p>

      <FormGroup title="端末情報">
        <FormRow title="端末名">
          <span>{status?.device_name ?? "-"}</span>
        </FormRow>
        <FormRow title="フィンガープリント" hint="voice-server cert fingerprint の出力と照合できる">
          <div className={styles.copyRow}>
            <span className={styles.mono}>
              {status?.fingerprint ? safeFormatFingerprint(status.fingerprint) : "未設定"}
            </span>
            {status?.fingerprint && (
              <Button variant="tertiary" size="sm" icon onClick={handleCopyFingerprint} aria-label="フィンガープリントをコピー">
                <Icon name={copied ? "check" : "copy"} size={13} />
              </Button>
            )}
          </div>
        </FormRow>
        <FormRow title="バージョン">
          <span>{pkg.version}</span>
        </FormRow>
      </FormGroup>

      <FormGroup title="再接続">
        <FormRow title="サーバー URL">
          <TextInput value={serverUrl} onChange={(e) => setServerUrl(e.target.value)} disabled={reconnecting} />
        </FormRow>
        <FormRow title="再ペアリング" hint={reconnectStatus || "接続先を変更したときに使う"}>
          <Button variant="secondary" size="sm" onClick={handleReconnect} disabled={reconnecting || !serverUrl}>
            <Icon name="refresh" size={13} />
            再接続
          </Button>
        </FormRow>
      </FormGroup>

      <details className={styles.advanced}>
        <summary className={styles.advancedSummary}>
          <Icon name="chevron-right" size={14} className={styles.chevron} />
          手動ペアリング (上級者向け)
        </summary>
        <div className={styles.advancedBody}>
          <p className={styles.desc}>
            通常は自動接続で完了します。サーバーが 6 桁コードモードのときや、証明書フィンガープリントを個別に
            確認・差し替えたい場合にだけ以下を使ってください。
          </p>

          <div className={styles.inline}>
            <TextInput
              className={styles.grow}
              value={code}
              onChange={(e) => setCode(e.target.value)}
              maxLength={6}
              inputMode="numeric"
              placeholder="6桁コード"
              disabled={pairing}
            />
            <Button variant="secondary" size="sm" onClick={handlePair} disabled={pairing || code.length === 0}>
              ペアリング
            </Button>
          </div>
          {pairStatus && <p className={styles.desc} role="status">{pairStatus}</p>}

          <div className={styles.inline}>
            <Button variant="secondary" size="sm" onClick={handleProbe}>
              <Icon name="fingerprint" size={13} />
              フィンガープリントを取得
            </Button>
            {probed && (
              <Button variant="primary" size="sm" onClick={() => handleApprove(probed)}>
                取得値を承認
              </Button>
            )}
          </div>
          {probed && <p className={styles.mono}>{safeFormatFingerprint(probed)}</p>}

          <div className={styles.inline}>
            <TextInput
              className={styles.grow}
              value={manualInput}
              onChange={(e) => setManualInput(e.target.value)}
              placeholder="8C:4D:17:...:88 を貼り付け"
            />
            <Button variant="secondary" size="sm" onClick={handleApproveManualInput} disabled={!manualInput}>
              この値を承認
            </Button>
          </div>
          {manualStatus && <p className={styles.desc} role="status">{manualStatus}</p>}
        </div>
      </details>
    </div>
  );
}
