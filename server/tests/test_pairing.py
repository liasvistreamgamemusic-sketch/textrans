from __future__ import annotations

from datetime import UTC, datetime, timedelta

import pytest
import yaml

from voice_server.pairing import PairingStore, _hash_code


def test_issue_writes_hash_not_plaintext(tmp_path) -> None:
    path = tmp_path / "pairing.yaml"
    store = PairingStore(path)
    pairing_code = store.issue(ttl_s=300)

    content = path.read_text(encoding="utf-8")
    assert pairing_code.code not in content
    assert _hash_code(pairing_code.code) in content


def test_verify_and_consume_succeeds_once(tmp_path) -> None:
    store = PairingStore(tmp_path / "pairing.yaml")
    pairing_code = store.issue(ttl_s=300)

    assert store.verify_and_consume(pairing_code.code) is True
    # 使い切り: 2回目は同じコードでも False。
    assert store.verify_and_consume(pairing_code.code) is False


def test_verify_and_consume_removes_file_on_success(tmp_path) -> None:
    path = tmp_path / "pairing.yaml"
    store = PairingStore(path)
    pairing_code = store.issue(ttl_s=300)

    store.verify_and_consume(pairing_code.code)
    assert not path.exists()


def test_verify_and_consume_rejects_wrong_code(tmp_path) -> None:
    path = tmp_path / "pairing.yaml"
    store = PairingStore(path)
    store.issue(ttl_s=300)

    assert store.verify_and_consume("000000") is False
    # 失敗時はファイルを削除しない (正しいコードならまだ使える)。
    assert path.exists()


def test_verify_and_consume_rejects_when_no_file(tmp_path) -> None:
    store = PairingStore(tmp_path / "pairing.yaml")
    assert store.verify_and_consume("123456") is False


def test_verify_and_consume_rejects_expired_code(tmp_path) -> None:
    path = tmp_path / "pairing.yaml"
    store = PairingStore(path)
    expired_at = datetime.now(UTC) - timedelta(seconds=1)
    path.write_text(
        yaml.safe_dump({"code_hash": _hash_code("123456"), "expires_at": expired_at.isoformat()}),
        encoding="utf-8",
    )

    assert store.verify_and_consume("123456") is False


@pytest.mark.parametrize("raw", ["not-yaml-dict", "code_hash: abc\nexpires_at: not-a-date\n"])
def test_verify_and_consume_rejects_malformed_file(tmp_path, raw) -> None:
    path = tmp_path / "pairing.yaml"
    path.write_text(raw, encoding="utf-8")
    store = PairingStore(path)
    assert store.verify_and_consume("123456") is False
