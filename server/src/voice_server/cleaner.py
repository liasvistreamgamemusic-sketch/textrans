"""LLM清書のオーケストレーション: スキップ判定・タイムアウト・ガード適用 (設計書 §4.5)。"""

from __future__ import annotations

import asyncio
import logging
import time
from dataclasses import dataclass, field

import httpx

from .config import LlmSection
from .guard import apply_clean_guard, apply_translate_guard
from .llm_client import LlmClient, build_system_prompt, build_user_prompt
from .text_rules import strip_fillers

logger = logging.getLogger(__name__)

# 清書モード限定: 8文字以下は LLM を通さずルールベースのフィラー除去のみで返す。
# 英訳モードは常に LLM を通す (設計書 §4.5)。
SHORT_INPUT_THRESHOLD = 8
_TEMPERATURE_MIN = 0.0
_TEMPERATURE_MAX = 0.2


@dataclass(frozen=True)
class CleanResult:
    text: str
    flags: list[str] = field(default_factory=list)
    llm_ms: int = 0


class Cleaner:
    def __init__(self, llm_client: LlmClient, llm_config: LlmSection) -> None:
        self._llm = llm_client
        self._cfg = llm_config

    async def clean(self, raw_text: str, mode: str, glossary_text: str) -> CleanResult:
        if not raw_text:
            return CleanResult(text="", flags=[])
        if mode == "raw":
            return CleanResult(text=raw_text, flags=[])
        if mode == "clean" and len(raw_text) <= SHORT_INPUT_THRESHOLD:
            return CleanResult(text=strip_fillers(raw_text), flags=["llm_skipped"])

        timeout_s = (self._cfg.timeout_base_ms + self._cfg.timeout_per_char_ms * len(raw_text)) / 1000
        max_tokens = len(raw_text) * 2 + 64
        temperature = min(max(self._cfg.temperature, _TEMPERATURE_MIN), _TEMPERATURE_MAX)
        system_prompt = build_system_prompt(mode, glossary_text)
        user_prompt = build_user_prompt(raw_text)

        started = time.monotonic()
        try:
            llm_text = await asyncio.wait_for(
                self._llm.chat(system_prompt, user_prompt, temperature, max_tokens),
                timeout=timeout_s,
            )
        except (TimeoutError, httpx.HTTPError) as exc:
            elapsed_ms = int((time.monotonic() - started) * 1000)
            logger.warning("LLM呼び出しをスキップします (種別=%s)", type(exc).__name__)
            return CleanResult(text=raw_text, flags=["llm_skipped"], llm_ms=elapsed_ms)
        llm_ms = int((time.monotonic() - started) * 1000)

        guard_fn = apply_translate_guard if mode == "translate_en" else apply_clean_guard
        guard_result = guard_fn(raw_text, llm_text)
        if guard_result.reason:
            logger.warning("LLM出力を却下しました (mode=%s, reason=%s)", mode, guard_result.reason)
        return CleanResult(text=guard_result.text, flags=guard_result.flags, llm_ms=llm_ms)
