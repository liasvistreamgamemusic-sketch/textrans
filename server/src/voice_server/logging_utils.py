"""ロギング設定。

設計書 §7.1: 「処理時間、モデル名、フラグを記録する。本文は既定で記録しない」。
このモジュール自体は書式のみを扱う。本文 (音声認識結果・清書結果) をログに渡さないことは
呼び出し側の責務であり、コードレビューで確認する。
"""

from __future__ import annotations

import logging


def setup_logging(level: str = "INFO") -> None:
    logging.basicConfig(
        level=getattr(logging, level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
