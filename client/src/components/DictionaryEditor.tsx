import { useEffect, useState } from "react";
import { getDictionary, putDictionary } from "../api";
import type { Dictionary, DictionaryTerm } from "../types";

const EMPTY_TERM: DictionaryTerm = { surface: "", aliases: [], replace: false };

export default function DictionaryEditor() {
  const [dictionary, setDictionary] = useState<Dictionary>({ terms: [] });
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getDictionary()
      .then(setDictionary)
      .catch((e) => setStatus(`読み込みに失敗: ${e}`))
      .finally(() => setLoading(false));
  }, []);

  function updateTerm(index: number, patch: Partial<DictionaryTerm>) {
    const terms = dictionary.terms.slice();
    terms[index] = { ...terms[index], ...patch };
    setDictionary({ terms });
  }

  function removeTerm(index: number) {
    const terms = dictionary.terms.slice();
    terms.splice(index, 1);
    setDictionary({ terms });
  }

  function addTerm() {
    setDictionary({ terms: [...dictionary.terms, { ...EMPTY_TERM }] });
  }

  async function handleSave() {
    setStatus("保存中…");
    try {
      await putDictionary(dictionary);
      setStatus("保存しました");
    } catch (e) {
      setStatus(`保存に失敗: ${e}`);
    }
  }

  if (loading) {
    return <p>読み込み中…</p>;
  }

  return (
    <section>
      <h2>辞書 (GET/PUT /v1/dictionary)</h2>
      <table>
        <thead>
          <tr>
            <th>正表記</th>
            <th>誤認識されやすい表記 (カンマ区切り)</th>
            <th>確定置換</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {dictionary.terms.map((term, index) => (
            <tr key={index}>
              <td>
                <input
                  value={term.surface}
                  onChange={(e) => updateTerm(index, { surface: e.target.value })}
                />
              </td>
              <td>
                <input
                  value={term.aliases.join(",")}
                  onChange={(e) =>
                    updateTerm(index, {
                      aliases: e.target.value
                        .split(",")
                        .map((s) => s.trim())
                        .filter((s) => s.length > 0),
                    })
                  }
                />
              </td>
              <td>
                <input
                  type="checkbox"
                  checked={term.replace}
                  onChange={(e) => updateTerm(index, { replace: e.target.checked })}
                />
              </td>
              <td>
                <button onClick={() => removeTerm(index)}>削除</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button onClick={addTerm}>項目を追加</button>
      <div>
        <button onClick={handleSave}>保存</button>
        <span role="status">{status}</span>
      </div>
    </section>
  );
}
