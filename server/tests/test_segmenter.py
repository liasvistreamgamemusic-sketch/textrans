from __future__ import annotations

import numpy as np

from voice_server.segmenter import Segmenter

SAMPLE_RATE = 16000


def _chunk(ms: int) -> np.ndarray:
    n = int(SAMPLE_RATE * ms / 1000)
    return np.ones(n, dtype=np.int16)


def test_silence_cuts_segment_after_threshold() -> None:
    seg = Segmenter(sample_rate=SAMPLE_RATE, silence_ms=600, max_segment_s=15, min_segment_s=1)
    segments = []
    segments += seg.push(_chunk(1500), is_speech=True)  # 1.5s speech (>= min_segment_s)
    segments += seg.push(_chunk(300), is_speech=False)
    segments += seg.push(_chunk(300), is_speech=False)  # 累計600ms無音 -> 区切り

    assert len(segments) == 1
    assert segments[0].sample_count == _chunk(1500 + 300 + 300).size


def test_short_segment_merges_with_next() -> None:
    seg = Segmenter(sample_rate=SAMPLE_RATE, silence_ms=600, max_segment_s=15, min_segment_s=1)
    segments = []
    segments += seg.push(_chunk(300), is_speech=True)  # 0.3s (< min_segment_s)
    segments += seg.push(_chunk(600), is_speech=False)  # 無音600ms検出だが短すぎるため結合継続
    assert segments == []

    segments += seg.push(_chunk(1000), is_speech=True)
    segments += seg.push(_chunk(600), is_speech=False)
    assert len(segments) == 1
    # 結合された全チャンクの長さになっている
    assert segments[0].sample_count == _chunk(300 + 600 + 1000 + 600).size


def test_max_segment_forces_cut_even_without_silence() -> None:
    seg = Segmenter(sample_rate=SAMPLE_RATE, silence_ms=600, max_segment_s=1, min_segment_s=1)
    segments = seg.push(_chunk(1000), is_speech=True)
    assert len(segments) == 1


def test_flush_returns_remaining_buffer() -> None:
    seg = Segmenter(sample_rate=SAMPLE_RATE, silence_ms=600, max_segment_s=15, min_segment_s=1)
    seg.push(_chunk(200), is_speech=True)
    segment = seg.flush()
    assert segment is not None
    assert segment.sample_count == _chunk(200).size


def test_flush_returns_none_when_empty() -> None:
    seg = Segmenter(sample_rate=SAMPLE_RATE, silence_ms=600, max_segment_s=15, min_segment_s=1)
    assert seg.flush() is None
