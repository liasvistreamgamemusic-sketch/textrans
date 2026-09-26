from __future__ import annotations

import numpy as np
import pytest

from voice_server.asr.dummy import DummyAsrBackend
from voice_server.config import ServerConfig
from voice_server.vad import VadClassifier


class StubVadSession:
    """1発話分の状態を持つテスト用 VAD セッション。"""

    def __init__(self, speech: bool) -> None:
        self.speech = speech
        self.calls: list[int] = []

    def is_speech(self, pcm: np.ndarray, sample_rate: int) -> bool:
        self.calls.append(len(pcm))
        return self.speech


class StubVadClassifier:
    """全チャンクを固定の speech/silence 判定にするテスト用 VAD (共有リソース)。

    `new_session()` を呼ぶたびに独立した `StubVadSession` を返す。他の発話の状態を
    壊さないことをテストで確認できるよう、発行済みセッションを `sessions` に記録する。
    """

    def __init__(self, speech: bool = True) -> None:
        self.speech = speech
        self.sessions: list[StubVadSession] = []

    def new_session(self) -> StubVadSession:
        session = StubVadSession(self.speech)
        self.sessions.append(session)
        return session


def make_test_config(tmp_path, **overrides) -> ServerConfig:
    cfg = ServerConfig(
        tokens={"path": tmp_path / "tokens.yaml"},
        dictionary={"path": tmp_path / "dictionary.yaml"},
        asr={"backend": "dummy"},
        **overrides,
    )
    return cfg


@pytest.fixture
def dummy_backend() -> DummyAsrBackend:
    return DummyAsrBackend()


@pytest.fixture
def stub_vad() -> VadClassifier:
    return StubVadClassifier(speech=True)
