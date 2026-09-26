"""1発話 (start〜end/cancel) の状態管理 (設計書 §3, §4.3)。"""

from __future__ import annotations

import asyncio
import logging
import time
from collections.abc import Awaitable, Callable
from typing import TYPE_CHECKING

import numpy as np

from .dictionary import apply_fixed_replacements, build_asr_context
from .segmenter import Segmenter
from .text_rules import detect_repetition

if TYPE_CHECKING:
    from .app import AppState

logger = logging.getLogger(__name__)

_END_OF_UTTERANCE = object()

OnPartial = Callable[[int, str], Awaitable[None]]


class Utterance:
    """1回の `start`〜`end` (または `cancel`) に対応する状態。"""

    def __init__(
        self,
        session_id: str,
        mode: str,
        sample_rate: int,
        state: AppState,
        on_partial: OnPartial,
    ) -> None:
        self.session_id = session_id
        self.mode = mode
        self.sample_rate = sample_rate
        self.cancelled = False
        self.seq = 0
        self.total_samples = 0
        self.asr_elapsed_ms = 0
        self.end_received_at: float | None = None

        self._state = state
        self._on_partial = on_partial
        cfg = state.config
        self._segmenter = Segmenter(
            sample_rate=sample_rate,
            silence_ms=cfg.vad.silence_ms,
            max_segment_s=cfg.vad.max_segment_s,
            min_segment_s=cfg.vad.min_segment_s,
        )
        self._context_chars = cfg.asr.context_chars
        self._prev_tail = ""
        self._raw_parts: list[str] = []
        self._queue: asyncio.Queue = asyncio.Queue()
        self._done = asyncio.Event()
        # VAD の状態 (端数バッファ・モデルの内部状態) は発話ごとに分離する。
        self._vad_session = state.vad_classifier.new_session()
        self._consumer_task = asyncio.create_task(self._consume())

    async def push_audio(self, pcm: np.ndarray) -> None:
        self.total_samples += len(pcm)
        is_speech = self._vad_session.is_speech(pcm, self.sample_rate)
        for segment in self._segmenter.push(pcm, is_speech):
            await self._queue.put(segment.pcm)

    async def finish(self) -> None:
        self.end_received_at = time.monotonic()
        tail = self._segmenter.flush()
        if tail is not None:
            await self._queue.put(tail.pcm)
        await self._queue.put(_END_OF_UTTERANCE)

    async def wait_done(self, timeout: float) -> bool:
        try:
            await asyncio.wait_for(self._done.wait(), timeout=timeout)
        except TimeoutError:
            return False
        return True

    def cancel(self) -> None:
        self.cancelled = True
        self._consumer_task.cancel()

    def build_raw_text(self) -> str:
        terms = self._state.dictionary_store.get().terms
        combined = "".join(self._raw_parts)
        return apply_fixed_replacements(combined, terms)

    async def _consume(self) -> None:
        try:
            while True:
                item = await self._queue.get()
                if item is _END_OF_UTTERANCE:
                    break
                if self.cancelled:
                    continue
                text = await self._transcribe(item)
                if text:
                    self._raw_parts.append(text)
                    self._prev_tail = text[-self._context_chars :] if self._context_chars > 0 else ""
                await self._on_partial(self.seq, text)
                self.seq += 1
        except asyncio.CancelledError:
            pass
        finally:
            self._done.set()

    async def _transcribe(self, pcm: np.ndarray) -> str:
        # 辞書文脈はチャンク単位ではなく、セグメント確定時 (ASR投入直前) だけ再構築する。
        terms = self._state.dictionary_store.get().terms
        dict_context = build_asr_context(terms)
        context_parts = [part for part in (dict_context, self._prev_tail) if part]
        context = " ".join(context_parts) or None

        started = time.monotonic()
        text = await asyncio.wrap_future(self._state.asr_worker.submit(pcm, context))
        if detect_repetition(text):
            logger.info("反復出力を検出。文脈なしで再認識します (session_id=%s)", self.session_id)
            text = await asyncio.wrap_future(self._state.asr_worker.submit(pcm, None))
        self.asr_elapsed_ms += int((time.monotonic() - started) * 1000)
        return text
