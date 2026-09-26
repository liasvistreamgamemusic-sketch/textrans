"""`_read_body_with_limit` の単体テスト (rest.py): Content-Length の有無どちらの経路も検証する。"""

from __future__ import annotations

import pytest
from fastapi import HTTPException
from starlette.requests import Request

from voice_server.rest import _read_body_with_limit


def _streaming_request(chunks: list[bytes], *, headers: list[tuple[bytes, bytes]] | None = None) -> Request:
    """Content-Length ヘッダーの無い chunked 転送を模した `Request` を作る。"""
    scope = {"type": "http", "headers": headers or []}
    events = [
        {"type": "http.request", "body": chunk, "more_body": i < len(chunks) - 1}
        for i, chunk in enumerate(chunks)
    ]
    state = {"i": 0}

    async def receive() -> dict:
        i = state["i"]
        state["i"] += 1
        return events[i]

    return Request(scope, receive)


@pytest.mark.asyncio
async def test_accepts_streaming_body_within_limit() -> None:
    request = _streaming_request([b"12345"])
    body = await _read_body_with_limit(request, max_bytes=10)
    assert body == b"12345"


@pytest.mark.asyncio
async def test_rejects_streaming_body_over_limit_without_content_length() -> None:
    request = _streaming_request([b"12345", b"67890", b"extra-bytes"])
    with pytest.raises(HTTPException) as exc_info:
        await _read_body_with_limit(request, max_bytes=10)
    assert exc_info.value.status_code == 413


@pytest.mark.asyncio
async def test_rejects_when_declared_content_length_exceeds_limit() -> None:
    request = _streaming_request([b""], headers=[(b"content-length", b"1000")])
    with pytest.raises(HTTPException) as exc_info:
        await _read_body_with_limit(request, max_bytes=10)
    assert exc_info.value.status_code == 413


@pytest.mark.asyncio
async def test_rejects_invalid_content_length_header() -> None:
    request = _streaming_request([b""], headers=[(b"content-length", b"not-a-number")])
    with pytest.raises(HTTPException) as exc_info:
        await _read_body_with_limit(request, max_bytes=10)
    assert exc_info.value.status_code == 400
