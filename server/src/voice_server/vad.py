"""VAD (無音検出) のバックエンド抽象。

`silero-vad` は torch(CPU) 依存があるため extra `vad` に分離し、import は遅延する。
テスト (mac / GPU なし) では `VadClassifier` を満たすスタブに差し替える。

モデル本体 (重み) は発話間で共有し、`_pending` (端数バッファ) と内部の再帰状態は
`new_session()` が返す `VadSession` に分離して発話ごとに独立させる。以前は共有インスタンス
1つが `_pending` を持っており、複数発話が同時に音声を送ると互いのバッファを破壊していた。
"""

from __future__ import annotations

from typing import Protocol

import numpy as np

# Silero VAD のモデルが受け付ける固定窓長 (実機で 1600 サンプルを渡して ValueError になった回帰)。
SILERO_WINDOW_SAMPLES: dict[int, int] = {16000: 512, 8000: 256}


class VadSession(Protocol):
    """1発話に閉じた VAD の状態 (端数バッファ・モデルの内部状態)。"""

    def is_speech(self, pcm: np.ndarray, sample_rate: int) -> bool: ...


class VadClassifier(Protocol):
    """VAD の共有リソース (モデル重み)。発話ごとに `new_session()` で状態を分離する。"""

    def new_session(self) -> VadSession: ...


def split_windows(buffer: np.ndarray, window: int) -> tuple[list[np.ndarray], np.ndarray]:
    """`buffer` を `window` サンプルずつに切り、端数を残りとして返す。"""
    count = len(buffer) // window
    windows = [buffer[i * window : (i + 1) * window] for i in range(count)]
    return windows, buffer[count * window :]


class SileroVadClassifier:
    """Silero VAD (CPU) の共有モデル。重みは初回 `new_session()` で1度だけロードする。

    既知の制約 (⚠️ assumed): Silero のモデル本体は呼び出し間で再帰状態を持つため、
    複数発話が真に同時に音声を送っている間は、モデル内部の状態を相互に汚染しうる
    (`_pending` バッファのような発話固有の状態は `SileroVadSession` に分離済みだが、
    モデル自体のインスタンスは共有している)。設計書 §1 の想定 (利用者1人、同時発話はまれ)
    の範囲では実害が小さいと判断し、セッション開始時に `reset_states()` を呼ぶ対応に留める。
    同時発話が常態化する場合は、セッションごとにモデルを複製するか、モデル呼び出し自体を
    直列化する対応が必要になる。
    """

    def __init__(self, threshold: float = 0.5) -> None:
        self._threshold = threshold
        self._model = None

    def _ensure_loaded(self):
        if self._model is None:
            from silero_vad import load_silero_vad  # 遅延 import (extra: vad)

            self._model = load_silero_vad()
        return self._model

    def new_session(self) -> SileroVadSession:
        model = self._ensure_loaded()
        model.reset_states()
        return SileroVadSession(model, self._threshold)


class SileroVadSession:
    """1発話分の VAD 状態 (端数バッファ)。モデル本体は `SileroVadClassifier` と共有する。"""

    def __init__(self, model, threshold: float) -> None:
        self._model = model
        self._threshold = threshold
        self._pending = np.empty(0, dtype=np.float32)

    def is_speech(self, pcm: np.ndarray, sample_rate: int) -> bool:
        import torch  # 遅延 import (extra: vad)

        window = SILERO_WINDOW_SAMPLES.get(sample_rate)
        if window is None:
            raise ValueError(f"Silero VAD が対応しないサンプルレートです: {sample_rate}")
        audio = pcm.astype(np.float32) / 32768.0
        windows, self._pending = split_windows(np.concatenate([self._pending, audio]), window)
        speech = False
        for chunk in windows:
            probability = float(self._model(torch.from_numpy(chunk), sample_rate).item())
            speech = speech or probability >= self._threshold
        return speech
