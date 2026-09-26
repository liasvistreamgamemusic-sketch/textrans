"""faster-whisper (CTranslate2) バックエンド。初版の既定 (設計書 §4.2, §6.1)。

`faster-whisper` は extra `gpu` に分離されているため import はメソッド内で遅延する。
mac の開発機ではこのモジュールの import 自体は安全だが、`load()` は失敗する。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from voice_server.config import AsrSection


class FasterWhisperBackend:
    name = "faster_whisper"
    supports_context = True

    def __init__(self) -> None:
        self._model = None

    def load(self, cfg: AsrSection) -> None:
        from faster_whisper import WhisperModel  # 遅延 import (extra: gpu)

        model_ref = str(cfg.model_path) if cfg.model_path else cfg.model
        self._model = WhisperModel(model_ref, device=cfg.device, compute_type=cfg.compute_type)

    def warmup(self) -> None:
        if self._model is None:
            raise RuntimeError("load() が呼ばれていません")
        silence = np.zeros(16000, dtype=np.float32)
        segments, _ = self._model.transcribe(silence, language="ja")
        list(segments)

    def transcribe(self, pcm: np.ndarray, context: str | None) -> str:
        if self._model is None:
            raise RuntimeError("load() が呼ばれていません")
        audio = pcm.astype(np.float32) / 32768.0
        segments, _ = self._model.transcribe(audio, language="ja", initial_prompt=context or None)
        return "".join(segment.text for segment in segments).strip()
