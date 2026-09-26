"""辞書 (設計書 §4.4)。確定置換・ASR文脈・LLM用語集の3用途に供給する。

正本は `dictionary.yaml`。ファイル監視は mtime ポーリングで行う。
"""

from __future__ import annotations

import difflib
import logging
import os
import re
import tempfile
import threading
from collections.abc import Sequence
from pathlib import Path

import yaml
from pydantic import BaseModel, Field

from .protocol_schemas import validate_message

logger = logging.getLogger(__name__)

ASR_CONTEXT_MAX_TERMS = 100
GLOSSARY_MAX_TERMS = 30
_KATAKANA_RE = re.compile(r"[ァ-ヶー]+")
_KATAKANA_MATCH_RATIO = 0.7


class DictionaryTerm(BaseModel):
    surface: str
    aliases: list[str] = Field(default_factory=list)
    replace: bool = False


class Dictionary(BaseModel):
    terms: list[DictionaryTerm] = Field(default_factory=list)


class DictionaryStore:
    """`dictionary.yaml` を保持し、mtime ポーリングで再読込する。"""

    def __init__(self, path: Path) -> None:
        self._path = Path(path)
        self._lock = threading.RLock()
        self._mtime: float | None = None
        self._dictionary = Dictionary()
        self.reload_if_changed(force=True)

    def reload_if_changed(self, force: bool = False) -> bool:
        if not self._path.exists():
            return False
        mtime = self._path.stat().st_mtime
        if not force and mtime == self._mtime:
            return False
        raw = yaml.safe_load(self._path.read_text(encoding="utf-8")) or {"terms": []}
        dictionary = Dictionary.model_validate(raw)
        with self._lock:
            self._dictionary = dictionary
            self._mtime = mtime
        return True

    def get(self) -> Dictionary:
        try:
            self.reload_if_changed()
        except (OSError, ValueError, yaml.YAMLError) as exc:
            logger.warning("辞書の再読込に失敗しました。前回の内容を使い続けます: %s", type(exc).__name__)
        with self._lock:
            return self._dictionary

    def save(self, dictionary: Dictionary) -> None:
        """PUT /v1/dictionary。保存前にプロトコルスキーマで検証し、アトミックに書き込む。"""
        payload = dictionary.model_dump(mode="json")
        validate_message("dictionary", payload)
        self._path.parent.mkdir(parents=True, exist_ok=True)
        serialized = yaml.safe_dump(payload, allow_unicode=True, sort_keys=False)

        # 一時ファイル + os.replace() でアトミックに書き込む。書き込み中のプロセス
        # 異常終了や同時読み込みで、辞書ファイルが半端な内容になることを防ぐ。
        fd, tmp_path_str = tempfile.mkstemp(
            dir=self._path.parent, prefix=f".{self._path.name}.", suffix=".tmp"
        )
        tmp_path = Path(tmp_path_str)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as tmp_file:
                tmp_file.write(serialized)
            os.replace(tmp_path, self._path)
        except OSError:
            try:
                tmp_path.unlink()
            except OSError:
                logger.warning("一時ファイルの削除に失敗しました (path=%s)", tmp_path)
            raise

        with self._lock:
            self._dictionary = dictionary
            self._mtime = self._path.stat().st_mtime


def apply_fixed_replacements(text: str, terms: Sequence[DictionaryTerm]) -> str:
    """`replace: true` の項目について、`aliases` を `surface` へ完全一致置換する。"""
    replacements: list[tuple[str, str]] = []
    for term in terms:
        if not term.replace:
            continue
        for alias in term.aliases:
            replacements.append((alias, term.surface))
    # 長いエイリアスを先に置換し、短いエイリアスが部分文字列として横取りするのを防ぐ。
    replacements.sort(key=lambda pair: len(pair[0]), reverse=True)
    result = text
    for alias, surface in replacements:
        result = result.replace(alias, surface)
    return result


def build_asr_context(terms: Sequence[DictionaryTerm], max_terms: int = ASR_CONTEXT_MAX_TERMS) -> str:
    """ASR の `initial_prompt` に渡す文脈。先頭から最大 `max_terms` 語。"""
    surfaces = [term.surface for term in terms][:max_terms]
    return " ".join(surfaces)


def select_glossary_terms(
    text: str, terms: Sequence[DictionaryTerm], max_terms: int = GLOSSARY_MAX_TERMS
) -> list[DictionaryTerm]:
    """LLM 清書用の用語集。出現一致、またはカタカナ読みが近い項目を最大 `max_terms` 件選ぶ。"""
    if not text:
        return []
    katakana_tokens = _KATAKANA_RE.findall(text)
    selected: list[DictionaryTerm] = []
    for term in terms:
        if len(selected) >= max_terms:
            break
        candidates = [term.surface, *term.aliases]
        if any(candidate in text for candidate in candidates):
            selected.append(term)
            continue
        if _has_close_katakana_match(katakana_tokens, candidates):
            selected.append(term)
    return selected


def _has_close_katakana_match(katakana_tokens: list[str], candidates: Sequence[str]) -> bool:
    katakana_candidates = [c for c in candidates if _KATAKANA_RE.fullmatch(c)]
    if not katakana_tokens or not katakana_candidates:
        return False
    for token in katakana_tokens:
        for candidate in katakana_candidates:
            if difflib.SequenceMatcher(None, token, candidate).ratio() >= _KATAKANA_MATCH_RATIO:
                return True
    return False
