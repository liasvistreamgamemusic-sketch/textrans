"""FastAPI アプリケーションファクトリ。依存を組み立て `app.state.voice` に載せる。"""

from __future__ import annotations

import logging
import threading
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass, field

import httpx
from fastapi import FastAPI

from .asr.base import AsrBackend, create_backend
from .asr.worker import AsrWorker
from .auth import TokenStore
from .cleaner import Cleaner
from .config import ServerConfig
from .dictionary import DictionaryStore
from .llm_client import LlmClient
from .logging_utils import setup_logging
from .pairing import PairingStore
from .rest import router as rest_router
from .vad import SileroVadClassifier, VadClassifier
from .ws import router as ws_router

logger = logging.getLogger(__name__)


@dataclass
class InFlightGate:
    """発話単位の同時処理数を制限する (設計書 §3.3: busy)。

    設計書 §3.3 は「別セッションが処理中はキューに入れる。キュー長は2まで」と書いており、
    処理中の1件 + キューで待てる `limits.queue_size` 件が同時に受理できる上限になる。
    そのため `capacity` は `queue_size + 1` を渡す (呼び出し側の責務)。
    """

    capacity: int
    _count: int = field(default=0, init=False)
    _lock: threading.Lock = field(default_factory=threading.Lock, init=False)

    def try_acquire(self) -> bool:
        with self._lock:
            if self._count >= self.capacity:
                return False
            self._count += 1
            return True

    def release(self) -> None:
        with self._lock:
            self._count = max(0, self._count - 1)


@dataclass
class AppState:
    config: ServerConfig
    token_store: TokenStore
    dictionary_store: DictionaryStore
    pairing_store: PairingStore
    asr_worker: AsrWorker
    vad_classifier: VadClassifier
    cleaner: Cleaner
    in_flight: InFlightGate
    asr_ready: bool = False
    llm_ready: bool = False


def create_app(
    config: ServerConfig,
    *,
    vad_classifier: VadClassifier | None = None,
    asr_backend: AsrBackend | None = None,
) -> FastAPI:
    """`app.state.voice` に依存一式を積んだ FastAPI アプリを作る。

    `vad_classifier` / `asr_backend` はテストでのスタブ差し替え用。
    """
    setup_logging(config.logging.level)

    backend = asr_backend if asr_backend is not None else create_backend(config.asr.backend)
    asr_worker = AsrWorker(backend)

    http_client = httpx.AsyncClient()
    llm_client = LlmClient(config.llm.base_url, config.llm.model, http_client=http_client)
    cleaner = Cleaner(llm_client, config.llm)

    state = AppState(
        config=config,
        token_store=TokenStore(config.tokens.path),
        dictionary_store=DictionaryStore(config.dictionary.path),
        pairing_store=PairingStore(config.pairing.path),
        asr_worker=asr_worker,
        vad_classifier=vad_classifier or SileroVadClassifier(),
        cleaner=cleaner,
        # 処理中1件 + キュー queue_size 件 = 同時受理数 queue_size + 1 (設計書 §3.3)。
        in_flight=InFlightGate(config.limits.queue_size + 1),
    )

    @asynccontextmanager
    async def lifespan(_: FastAPI) -> AsyncIterator[None]:
        try:
            backend.load(config.asr)
            backend.warmup()
            state.asr_ready = True
        except Exception:  # noqa: BLE001 起動は止めず /healthz で報告する
            logger.exception("ASR backend の初期化に失敗しました")
            state.asr_ready = False

        try:
            await llm_client.check_ready()
            state.llm_ready = True
        except Exception:  # noqa: BLE001 llama-server 未起動でも Gateway は起動する
            logger.warning("llama-server に接続できません。/healthz は llm=false を返します")
            state.llm_ready = False

        try:
            yield
        finally:
            asr_worker.stop()
            await http_client.aclose()

    app = FastAPI(title="voice-server", lifespan=lifespan)
    app.state.voice = state
    app.include_router(rest_router)
    app.include_router(ws_router)
    return app
