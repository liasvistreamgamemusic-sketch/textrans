"""端末トークンの発行・失効・検証 (設計書 §3.1)。

平文トークンはサーバーに保存しない。SHA-256 ハッシュのみを `tokens.yaml` に保存し、
検証は定数時間比較 (`hmac.compare_digest`) で行う。
"""

from __future__ import annotations

import hashlib
import hmac
import logging
import os
import secrets
import threading
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

import yaml

logger = logging.getLogger(__name__)


def hash_token(raw_token: str) -> str:
    return hashlib.sha256(raw_token.encode("utf-8")).hexdigest()


_BEARER_PREFIX = "Bearer "


def extract_bearer_token(auth_header: str) -> str | None:
    """`Authorization: Bearer <token>` ヘッダーからトークンを取り出す (WS/REST共通)。"""
    if not auth_header.startswith(_BEARER_PREFIX):
        return None
    return auth_header[len(_BEARER_PREFIX) :]


@dataclass(frozen=True)
class TokenRecord:
    device_name: str
    token_hash: str
    issued_at: str


class TokenStore:
    """`tokens.yaml` (端末名 → SHA-256 ハッシュ) を管理する。"""

    def __init__(self, path: Path) -> None:
        self._path = Path(path)
        self._lock = threading.RLock()

    def _load(self) -> dict[str, dict[str, str]]:
        if not self._path.exists():
            return {}
        raw = yaml.safe_load(self._path.read_text(encoding="utf-8")) or {}
        # `tokens: null` (キーはあるが値が空) でも {} を返す。`.get(..., {})` はキーが
        # 存在する場合デフォルトを使わないため、None のまま返して全断していた。
        return raw.get("tokens") or {}

    def _save(self, tokens: dict[str, dict[str, str]]) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        self._path.write_text(
            yaml.safe_dump({"tokens": tokens}, allow_unicode=True, sort_keys=True),
            encoding="utf-8",
        )
        try:
            os.chmod(self._path, 0o600)
        except OSError:
            logger.warning("tokens.yaml の権限設定に失敗しました (path=%s)", self._path)

    def issue(self, device_name: str) -> str:
        """新しいトークンを発行し、平文を1回だけ返す。"""
        with self._lock:
            tokens = self._load()
            raw_token = secrets.token_urlsafe(32)
            tokens[device_name] = {
                "hash": hash_token(raw_token),
                "issued_at": datetime.now(UTC).isoformat(),
            }
            self._save(tokens)
            return raw_token

    def revoke(self, device_name: str) -> bool:
        with self._lock:
            tokens = self._load()
            if device_name not in tokens:
                return False
            del tokens[device_name]
            self._save(tokens)
            return True

    def list(self) -> list[TokenRecord]:
        with self._lock:
            tokens = self._load()
        return [
            TokenRecord(
                device_name=name,
                token_hash=entry.get("hash", ""),
                issued_at=entry.get("issued_at", ""),
            )
            for name, entry in sorted(tokens.items())
        ]

    def verify(self, raw_token: str | None) -> str | None:
        """有効なら端末名を返す。無効・欠落なら None (本文をログに出さない)。"""
        if not raw_token:
            return None
        candidate_hash = hash_token(raw_token)
        with self._lock:
            tokens = self._load()
        for device_name, entry in tokens.items():
            if hmac.compare_digest(entry.get("hash", ""), candidate_hash):
                return device_name
        return None
