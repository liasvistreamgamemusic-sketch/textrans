from __future__ import annotations

import httpx
import pytest
import respx

from voice_server.cleaner import Cleaner
from voice_server.config import LlmSection
from voice_server.llm_client import LlmClient

BASE_URL = "http://127.0.0.1:8081"


def _llm_response(content: str) -> dict:
    return {"choices": [{"message": {"content": content}}]}


@pytest.mark.asyncio
async def test_clean_mode_short_input_skips_llm() -> None:
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    result = await cleaner.clean("えっと明日", "clean", "")
    assert result.flags == ["llm_skipped"]
    assert result.text == "明日"


@pytest.mark.asyncio
async def test_raw_mode_returns_raw_text_unchanged() -> None:
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    result = await cleaner.clean("えっと明日の会議について話したいことがあります", "raw", "")
    assert result.flags == []
    assert result.text == "えっと明日の会議について話したいことがあります"


@pytest.mark.asyncio
async def test_empty_input_returns_empty() -> None:
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    result = await cleaner.clean("", "clean", "")
    assert result.text == ""
    assert result.flags == []


@pytest.mark.asyncio
@respx.mock
async def test_clean_mode_calls_llm_and_accepts_good_output() -> None:
    respx.post(f"{BASE_URL}/v1/chat/completions").mock(
        return_value=httpx.Response(200, json=_llm_response("明日の会議についてお話したいことがあります。"))
    )
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    raw = "えーと明日の会議についてお話したいことがあります"
    result = await cleaner.clean(raw, "clean", "")
    assert result.flags == []
    assert result.text == "明日の会議についてお話したいことがあります。"


@pytest.mark.asyncio
@respx.mock
async def test_llm_timeout_falls_back_to_raw_text_with_skip_flag() -> None:
    async def _slow(request: httpx.Request) -> httpx.Response:
        import asyncio

        await asyncio.sleep(10)
        return httpx.Response(200, json=_llm_response("遅い応答"))

    respx.post(f"{BASE_URL}/v1/chat/completions").mock(side_effect=_slow)
    client = LlmClient(BASE_URL, "test-model")
    cfg = LlmSection(timeout_base_ms=10, timeout_per_char_ms=1)
    cleaner = Cleaner(client, cfg)
    raw = "明日の会議についてお話したいことがあります"
    result = await cleaner.clean(raw, "clean", "")
    assert result.flags == ["llm_skipped"]
    assert result.text == raw


@pytest.mark.asyncio
@respx.mock
async def test_llm_connection_error_falls_back_to_raw_text() -> None:
    respx.post(f"{BASE_URL}/v1/chat/completions").mock(side_effect=httpx.ConnectError("refused"))
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    raw = "明日の会議についてお話したいことがあります"
    result = await cleaner.clean(raw, "clean", "")
    assert result.flags == ["llm_skipped"]
    assert result.text == raw


@pytest.mark.asyncio
@respx.mock
async def test_clean_mode_rejects_bad_output_and_falls_back() -> None:
    respx.post(f"{BASE_URL}/v1/chat/completions").mock(
        return_value=httpx.Response(200, json=_llm_response("了解"))
    )
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    raw = "明日の会議についてお話したいことが色々あります"
    result = await cleaner.clean(raw, "clean", "")
    assert result.flags == ["llm_rejected"]
    assert result.text == raw


@pytest.mark.asyncio
@respx.mock
async def test_translate_mode_always_calls_llm_even_for_short_input() -> None:
    respx.post(f"{BASE_URL}/v1/chat/completions").mock(
        return_value=httpx.Response(200, json=_llm_response("Tomorrow."))
    )
    client = LlmClient(BASE_URL, "test-model")
    cleaner = Cleaner(client, LlmSection())
    result = await cleaner.clean("明日", "translate_en", "")
    assert result.flags == []
    assert result.text == "Tomorrow."
