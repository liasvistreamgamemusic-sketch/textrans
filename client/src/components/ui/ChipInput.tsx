import { useState } from "react";
import type { KeyboardEvent } from "react";
import Icon from "./Icon";
import styles from "./ChipInput.module.css";

interface ChipInputProps {
  values: string[];
  onChange: (values: string[]) => void;
  placeholder?: string;
  "aria-label": string;
}

// Enter または「,」区切りでエイリアスを追加するチップ入力 (辞書エディタ用)。
export default function ChipInput({ values, onChange, placeholder, ...rest }: ChipInputProps) {
  const [draft, setDraft] = useState("");

  function commit() {
    const trimmed = draft.trim();
    if (trimmed.length === 0) return;
    if (!values.includes(trimmed)) {
      onChange([...values, trimmed]);
    }
    setDraft("");
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commit();
    } else if (e.key === "Backspace" && draft.length === 0 && values.length > 0) {
      onChange(values.slice(0, -1));
    }
  }

  function removeAt(index: number) {
    onChange(values.filter((_, i) => i !== index));
  }

  return (
    <div className={styles.field} aria-label={rest["aria-label"]}>
      {values.map((value, index) => (
        <span className={styles.chip} key={`${value}-${index}`}>
          {value}
          <button
            type="button"
            className={styles.remove}
            aria-label={`${value} を削除`}
            onClick={() => removeAt(index)}
          >
            <Icon name="x" size={11} />
          </button>
        </span>
      ))}
      <input
        className={styles.input}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={handleKeyDown}
        onBlur={commit}
        placeholder={values.length === 0 ? placeholder : ""}
      />
    </div>
  );
}
