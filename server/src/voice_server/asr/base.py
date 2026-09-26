"""ASR バックエンド抽象 (設計書 §4.2)。"""

from __future__ import annotations

from typing import TYPE_CHECKING, Protocol

import numpy as np

if TYPE_CHECKING:
    from voice_server.config import AsrSection


class AsrBackend(Protocol):
    name: str
    supports_context: bool

    def load(self, cfg: AsrSection) -> None: ...

    def warmup(self) -> None: ...

    def transcribe(self, pcm: np.ndarray, context: str | None) -> str: ...


def create_backend(name: str) -> AsrBackend:
    """設定 `asr.backend` からバックエンドのインスタンスを作る。"""
    if name == "dummy":
        from .dummy import DummyAsrBackend

        return DummyAsrBackend()
    if name == "faster_whisper":
        from .faster_whisper_backend import FasterWhisperBackend

        return FasterWhisperBackend()
    if name in ("qwen3_asr", "cohere_transcribe"):
        raise NotImplementedError(
            f"ASR backend '{name}' は bench 用の比較候補であり未実装です (設計書 §6.1)。"
        )
    raise ValueError(f"未対応の ASR backend: {name!r}")
