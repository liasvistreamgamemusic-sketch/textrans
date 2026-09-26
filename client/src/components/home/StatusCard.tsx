import type { Phase } from "../../types";
import Icon from "../ui/Icon";
import Keycap from "../ui/Keycap";
import Waveform from "./Waveform";
import styles from "./StatusCard.module.css";

const PHASE_META: Record<Phase, { title: string; subtitle: string }> = {
  idle: { title: "待機中", subtitle: "ホットキーを押して話しかけてください" },
  recording: { title: "聞いています…", subtitle: "話し終えたらキーを離してください" },
  waiting: { title: "処理中…", subtitle: "音声認識と清書を行っています" },
  inserting: { title: "挿入中…", subtitle: "カーソル位置にテキストを挿入しています" },
};

interface StatusCardProps {
  phase: Phase;
  level: number;
  hotkey: string;
}

export default function StatusCard({ phase, level, hotkey }: StatusCardProps) {
  const meta = PHASE_META[phase];
  return (
    <section className={`${styles.card} reveal`} data-phase={phase} aria-live="polite">
      <div className={styles.iconWrap}>
        {phase === "idle" && <Icon name="mic" size={26} />}
        {phase === "recording" && <Icon name="mic" size={26} />}
        {phase === "waiting" && <Icon name="loader" size={26} className={styles.spin} />}
        {phase === "inserting" && <Icon name="check" size={26} />}
      </div>
      {phase === "recording" ? (
        <Waveform active level={level} />
      ) : (
        <p className={styles.title}>{meta.title}</p>
      )}
      <p className={styles.subtitle}>{phase === "recording" ? meta.title : meta.subtitle}</p>
      {phase === "idle" && (
        <div className={styles.hotkeyRow}>
          <Icon name="keyboard" size={14} />
          <Keycap hotkey={hotkey} />
          <span>を押している間、録音します</span>
        </div>
      )}
    </section>
  );
}
