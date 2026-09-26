"""契約テスト: protocol/fixtures を各スキーマで検証する (protocol/README.md)。"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import ValidationError

from voice_server.protocol_schemas import find_protocol_v1_dir, validate_message

_PROTOCOL_ROOT = Path(__file__).resolve().parents[2] / "protocol"
_FIXTURES_DIR = _PROTOCOL_ROOT / "fixtures"
_KNOWN_TYPES = ["start", "end", "cancel", "ready", "partial", "final", "error", "dictionary"]


def _schema_name_for(fixture_path: Path) -> str:
    stem = fixture_path.stem
    # ファイル名の先頭 `_` までがスキーマ名 (protocol/README.md)
    for known in sorted(_KNOWN_TYPES, key=len, reverse=True):
        if stem == known or stem.startswith(f"{known}_"):
            return known
    raise AssertionError(f"fixture名からスキーマ名を特定できません: {fixture_path.name}")


def _valid_fixtures() -> list[Path]:
    return sorted((_FIXTURES_DIR / "valid").glob("*.json"))


def _invalid_fixtures() -> list[Path]:
    return sorted((_FIXTURES_DIR / "invalid").glob("*.json"))


def test_protocol_dir_resolves() -> None:
    assert find_protocol_v1_dir().is_dir()


@pytest.mark.parametrize("fixture_path", _valid_fixtures(), ids=lambda p: p.name)
def test_valid_fixtures_pass(fixture_path: Path) -> None:
    schema_name = _schema_name_for(fixture_path)
    instance = json.loads(fixture_path.read_text(encoding="utf-8"))
    validate_message(schema_name, instance)  # ValidationError を投げなければ成功


@pytest.mark.parametrize("fixture_path", _invalid_fixtures(), ids=lambda p: p.name)
def test_invalid_fixtures_rejected(fixture_path: Path) -> None:
    schema_name = _schema_name_for(fixture_path)
    instance = json.loads(fixture_path.read_text(encoding="utf-8"))
    with pytest.raises(ValidationError):
        validate_message(schema_name, instance)
