"""REST API (設計書 §4.7): /healthz, /v1/info, /v1/modes, /v1/dictionary, /v1/pair。

`/healthz` と `/v1/pair` のみ無認証。他は Bearer 認証必須。
"""

from __future__ import annotations

import asyncio
import json

from fastapi import APIRouter, Depends, HTTPException, Request
from jsonschema import ValidationError
from pydantic import BaseModel, Field
from pydantic import ValidationError as PydanticValidationError

from .auth import extract_bearer_token
from .dictionary import Dictionary
from .protocol_schemas import validate_message
from .tls import fingerprint as compute_fingerprint

router = APIRouter()

AVAILABLE_MODES = ["clean", "raw", "translate_en"]
PROTOCOL_VERSION = 1
SERVER_VERSION = "0.1.0"

_PAIR_INVALID_DETAIL = "pairing code invalid or expired"
_DEVICE_NAME_PATTERN = r"^[A-Za-z0-9_-]{1,64}$"


class PairRequest(BaseModel):
    device: str = Field(pattern=_DEVICE_NAME_PATTERN)
    code: str


async def _read_body_with_limit(request: Request, max_bytes: int) -> bytes:
    """`request` の本文を `max_bytes` を超えないことを確認しながら読む。

    `Content-Length` があれば先に検査して即座に拒否する。無い場合 (chunked 転送等) は
    ストリームで読みながら累積サイズを監視し、超過した時点で打ち切る。
    """
    content_length = request.headers.get("content-length")
    if content_length is not None:
        try:
            declared_length = int(content_length)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail="invalid Content-Length") from exc
        if declared_length > max_bytes:
            raise HTTPException(status_code=413, detail="request body too large")

    total = 0
    chunks: list[bytes] = []
    async for chunk in request.stream():
        total += len(chunk)
        if total > max_bytes:
            raise HTTPException(status_code=413, detail="request body too large")
        chunks.append(chunk)
    return b"".join(chunks)


async def require_token(request: Request) -> str:
    token = extract_bearer_token(request.headers.get("authorization", ""))
    state = request.app.state.voice
    device = state.token_store.verify(token)
    if device is None:
        raise HTTPException(status_code=401, detail="invalid token")
    return device


@router.get("/healthz")
async def healthz(request: Request) -> dict:
    state = request.app.state.voice
    return {"asr": state.asr_ready, "llm": state.llm_ready}


@router.post("/v1/pair")
async def pair(request: Request) -> dict:
    """CLI `voice-server pair` で生成したコードを使ってトークンを発行する (1回きり、無認証)。

    成功/失敗の理由 (コード不一致・期限切れ・ファイル無し) は一律 403 とし区別しない
    (総当たり耐性)。失敗時は `limits.pair_failure_delay_s` だけ待ってから応答する。
    """
    state = request.app.state.voice
    # 専用の上限は設けず、他の小さな JSON ボディと同じ上限 (`max_dictionary_body_bytes`) を流用する。
    max_bytes = state.config.limits.max_dictionary_body_bytes
    raw_body = await _read_body_with_limit(request, max_bytes)

    try:
        payload = json.loads(raw_body)
    except json.JSONDecodeError as exc:
        raise HTTPException(status_code=400, detail="invalid JSON") from exc

    try:
        pair_request = PairRequest.model_validate(payload)
    except PydanticValidationError as exc:
        raise HTTPException(status_code=422, detail="invalid pairing request") from exc

    if not state.pairing_store.verify_and_consume(pair_request.code):
        await asyncio.sleep(state.config.limits.pair_failure_delay_s)
        raise HTTPException(status_code=403, detail=_PAIR_INVALID_DETAIL)

    token = state.token_store.issue(pair_request.device)
    device_fingerprint = compute_fingerprint(state.config.server.tls_cert_path)
    return {"device": pair_request.device, "token": token, "fingerprint": device_fingerprint}


@router.get("/v1/info")
async def info(request: Request, device: str = Depends(require_token)) -> dict:
    state = request.app.state.voice
    return {
        "server_version": SERVER_VERSION,
        "protocol_version": PROTOCOL_VERSION,
        "asr_backend": state.config.asr.backend,
        "asr_model": state.config.asr.model,
    }


@router.get("/v1/modes")
async def modes(device: str = Depends(require_token)) -> dict:
    return {"modes": AVAILABLE_MODES}


@router.get("/v1/dictionary")
async def get_dictionary(request: Request, device: str = Depends(require_token)) -> dict:
    state = request.app.state.voice
    return state.dictionary_store.get().model_dump(mode="json")


@router.put("/v1/dictionary")
async def put_dictionary(request: Request, device: str = Depends(require_token)) -> dict:
    state = request.app.state.voice
    max_bytes = state.config.limits.max_dictionary_body_bytes
    raw_body = await _read_body_with_limit(request, max_bytes)

    try:
        body = json.loads(raw_body)
    except json.JSONDecodeError as exc:
        raise HTTPException(status_code=400, detail="invalid JSON") from exc

    try:
        validate_message("dictionary", body)
    except ValidationError as exc:
        raise HTTPException(status_code=422, detail=exc.message) from exc

    dictionary = Dictionary.model_validate(body)
    state.dictionary_store.save(dictionary)
    return dictionary.model_dump(mode="json")
