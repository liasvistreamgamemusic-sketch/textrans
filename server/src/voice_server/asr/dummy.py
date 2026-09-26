"""単体・結合テスト用の ASR バックエンド。GPU 不要 (設計書 §4.2)。"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from voice_server.config import AsrSection

DEFAULT_FIXED_TEXT = "これはダミー認識結果です"


class DummyAsrBackend:
    name = "dummy"
    supports_context = True

    def __init__(self, fixed_text: str = DEFAULT_FIXED_TEXT) -> None:
        self._fixed_text = fixed_text

    def load(self, cfg: AsrSection) -> None:
        return None

    def warmup(self) -> None:
        return None

    def transcribe(self, pcm: np.ndarray, context: str | None) -> str:
        if pcm.size == 0 or not np.any(pcm):
            return ""
        return self._fixed_text
