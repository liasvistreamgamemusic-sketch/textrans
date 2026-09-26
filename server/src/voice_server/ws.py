"""`/v1/dictate` WebSocket エンドポイント (設計書 §3)。"""

from __future__ import annotations

import asyncio
import json
import logging
import time

import numpy as np
from fastapi import APIRouter, Depends, HTTPException
from jsonschema import ValidationError
from starlette.websockets import WebSocket, WebSocketDisconnect

from .auth import extract_bearer_token
from .dictionary import select_glossary_terms
from .protocol_schemas import validate_message
from .session import Utterance

logger = logging.getLogger(__name__)
router = APIRouter()

CURRENT_PROTOCOL_VERSION = 1
# 「現行と1つ前のバージョンを受け付ける」(protocol/README.md)。現行が1のため今は1つのみ。
SUPPORTED_PROTOCOL_VERSIONS = {CURRENT_PROTOCOL_VERSION}


async def authenticate_websocket(websocket: WebSocket) -> str:
    token = extract_bearer_token(websocket.headers.get("authorization", ""))
    state = websocket.app.state.voice
    device = state.token_store.verify(token)
    if device is None:
        raise HTTPException(status_code=401, detail="invalid token")
    return device


class ConnectionContext:
    """1つの WebSocket 接続の状態: 現在アクティブな発話と、完了待ちの発話。"""

    def __init__(self, websocket: WebSocket, state) -> None:
        self.websocket = websocket
        self.state = state
        self.active: Utterance | None = None
        self.pending: dict[str, Utterance] = {}
        self._send_lock = asyncio.Lock()

    async def send(self, payload: dict) -> None:
        async with self._send_lock:
            await self.websocket.send_json(payload)

    async def send_error(self, session_id: str | None, code: str, message: str, retryable: bool) -> None:
        await self.send(
            {
                "type": "error",
                "session_id": session_id,
                "code": code,
                "message": message,
                "retryable": retryable,
            }
        )

    def release(self, session_id: str) -> None:
        """`pending` から実際に取り除けたときだけ `in_flight` を解放する。

        `end` 直後に `cancel` が来ると、`_finalize` タスクと `_handle_cancel` の両方が
        同じ発話に対して `release()` を呼ぶことがある (end→finalize起動→cancel の順で
        finalize がまだ完了していない場合)。二重に `in_flight.release()` すると
        カウントが実際の同時処理数より少なくなり、`busy` が正しく発生しなくなる。
        """
        if self.pending.pop(session_id, None) is not None:
            self.state.in_flight.release()

    async def cleanup(self) -> None:
        for utterance in list(self.pending.values()):
            utterance.cancel()
            self.state.in_flight.release()
        self.pending.clear()
        self.active = None


@router.websocket("/v1/dictate")
async def dictate(websocket: WebSocket, device: str = Depends(authenticate_websocket)) -> None:
    await websocket.accept()
    state = websocket.app.state.voice
    conn = ConnectionContext(websocket, state)
    logger.info("接続確立 (device=%s)", device)
    try:
        while True:
            message = await websocket.receive()
            if message.get("type") == "websocket.disconnect":
                break
            text = message.get("text")
            data = message.get("bytes")
            if text is not None:
                await _handle_text(conn, text)
            elif data is not None:
                await _handle_binary(conn, data)
    except WebSocketDisconnect:
        pass
    finally:
        await conn.cleanup()
        logger.info("接続終了 (device=%s)", device)


async def _handle_text(conn: ConnectionContext, raw: str) -> None:
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        await conn.send_error(None, "invalid_message", "JSONとして解析できません", False)
        return
    if not isinstance(payload, dict):
        message = "メッセージはJSONオブジェクトである必要があります"
        await conn.send_error(None, "invalid_message", message, False)
        return

    msg_type = payload.get("type")
    if msg_type == "start":
        await _handle_start(conn, payload)
    elif msg_type == "end":
        await _handle_end(conn, payload)
    elif msg_type == "cancel":
        await _handle_cancel(conn, payload)
    else:
        session_id = payload.get("session_id")
        await conn.send_error(session_id, "invalid_message", f"未知の type: {msg_type!r}", False)


async def _handle_start(conn: ConnectionContext, payload: dict) -> None:
    try:
        validate_message("start", payload)
    except ValidationError as exc:
        await conn.send_error(payload.get("session_id"), "invalid_message", exc.message, False)
        return

    session_id = payload["session_id"]
    if conn.active is not None:
        await conn.send_error(session_id, "invalid_message", "既に発話中です", False)
        return

    version = payload["protocol_version"]
    if version not in SUPPORTED_PROTOCOL_VERSIONS:
        message = f"未対応の protocol_version: {version}"
        await conn.send_error(session_id, "unsupported_version", message, False)
        await conn.websocket.close(code=1008)
        return

    if not conn.state.in_flight.try_acquire():
        await conn.send_error(session_id, "busy", "処理中の発話が上限に達しています", True)
        return

    async def on_partial(seq: int, seg_text: str) -> None:
        await conn.send({"type": "partial", "session_id": session_id, "seq": seq, "text": seg_text})

    utterance = Utterance(session_id, payload["mode"], payload["sample_rate"], conn.state, on_partial)
    conn.active = utterance
    conn.pending[session_id] = utterance
    await conn.send({"type": "ready", "session_id": session_id})


async def _handle_binary(conn: ConnectionContext, data: bytes) -> None:
    utterance = conn.active
    if utterance is None:
        await conn.send_error(None, "invalid_message", "start前またはend後の音声フレームです", False)
        return

    if len(data) % 2 != 0:
        message = "PCM16フレームのバイト数が奇数です"
        await conn.send_error(utterance.session_id, "invalid_message", message, False)
        return

    pcm = np.frombuffer(data, dtype="<i2")
    max_samples = conn.state.config.limits.max_utterance_s * utterance.sample_rate
    if utterance.total_samples + len(pcm) > max_samples:
        await conn.send_error(utterance.session_id, "too_long", "発話が上限時間を超えました", False)
        conn.active = None
        utterance.cancel()
        conn.release(utterance.session_id)
        return

    try:
        await utterance.push_audio(pcm)
    except Exception:  # noqa: BLE001 VAD/セグメンタの失敗で接続ごと落とさず error(internal) を返す
        logger.exception("音声フレームの処理に失敗しました (session_id=%s)", utterance.session_id)
        await conn.send_error(utterance.session_id, "internal", "内部エラーが発生しました", False)
        conn.active = None
        utterance.cancel()
        conn.release(utterance.session_id)


async def _handle_end(conn: ConnectionContext, payload: dict) -> None:
    try:
        validate_message("end", payload)
    except ValidationError as exc:
        await conn.send_error(payload.get("session_id"), "invalid_message", exc.message, False)
        return

    session_id = payload["session_id"]
    utterance = conn.pending.get(session_id)
    if utterance is None or utterance is not conn.active:
        await conn.send_error(session_id, "unknown_session", "不明または終了済みのセッションです", False)
        return

    conn.active = None
    asyncio.create_task(_finalize(conn, utterance))


async def _handle_cancel(conn: ConnectionContext, payload: dict) -> None:
    try:
        validate_message("cancel", payload)
    except ValidationError:
        return  # 設計: cancel は不正でも何も返さない (§3.2)

    session_id = payload.get("session_id")
    utterance = conn.pending.get(session_id)
    if utterance is None:
        return
    if conn.active is utterance:
        conn.active = None
    utterance.cancel()
    conn.release(session_id)


async def _finalize(conn: ConnectionContext, utterance: Utterance) -> None:
    try:
        await utterance.finish()
        finished = await utterance.wait_done(timeout=conn.state.config.limits.end_grace_s)
        if utterance.cancelled:
            return

        if not finished:
            logger.warning(
                "end_grace_s を超過したため空の final を返します (session_id=%s)",
                utterance.session_id,
            )
            raw_text = ""
        else:
            raw_text = utterance.build_raw_text()

        terms = conn.state.dictionary_store.get().terms
        glossary_terms = select_glossary_terms(raw_text, terms)
        glossary_text = ", ".join(term.surface for term in glossary_terms)

        clean_result = await conn.state.cleaner.clean(raw_text, utterance.mode, glossary_text)

        total_ms = int((time.monotonic() - (utterance.end_received_at or time.monotonic())) * 1000)
        await conn.send(
            {
                "type": "final",
                "session_id": utterance.session_id,
                "raw_text": raw_text,
                "text": clean_result.text,
                "mode": utterance.mode,
                "flags": clean_result.flags,
                "timings": {
                    "asr_ms": utterance.asr_elapsed_ms,
                    "llm_ms": clean_result.llm_ms,
                    "total_ms": total_ms,
                },
            }
        )
    except Exception:  # noqa: BLE001 内部エラーを error(code=internal) として通知する
        logger.exception("発話の最終処理に失敗しました (session_id=%s)", utterance.session_id)
        await conn.send_error(utterance.session_id, "internal", "内部エラーが発生しました", False)
    finally:
        conn.release(utterance.session_id)
