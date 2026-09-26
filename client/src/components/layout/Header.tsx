import type { OutputMode, Status } from "../../types";
import Badge from "../ui/Badge";
import styles from "./Header.module.css";

const OUTPUT_MODE_LABEL: Record<OutputMode, string> = {
  clean: "清書",
  raw: "生テキスト",
  translate_en: "英訳",
};

function connectionState(status: Status | null): "connected" | "pairing" | "disconnected" {
  if (!status) return "disconnected";
  if (!status.paired) return "pairing";
  return status.connected ? "connected" : "disconnected";
}

const STATE_LABEL: Record<ReturnType<typeof connectionState>, string> = {
  connected: "接続中",
  pairing: "ペアリング中",
  disconnected: "切断",
};

interface HeaderProps {
  status: Status | null;
  outputMode: OutputMode;
}

export default function Header({ status, outputMode }: HeaderProps) {
  const state = connectionState(status);
  return (
    <header className={styles.header}>
      <div className={styles.status}>
        <span className={styles.dotWrap} data-state={state}>
          <span className={styles.ring} />
          <span className={styles.dot} data-state={state} />
        </span>
        <span className={styles.label}>{STATE_LABEL[state]}</span>
        {status && <span>{status.device_name}</span>}
      </div>
      <div className={styles.modeBadge}>
        <Badge tone="info">出力: {OUTPUT_MODE_LABEL[outputMode]}</Badge>
      </div>
    </header>
  );
}
