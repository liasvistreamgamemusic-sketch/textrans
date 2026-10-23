"""faster-whisper バックエンドの無音幻覚フィルタ (設計書 §4.3)。

faster-whisper 本体は extra `gpu` で mac には入っていないため、`sys.modules` にフェイクの
`faster_whisper` モジュールを差し込んで `load()` 以降の挙動だけを検証する。
"""

from __future__ import annotations

import sys
import types
from dataclasses import dataclass

import numpy as np
import pytest

from voice_server.asr.faster_whisper_backend import FasterWhisperBackend
from voice_server.config import AsrSection


@dataclass
class _FakeSegment:
    text: str
    no_speech_prob: float


class _FakeWhisperModel:
    def __init__(self, *args, **kwargs) -> None:
        self.calls: list[dict] = []
        self.segments: list[_FakeSegment] = []

    def transcribe(self, audio, **kwargs):
        self.calls.append(kwargs)
        return iter(self.segments), None


@pytest.fixture
def backend(monkeypatch) -> tuple[FasterWhisperBackend, _FakeWhisperModel]:
    module = types.ModuleType("faster_whisper")
    module.WhisperModel = _FakeWhisperModel
    monkeypatch.setitem(sys.modules, "faster_whisper", module)
    be = FasterWhisperBackend()
    be.load(AsrSection(backend="faster_whisper", vad_filter=True, no_speech_threshold=0.6))
    assert isinstance(be._model, _FakeWhisperModel)
    return be, be._model


def test_transcribe_passes_vad_filter_and_threshold(backend) -> None:
    be, model = backend
    model.segments = [_FakeSegment("こんにちは", 0.1)]
    assert be.transcribe(np.ones(16000, dtype=np.int16), "文脈") == "こんにちは"
    assert model.calls == [
        {"language": "ja", "initial_prompt": "文脈", "vad_filter": True, "no_speech_threshold": 0.6}
    ]


def test_transcribe_drops_segments_above_no_speech_threshold(backend) -> None:
    be, model = backend
    model.segments = [
        _FakeSegment("こんにちは", 0.2),
        _FakeSegment("ご視聴ありがとうございました", 0.9),
    ]
    assert be.transcribe(np.ones(16000, dtype=np.int16), None) == "こんにちは"


def test_transcribe_returns_empty_when_all_segments_dropped(backend) -> None:
    be, model = backend
    model.segments = [_FakeSegment("おやすみなさい", 0.95)]
    assert be.transcribe(np.ones(16000, dtype=np.int16), None) == ""


def test_transcribe_before_load_raises() -> None:
    with pytest.raises(RuntimeError):
        FasterWhisperBackend().transcribe(np.ones(16000, dtype=np.int16), None)
