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
        self._cfg: AsrSection | None = None

    def load(self, cfg: AsrSection) -> None:
        from faster_whisper import WhisperModel  # 遅延 import (extra: gpu)

        model_ref = str(cfg.model_path) if cfg.model_path else cfg.model
        self._model = WhisperModel(model_ref, device=cfg.device, compute_type=cfg.compute_type)
        self._cfg = cfg

    def _run(self, audio: np.ndarray, context: str | None) -> str:
        if self._model is None or self._cfg is None:
            raise RuntimeError("load() が呼ばれていません")
        threshold = self._cfg.no_speech_threshold
        segments, _ = self._model.transcribe(
            audio,
            language="ja",
            initial_prompt=context or None,
            vad_filter=self._cfg.vad_filter,
            no_speech_threshold=threshold,
        )
        # faster-whisper 内部のガードは no_speech_prob > 閾値 かつ avg_logprob < log_prob_threshold の
        # ときしかセグメントを捨てないため、無音に対して自信ありげに出る「ご視聴ありがとうございました」等の
        # 定型句が通ってしまう。ここでは no_speech_prob だけで捨てる (設計書 §4.3)。
        return "".join(segment.text for segment in segments if segment.no_speech_prob <= threshold).strip()

    def warmup(self) -> None:
        # vad_filter 有効時は内蔵 VAD (onnx) のロードもここで済ませる。
        self._run(np.zeros(16000, dtype=np.float32), None)

    def transcribe(self, pcm: np.ndarray, context: str | None) -> str:
        return self._run(pcm.astype(np.float32) / 32768.0, context)
