import type { ReactNode } from "react";
import styles from "./EmptyState.module.css";

// 自作 SVG (辞書カードの積み重ねを図案化)。汎用アイコンタイルではなく文脈に合う図版にする。
function DictionaryArt() {
  return (
    <svg width="88" height="64" viewBox="0 0 88 64" className={styles.art} aria-hidden="true">
      <rect x="10" y="18" width="52" height="36" rx="6" fill="none" stroke="currentColor" strokeWidth="1.6" opacity="0.5" />
      <rect x="22" y="10" width="52" height="36" rx="6" fill="none" stroke="currentColor" strokeWidth="1.6" opacity="0.75" />
      <rect x="34" y="4" width="50" height="34" rx="6" fill="var(--color-surface-2)" stroke="currentColor" strokeWidth="1.6" />
      <line x1="42" y1="15" x2="66" y2="15" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <line x1="42" y1="23" x2="76" y2="23" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" opacity="0.6" />
      <line x1="42" y1="31" x2="58" y2="31" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" opacity="0.6" />
    </svg>
  );
}

interface EmptyStateProps {
  title: string;
  description: string;
  action?: ReactNode;
}

export default function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <div className={styles.wrap}>
      <DictionaryArt />
      <p className={styles.title}>{title}</p>
      <p className={styles.desc}>{description}</p>
      {action}
    </div>
  );
}
