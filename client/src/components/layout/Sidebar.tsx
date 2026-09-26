import Icon from "../ui/Icon";
import type { IconName } from "../ui/Icon";
import styles from "./Sidebar.module.css";

export type ViewId = "home" | "dictionary" | "settings" | "details";

const ITEMS: Array<{ id: ViewId; label: string; icon: IconName }> = [
  { id: "home", label: "ホーム", icon: "home" },
  { id: "dictionary", label: "辞書", icon: "book" },
  { id: "settings", label: "設定", icon: "settings" },
  { id: "details", label: "詳細", icon: "info" },
];

interface SidebarProps {
  current: ViewId;
  onChange: (view: ViewId) => void;
}

export default function Sidebar({ current, onChange }: SidebarProps) {
  return (
    <nav className={styles.nav} aria-label="メインナビゲーション">
      <div className={styles.brand}>
        <span className={styles.mark}>V</span>
        voice-client
      </div>
      {ITEMS.map((item) => (
        <button
          key={item.id}
          type="button"
          className={styles.item}
          aria-current={current === item.id ? "page" : undefined}
          onClick={() => onChange(item.id)}
        >
          <Icon name={item.icon} size={15} />
          {item.label}
        </button>
      ))}
      <div className={styles.spacer} />
    </nav>
  );
}
