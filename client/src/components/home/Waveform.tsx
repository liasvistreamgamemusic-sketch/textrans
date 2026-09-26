import { useEffect, useState } from "react";
import styles from "./Waveform.module.css";

const BAR_COUNT = 28;

interface WaveformProps {
  active: boolean;
  level: number; // 直近の RMS (0..1 目安)
}

// 録音中の RMS を棒グラフとして流し込む。`voice://level` (50ms ごと) を親から流し込んで
// 直近 BAR_COUNT 件のリングバッファとして描画する — 実データを反映する主役演出。
export default function Waveform({ active, level }: WaveformProps) {
  const [bars, setBars] = useState<number[]>(() => Array(BAR_COUNT).fill(0.05));

  useEffect(() => {
    if (!active) {
      setBars(Array(BAR_COUNT).fill(0.05));
      return;
    }
    setBars((prev) => [...prev.slice(1), Math.max(0.05, Math.min(1, level))]);
  }, [level, active]);

  return (
    <div className={styles.wave} data-active={active} role="img" aria-label="録音中の音量波形">
      {bars.map((value, i) => (
        <span key={i} className={styles.bar} style={{ transform: `scaleY(${value * 8})`, height: 6 }} />
      ))}
    </div>
  );
}
