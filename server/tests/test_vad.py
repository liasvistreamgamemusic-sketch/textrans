from __future__ import annotations

import numpy as np

from voice_server.vad import split_windows

from .conftest import StubVadClassifier

SAMPLE_RATE = 16000


def test_split_windows_splits_evenly_and_keeps_remainder() -> None:
    buffer = np.arange(1000, dtype=np.float32)
    windows, remainder = split_windows(buffer, window=300)
    assert len(windows) == 3
    assert all(len(w) == 300 for w in windows)
    assert len(remainder) == 100


def test_new_session_returns_independent_sessions() -> None:
    classifier = StubVadClassifier(speech=True)
    session_a = classifier.new_session()
    session_b = classifier.new_session()
    assert session_a is not session_b
    assert session_a.calls == []
    assert session_b.calls == []


def test_concurrent_sessions_do_not_share_state() -> None:
    """2つの発話 (Utterance) が交互に push しても互いの状態を壊さないこと。"""
    classifier = StubVadClassifier(speech=True)
    session_a = classifier.new_session()
    session_b = classifier.new_session()

    # 交互に呼び出す (実際の Utterance が交互に push_audio するのを模す)
    session_a.is_speech(np.zeros(160, dtype=np.int16), SAMPLE_RATE)
    session_b.is_speech(np.zeros(320, dtype=np.int16), SAMPLE_RATE)
    session_a.is_speech(np.zeros(160, dtype=np.int16), SAMPLE_RATE)
    session_b.is_speech(np.zeros(320, dtype=np.int16), SAMPLE_RATE)

    assert session_a.calls == [160, 160]
    assert session_b.calls == [320, 320]
