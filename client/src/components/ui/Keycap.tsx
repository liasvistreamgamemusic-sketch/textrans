import styles from "./Keycap.module.css";

const SYMBOL: Record<string, string> = {
  Ctrl: "Ctrl",
  Cmd: "⌘",
  Command: "⌘",
  Shift: "⇧",
  Alt: "⌥",
  Option: "⌥",
  Space: "Space",
  Esc: "Esc",
  Escape: "Esc",
};

// raycast.md の keycap コンポーネント (⌘ K のような物理キー風グリフ) を
// ホットキー文字列 ("Ctrl+Shift+Space") から組み立てる。
export default function Keycap({ hotkey }: { hotkey: string }) {
  const parts = hotkey.split("+").filter(Boolean);
  return (
    <span className={styles.row} aria-label={hotkey}>
      {parts.map((part, i) => (
        <span className={styles.key} key={`${part}-${i}`}>
          {SYMBOL[part] ?? part}
        </span>
      ))}
    </span>
  );
}
