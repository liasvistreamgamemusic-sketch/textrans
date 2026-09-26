import type { DictionaryTerm } from "../../types";
import Badge from "../ui/Badge";
import styles from "./TermCard.module.css";

interface TermCardProps {
  term: DictionaryTerm;
  active: boolean;
  onSelect: () => void;
}

export default function TermCard({ term, active, onSelect }: TermCardProps) {
  return (
    <button type="button" className={styles.card} aria-current={active} onClick={onSelect}>
      <span className={styles.title}>
        {term.surface || "(無題)"}
      </span>
      <div className={styles.badges}>
        <Badge tone="neutral">エイリアス {term.aliases.length}</Badge>
        {term.replace && <Badge tone="info">確定置換</Badge>}
      </div>
    </button>
  );
}
