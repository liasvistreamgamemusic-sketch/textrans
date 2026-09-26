"""LLM 出力ガード (設計書 §4.5)。不合格ならガードが `raw_text` へ戻す。"""

from __future__ import annotations

import re
from dataclasses import dataclass

_THINK_TAG_RE = re.compile(r"<think>.*?</think>", re.DOTALL)
_CODE_FENCE_RE = re.compile(r"^```[a-zA-Z]*\n?|```$")
_PREFACE_PATTERNS = [
    re.compile(r"^(清書(しました|結果)[:：]?\s*)"),
    re.compile(r"^(以下(の)?(通り|とおり)[:：]?\s*)"),
    re.compile(r"^(翻訳(しました|結果)[:：]?\s*)"),
]
_JP_CHAR_RE = re.compile(r"[぀-ヿ㐀-䶿一-鿿]")

CLEAN_LENGTH_RATIO_MIN = 0.4
CLEAN_LENGTH_RATIO_MAX = 1.3
TRANSLATE_LENGTH_RATIO_MIN = 0.8
TRANSLATE_LENGTH_RATIO_MAX = 6.0
TRANSLATE_JP_RATIO_MAX = 0.2


@dataclass(frozen=True)
class GuardResult:
    text: str
    flags: list[str]
    reason: str | None = None  # 却下理由 (ログ用。本文は含めない)


def strip_llm_noise(text: str) -> str:
    """`<think>` タグ、前置き、コードフェンスを除去する。"""
    cleaned = _THINK_TAG_RE.sub("", text).strip()
    cleaned = _CODE_FENCE_RE.sub("", cleaned).strip()
    for pattern in _PREFACE_PATTERNS:
        cleaned = pattern.sub("", cleaned)
    return cleaned.strip()


def japanese_char_ratio(text: str) -> float:
    if not text:
        return 0.0
    jp_chars = len(_JP_CHAR_RE.findall(text))
    return jp_chars / len(text)


def apply_clean_guard(raw_text: str, llm_text: str) -> GuardResult:
    """清書モードの出力ガード: 長さ比・余計な出力・空出力を検査する。"""
    cleaned = strip_llm_noise(llm_text)
    if raw_text and not cleaned:
        return GuardResult(raw_text, ["llm_rejected"], "empty")
    ratio = len(cleaned) / len(raw_text) if raw_text else 1.0
    if ratio < CLEAN_LENGTH_RATIO_MIN or ratio > CLEAN_LENGTH_RATIO_MAX:
        return GuardResult(raw_text, ["llm_rejected"], f"length_ratio={ratio:.2f}")
    return GuardResult(cleaned, [])


def apply_translate_guard(raw_text: str, llm_text: str) -> GuardResult:
    """英訳モードの出力ガード: 日本語混入率・長さ比を検査する。"""
    cleaned = strip_llm_noise(llm_text)
    if raw_text and not cleaned:
        return GuardResult(raw_text, ["llm_rejected"], "empty")
    jp_ratio = japanese_char_ratio(cleaned)
    if jp_ratio > TRANSLATE_JP_RATIO_MAX:
        return GuardResult(raw_text, ["llm_rejected"], f"jp_ratio={jp_ratio:.2f}")
    ratio = len(cleaned) / len(raw_text) if raw_text else 1.0
    if ratio < TRANSLATE_LENGTH_RATIO_MIN or ratio > TRANSLATE_LENGTH_RATIO_MAX:
        return GuardResult(raw_text, ["llm_rejected"], f"length_ratio={ratio:.2f}")
    return GuardResult(cleaned, [])
