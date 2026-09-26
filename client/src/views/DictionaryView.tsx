import { useEffect, useMemo, useState } from "react";
import { getDictionary, putDictionary } from "../api";
import TermCard from "../components/dictionary/TermCard";
import TermEditorPane from "../components/dictionary/TermEditorPane";
import Button from "../components/ui/Button";
import EmptyState from "../components/ui/EmptyState";
import Icon from "../components/ui/Icon";
import type { Dictionary, DictionaryTerm } from "../types";
import styles from "./DictionaryView.module.css";

const EMPTY_TERM: DictionaryTerm = { surface: "", aliases: [], replace: false };

export default function DictionaryView() {
  const [dictionary, setDictionary] = useState<Dictionary>({ terms: [] });
  const [savedDictionary, setSavedDictionary] = useState<Dictionary>({ terms: [] });
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    getDictionary()
      .then((d) => {
        setDictionary(d);
        setSavedDictionary(d);
        setSelected(d.terms.length > 0 ? 0 : null);
      })
      .catch((e) => setError(`読み込みに失敗: ${e}`))
      .finally(() => setLoading(false));
  }, []);

  const dirty = useMemo(
    () => JSON.stringify(dictionary) !== JSON.stringify(savedDictionary),
    [dictionary, savedDictionary],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return dictionary.terms
      .map((term, index) => ({ term, index }))
      .filter(
        ({ term }) =>
          q.length === 0 ||
          term.surface.toLowerCase().includes(q) ||
          term.aliases.some((a) => a.toLowerCase().includes(q)),
      );
  }, [dictionary.terms, query]);

  async function handleSave() {
    setSaving(true);
    setError("");
    try {
      await putDictionary(dictionary);
      setSavedDictionary(dictionary);
    } catch (e) {
      setError(`保存に失敗: ${e}`);
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (dirty && !saving) handleSave();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [dictionary, dirty, saving]);

  function updateSelected(patch: Partial<DictionaryTerm>) {
    if (selected === null) return;
    setDictionary((prev) => {
      const terms = prev.terms.slice();
      terms[selected] = { ...terms[selected], ...patch };
      return { terms };
    });
  }

  function addTerm() {
    setDictionary((prev) => ({ terms: [...prev.terms, { ...EMPTY_TERM }] }));
    setSelected(dictionary.terms.length);
    setQuery("");
  }

  function deleteSelected() {
    if (selected === null) return;
    setDictionary((prev) => {
      const terms = prev.terms.filter((_, i) => i !== selected);
      return { terms };
    });
    setSelected(null);
  }

  if (loading) {
    return <p>読み込み中…</p>;
  }

  const selectedTerm = selected !== null ? dictionary.terms[selected] : null;

  return (
    <div className={styles.view}>
      <div className={styles.list}>
        <div className={styles.toolbar}>
          <label className={styles.search}>
            <Icon name="search" size={14} />
            <input
              className={styles.searchInput}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="語を検索"
              aria-label="辞書を検索"
            />
          </label>
          <Button variant="tertiary" icon onClick={addTerm} aria-label="語を追加">
            <Icon name="plus" size={15} />
          </Button>
        </div>
        {filtered.length === 0 ? (
          <EmptyState
            title={query ? "見つかりませんでした" : "辞書が空です"}
            description={query ? "検索語を変えるか、新しい語を追加してください。" : "「+」から最初の語を追加してください。"}
          />
        ) : (
          <div className={styles.cards} role="list" aria-label="辞書の語一覧">
            {filtered.map(({ term, index }) => (
              <TermCard key={index} term={term} active={index === selected} onSelect={() => setSelected(index)} />
            ))}
          </div>
        )}
      </div>

      <div className={styles.pane}>
        {selectedTerm ? (
          <TermEditorPane
            term={selectedTerm}
            dirty={dirty}
            saving={saving}
            onChange={updateSelected}
            onSave={handleSave}
            onDelete={deleteSelected}
          />
        ) : (
          <EmptyState title="語を選択してください" description="左のリストから語を選ぶと、ここで編集できます。" />
        )}
        {error && <p role="status">{error}</p>}
      </div>
    </div>
  );
}
