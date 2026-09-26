import { useEffect, useRef, useState } from "react";
import {
  getHistory,
  getStatus,
  onInsertFailed,
  onLevel,
  onResult,
  onStatus,
} from "../api";
import type { HistoryItem, InsertFailedReason, Status } from "../types";

const HISTORY_LIMIT = 20;

export interface VoiceState {
  status: Status | null;
  history: HistoryItem[];
  level: number;
  failReasons: Record<string, InsertFailedReason>;
}

// `voice://status` / `voice://level` / `voice://result` / `voice://insert_failed` を
// まとめて購読し、Home 画面 (と接続状態を表示するヘッダー) が使う最新状態を保持する。
export function useVoiceState(): VoiceState {
  const [status, setStatus] = useState<Status | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [level, setLevel] = useState(0);
  const [failReasons, setFailReasons] = useState<Record<string, InsertFailedReason>>({});
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    getStatus()
      .then((s) => mounted.current && setStatus(s))
      .catch(() => undefined);
    getHistory()
      .then((items) => mounted.current && setHistory(items.slice(0, HISTORY_LIMIT)))
      .catch(() => undefined);

    const unlistenPromises = [
      onStatus((s) => mounted.current && setStatus(s)),
      onLevel((l) => mounted.current && setLevel(l.rms)),
      onResult((item) =>
        mounted.current &&
        setHistory((prev) => [item, ...prev].slice(0, HISTORY_LIMIT)),
      ),
      onInsertFailed(
        (e) =>
          mounted.current &&
          setFailReasons((prev) => ({ ...prev, [e.id]: e.reason })),
      ),
    ];

    return () => {
      mounted.current = false;
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()).catch(() => undefined));
    };
  }, []);

  return { status, history, level, failReasons };
}
