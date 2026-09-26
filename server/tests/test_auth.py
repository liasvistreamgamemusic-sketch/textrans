from __future__ import annotations

from voice_server.auth import TokenStore, hash_token


def test_issue_and_verify_roundtrip(tmp_path) -> None:
    store = TokenStore(tmp_path / "tokens.yaml")
    raw = store.issue("mac-mini")
    assert store.verify(raw) == "mac-mini"


def test_verify_rejects_unknown_token(tmp_path) -> None:
    store = TokenStore(tmp_path / "tokens.yaml")
    store.issue("mac-mini")
    assert store.verify("not-a-real-token") is None


def test_verify_rejects_none_or_empty(tmp_path) -> None:
    store = TokenStore(tmp_path / "tokens.yaml")
    assert store.verify(None) is None
    assert store.verify("") is None


def test_revoke_removes_token(tmp_path) -> None:
    store = TokenStore(tmp_path / "tokens.yaml")
    raw = store.issue("mac-mini")
    assert store.revoke("mac-mini") is True
    assert store.verify(raw) is None


def test_revoke_unknown_device_returns_false(tmp_path) -> None:
    store = TokenStore(tmp_path / "tokens.yaml")
    assert store.revoke("no-such-device") is False


def test_list_returns_device_names_without_plaintext(tmp_path) -> None:
    store = TokenStore(tmp_path / "tokens.yaml")
    store.issue("mac-mini")
    store.issue("windows-pc")
    records = store.list()
    names = {r.device_name for r in records}
    assert names == {"mac-mini", "windows-pc"}
    for record in records:
        assert record.token_hash  # ハッシュは保存されているが平文ではない


def test_tokens_file_stores_hash_not_plaintext(tmp_path) -> None:
    path = tmp_path / "tokens.yaml"
    store = TokenStore(path)
    raw = store.issue("mac-mini")
    content = path.read_text(encoding="utf-8")
    assert raw not in content
    assert hash_token(raw) in content


def test_verify_does_not_crash_when_tokens_key_is_null(tmp_path) -> None:
    """`tokens: null` (キーはあるが値が空) でも例外にならず、単に未認証として扱う。"""
    path = tmp_path / "tokens.yaml"
    path.write_text("tokens: null\n", encoding="utf-8")
    store = TokenStore(path)
    assert store.verify("anything") is None
    assert store.list() == []


def test_revoke_does_not_crash_when_tokens_key_is_null(tmp_path) -> None:
    path = tmp_path / "tokens.yaml"
    path.write_text("tokens: null\n", encoding="utf-8")
    store = TokenStore(path)
    assert store.revoke("mac-mini") is False
