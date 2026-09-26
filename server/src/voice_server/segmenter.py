"""VAD の判定 (is_speech) を受け取ってセグメントに区切る (設計書 §4.1, §4.3)。

VAD 自体の実装 (Silero 等) には依存しない。呼び出し側が各チャンクの
`is_speech` を判定して渡す (テストではスタブ化できる)。
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass
class Segment:
    pcm: np.ndarray

    @property
    def sample_count(self) -> int:
        return len(self.pcm)


class Segmenter:
    """無音 `silence_ms` 以上、または `max_segment_s` 到達で区切る。

    `min_segment_s` 未満のセグメントは単独では確定させず、次のセグメントと結合する。
    """

    def __init__(self, sample_rate: int, silence_ms: int, max_segment_s: int, min_segment_s: int) -> None:
        self.sample_rate = sample_rate
        self.silence_ms = silence_ms
        self.max_segment_s = max_segment_s
        self.min_segment_s = min_segment_s
        self._chunks: list[np.ndarray] = []
        self._total_samples = 0
        self._silence_run_ms = 0.0

    def _total_ms(self) -> float:
        return self._total_samples / self.sample_rate * 1000

    def _reset(self) -> None:
        self._chunks = []
        self._total_samples = 0
        self._silence_run_ms = 0.0

    def _emit(self) -> Segment:
        pcm = np.concatenate(self._chunks) if self._chunks else np.array([], dtype=np.int16)
        self._reset()
        return Segment(pcm=pcm)

    def push(self, chunk: np.ndarray, is_speech: bool) -> list[Segment]:
        """PCM チャンクを1つ積み、確定したセグメントを返す (0個の場合もある)。"""
        if len(chunk) == 0:
            return []

        chunk_ms = len(chunk) / self.sample_rate * 1000
        self._chunks.append(chunk)
        self._total_samples += len(chunk)

        if is_speech:
            self._silence_run_ms = 0.0
        else:
            self._silence_run_ms += chunk_ms

        total_ms = self._total_ms()
        reached_max = total_ms >= self.max_segment_s * 1000
        reached_silence_cut = self._silence_run_ms >= self.silence_ms

        if not (reached_max or reached_silence_cut):
            return []

        if reached_max or total_ms >= self.min_segment_s * 1000:
            return [self._emit()]

        # 無音区切りは検出したが、まだ min_segment_s 未満: 区切らず次と結合する。
        self._silence_run_ms = 0.0
        return []

    def flush(self) -> Segment | None:
        """発話終了時に残りをセグメントとして返す (空なら None)。"""
        if self._total_samples == 0:
            return None
        return self._emit()
