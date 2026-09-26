import type { ReactNode } from "react";
import styles from "./Badge.module.css";

type Tone = "neutral" | "info" | "success" | "danger" | "warning";

export default function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`${styles.badge} ${styles[tone]}`}>{children}</span>;
}
