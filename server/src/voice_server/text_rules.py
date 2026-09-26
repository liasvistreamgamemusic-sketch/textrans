"""ルールベースのテキスト処理 (設計書 §4.3, §4.5)。"""

from __future__ import annotations

import re

# 削除するフィラー。長い表記を先に置換し、短い表記が部分文字列として横取りしないようにする。
# 「あの」は指示語 (例:「あの人」) と区別するため専用に扱う。
_SIMPLE_FILLERS = ["えっと", "えー", "まあ", "なんか"]
_ANO_FILLER_RE = re.compile(r"あの(?!人)")

# 3回以上の連続反復を検出する (ホットワード・無音由来の反復出力対策)。
# ASR 出力 (1発話・最大120秒分) が対象で、外部の非有界な入力ではないため
# 部分文字列長を20文字に制限して壊滅的バックトラッキングを避ける。
_REPETITION_RE = re.compile(r"(.{1,20}?)\1{2,}")


def strip_fillers(text: str) -> str:
    """清書モードで8文字以下の入力に適用するルールベースのフィラー除去 (LLMを通さない)。"""
    result = _ANO_FILLER_RE.sub("", text)
    for filler in _SIMPLE_FILLERS:
        result = result.replace(filler, "")
    return result


def detect_repetition(text: str, min_repeats: int = 3) -> bool:
    """同じ語句が `min_repeats` 回以上連続して現れるかを検出する。"""
    if min_repeats < 3:
        raise ValueError("min_repeats は 3 以上を指定する (設計書 §4.3)")
    match = _REPETITION_RE.search(text)
    if match is None:
        return False
    unit = match.group(1)
    return unit != "" and len(match.group(0)) >= len(unit) * min_repeats
