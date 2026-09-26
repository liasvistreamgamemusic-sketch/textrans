import { openAccessibilitySettings } from "../api";
import AccessibilityWarning from "../components/home/AccessibilityWarning";
import HistoryList from "../components/home/HistoryList";
import StatusCard from "../components/home/StatusCard";
import type { VoiceState } from "../lib/useVoiceState";
import styles from "./HomeView.module.css";

interface HomeViewProps {
  voice: VoiceState;
  hotkey: string;
}

export default function HomeView({ voice, hotkey }: HomeViewProps) {
  const { status, history, level, failReasons } = voice;

  return (
    <div className={styles.view}>
      {status && !status.ax_trusted && (
        <AccessibilityWarning onOpenSettings={() => openAccessibilitySettings()} />
      )}
      <StatusCard phase={status?.phase ?? "idle"} level={level} hotkey={hotkey} />
      <HistoryList items={history} failReasons={failReasons} />
    </div>
  );
}
