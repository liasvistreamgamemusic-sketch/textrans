import { useState } from "react";
import type { HistoryItem, InsertFailedReason } from "../../types";
import { copyHistoryItem } from "../../api";
import Badge from "../ui/Badge";
import Icon from "../ui/Icon";
import EmptyState from "../ui/EmptyState";
import styles from "./HistoryList.module.css";

const MODE_LABEL: Record<HistoryItem["mode"], string> = {
  clean: "清書",
  raw: "生テキスト",
  translate_en: "英訳",
};

const FAIL_REASON_LABEL: Record<InsertFailedReason, string> = {
  accessibility: "権限不足",
  front_app_changed: "挿入先が変わった",
  other: "挿入失敗",
};

function formatTime(iso: string): string {
  const date = new Date(iso);
  return date.toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" });
}

interface HistoryListProps {
  items: HistoryItem[];
  failReasons: Record<string, InsertFailedReason>;
}

export default function HistoryList({ items, failReasons }: HistoryListProps) {
  const [copiedId, setCopiedId] = useState<string | null>(null);

  async function handleCopy(id: string) {
    try {
      await copyHistoryItem(id);
      setCopiedId(id);
      setTimeout(() => setCopiedId((current) => (current === id ? null : current)), 1400);
    } catch {
      // コピー失敗はユーザー操作に影響しない補助機能のため、ここでは状態を変えないだけに留める。
    }
  }

  return (
    <section className={styles.section}>
      <div className={styles.heading}>
        <p className={styles.headingTitle}>直近の結果</p>
      </div>
      {items.length === 0 ? (
        <EmptyState title="まだ結果がありません" description="ホットキーを押して話しかけると、ここに認識結果が並びます。" />
      ) : (
        <div className={styles.list} role="list">
          {items.map((item) => {
            const failReason = failReasons[item.id];
            const copied = copiedId === item.id;
            return (
              <button
                key={item.id}
                type="button"
                role="listitem"
                className={styles.row}
                onClick={() => handleCopy(item.id)}
                title="クリックでコピー"
              >
                <div className={styles.rowBody}>
                  <p className={styles.text}>{item.text}</p>
                  <div className={styles.meta}>
                    <span>{formatTime(item.at)}</span>
                    <span>·</span>
                    <span>{MODE_LABEL[item.mode]}</span>
                    <span>·</span>
                    <span>{item.timings.total_ms}ms</span>
                    {item.flags.includes("llm_skipped") && (
                      <Badge tone="neutral">LLM 省略 (短文)</Badge>
                    )}
                    {item.flags.includes("llm_rejected") && (
                      <Badge tone="warning">LLM 出力を却下 → 生テキスト</Badge>
                    )}
                  </div>
                </div>
                {failReason ? (
                  <Badge tone="danger">{FAIL_REASON_LABEL[failReason]}</Badge>
                ) : item.inserted ? (
                  <Badge tone="success">
                    <Icon name="check" size={10} />
                    挿入済み
                  </Badge>
                ) : (
                  <Badge tone="neutral">未挿入</Badge>
                )}
                <Icon name={copied ? "check" : "copy"} size={14} className={copied ? styles.copied : undefined} />
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
