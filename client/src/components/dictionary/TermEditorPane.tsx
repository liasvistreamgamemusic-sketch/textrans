import type { DictionaryTerm } from "../../types";
import Button from "../ui/Button";
import ChipInput from "../ui/ChipInput";
import Icon from "../ui/Icon";
import Toggle from "../ui/Toggle";
import styles from "./TermEditorPane.module.css";

interface TermEditorPaneProps {
  term: DictionaryTerm;
  dirty: boolean;
  saving: boolean;
  onChange: (patch: Partial<DictionaryTerm>) => void;
  onSave: () => void;
  onDelete: () => void;
}

export default function TermEditorPane({ term, dirty, saving, onChange, onSave, onDelete }: TermEditorPaneProps) {
  return (
    <div className={`${styles.card} reveal`}>
      <div className={styles.field}>
        <label className={styles.label} htmlFor="term-surface">
          正表記
        </label>
        <input
          id="term-surface"
          className={styles.input}
          value={term.surface}
          onChange={(e) => onChange({ surface: e.target.value })}
          placeholder="例: Kubernetes"
        />
      </div>

      <div className={styles.field}>
        <label className={styles.label} htmlFor="term-aliases">
          誤認識されやすい表記 (エイリアス)
        </label>
        <ChipInput
          aria-label="エイリアス"
          values={term.aliases}
          onChange={(aliases) => onChange({ aliases })}
          placeholder="入力して Enter または , で追加"
        />
      </div>

      <div className={styles.toggleRow}>
        <Toggle checked={term.replace} onChange={(replace) => onChange({ replace })} label="確定置換" />
        <div className={styles.field}>
          <span className={styles.label}>確定置換</span>
          <p className={styles.hint}>
            一般語と衝突する語はオフにして LLM に判断させる。オンにするとエイリアスを検出時に必ず正表記へ置き換える。
          </p>
        </div>
      </div>

      <div className={styles.footer}>
        <div className={styles.unsaved}>
          {dirty && (
            <>
              <span className={styles.dot} />
              未保存の変更があります
            </>
          )}
        </div>
        <div className={styles.actions}>
          <Button variant="danger" size="sm" onClick={onDelete}>
            <Icon name="x" size={13} />
            削除
          </Button>
          <Button variant="primary" size="sm" onClick={onSave} disabled={!dirty || saving}>
            {saving ? "保存中…" : "保存 (⌘S)"}
          </Button>
        </div>
      </div>
    </div>
  );
}
