"""ASR 専用ワーカースレッド (設計書 §4.1: 「専用スレッド1本+キュー」でGPU推論を直列化)。"""

from __future__ import annotations

import concurrent.futures as cf
import logging
import queue
import threading
from dataclasses import dataclass

import numpy as np

from .base import AsrBackend

logger = logging.getLogger(__name__)


@dataclass
class _AsrJob:
    pcm: np.ndarray
    context: str | None
    future: cf.Future[str]


class AsrWorker:
    """バックエンドへの `transcribe` 呼び出しを単一スレッドで直列化する。"""

    def __init__(self, backend: AsrBackend) -> None:
        self._backend = backend
        self._queue: queue.Queue[_AsrJob | None] = queue.Queue()
        self._thread = threading.Thread(target=self._run, name="asr-worker", daemon=True)
        self._thread.start()

    def submit(self, pcm: np.ndarray, context: str | None) -> cf.Future[str]:
        future: cf.Future[str] = cf.Future()
        self._queue.put(_AsrJob(pcm=pcm, context=context, future=future))
        return future

    def _run(self) -> None:
        while True:
            job = self._queue.get()
            if job is None:
                break
            if not job.future.set_running_or_notify_cancel():
                continue
            try:
                text = self._backend.transcribe(job.pcm, job.context)
            except Exception as exc:  # noqa: BLE001 呼び出し元へ確実に伝える
                logger.exception("ASR transcribe に失敗しました")
                job.future.set_exception(exc)
            else:
                job.future.set_result(text)

    def stop(self, timeout: float = 5.0) -> None:
        self._queue.put(None)
        self._thread.join(timeout=timeout)
