import type { InputHTMLAttributes, ReactNode } from "react";
import styles from "./FormGroup.module.css";

export function FormGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className={`${styles.group} reveal`}>
      <p className={styles.title}>{title}</p>
      {children}
    </div>
  );
}

export function FormRow({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className={styles.row}>
      <div className={styles.rowLabel}>
        <span className={styles.rowTitle}>{title}</span>
        {hint && <span className={styles.rowHint}>{hint}</span>}
      </div>
      <div className={styles.rowControl}>{children}</div>
    </div>
  );
}

export function TextInput({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...rest} className={className ? `${styles.textInput} ${className}` : styles.textInput} />;
}
