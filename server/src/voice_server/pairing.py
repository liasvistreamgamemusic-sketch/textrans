"""ペアリング機能: CLI で生成した6桁コードを使い、REST 経由でトークンを発行する。

`pairing.yaml` には平文コードを保存せず、SHA-256 ハッシュと有効期限のみを保存する (`auth.py` の
`TokenStore` と同じ方針)。検証は定数時間比較 (`hmac.compare_digest`) で行い、成功したら1回きりの
使い切りとしてファイルを削除する。
"""

from __future__ import annotations

import hashlib
import hmac
import logging
import os
import secrets
import threading
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path

import yaml

logger = logging.getLogger(__name__)

_CODE_DIGITS = 6
_CODE_MODULUS = 10**_CODE_DIGITS


def _hash_code(code: str) -> str:
    return hashlib.sha256(code.encode("utf-8")).hexdigest()


def _generate_code() -> str:
    return f"{secrets.randbelow(_CODE_MODULUS):0{_CODE_DIGITS}d}"


@dataclass(frozen=True)
class PairingCode:
    code: str
    expires_at: datetime


class PairingStore:
    """`pairing.yaml` (コードの SHA-256 ハッシュ + 有効期限) を管理する。"""

    def __init__(self, path: Path) -> None:
        self._path = Path(path)
        self._lock = threading.RLock()

    def issue(self, ttl_s: float) -> PairingCode:
        """6桁の数字コードを生成し、ハッシュと有効期限を書き込む。平文コードはこの1回だけ返す。"""
        with self._lock:
            code = _generate_code()
            expires_at = datetime.now(UTC) + timedelta(seconds=ttl_s)
            self._path.parent.mkdir(parents=True, exist_ok=True)
            self._path.write_text(
                yaml.safe_dump(
                    {"code_hash": _hash_code(code), "expires_at": expires_at.isoformat()},
                    allow_unicode=True,
                    sort_keys=True,
                ),
                encoding="utf-8",
            )
            try:
                os.chmod(self._path, 0o600)
            except OSError:
                logger.warning("pairing.yaml の権限設定に失敗しました (path=%s)", self._path)
            return PairingCode(code=code, expires_at=expires_at)

    def verify_and_consume(self, code: str) -> bool:
        """コードが有効なら `pairing.yaml` を削除して True を返す (1回きり)。

        不一致・期限切れ・ファイル無し・内容不正はいずれも False (呼び出し側で一律 403 とし、
        どの理由かを区別しない)。
        """
        with self._lock:
            if not self._path.exists():
                return False
            try:
                raw = yaml.safe_load(self._path.read_text(encoding="utf-8")) or {}
            except yaml.YAMLError:
                logger.warning("pairing.yaml の読み込みに失敗しました (path=%s)", self._path)
                return False
            if not isinstance(raw, dict):
                logger.warning("pairing.yaml の内容が不正です (path=%s)", self._path)
                return False

            code_hash = raw.get("code_hash", "")
            expires_at_raw = raw.get("expires_at", "")
            try:
                expires_at = datetime.fromisoformat(expires_at_raw)
            except (TypeError, ValueError):
                logger.warning("pairing.yaml の expires_at 形式が不正です (path=%s)", self._path)
                return False

            valid = hmac.compare_digest(code_hash, _hash_code(code)) and datetime.now(UTC) <= expires_at
            if valid:
                self._consume_locked()
            return valid

    def _consume_locked(self) -> None:
        try:
            self._path.unlink()
        except OSError:
            logger.warning("pairing.yaml の削除に失敗しました (path=%s)", self._path)
