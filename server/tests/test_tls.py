from __future__ import annotations

import re

import pytest

from voice_server.tls import ensure_certificate, fingerprint

_FINGERPRINT_RE = re.compile(r"^([0-9A-F]{2}:){31}[0-9A-F]{2}$")


def test_ensure_certificate_creates_files(tmp_path) -> None:
    cert_path = tmp_path / "tls" / "server.crt"
    key_path = tmp_path / "tls" / "server.key"
    created = ensure_certificate(cert_path, key_path)
    assert created is True
    assert cert_path.exists()
    assert key_path.exists()


def test_ensure_certificate_is_idempotent(tmp_path) -> None:
    cert_path = tmp_path / "tls" / "server.crt"
    key_path = tmp_path / "tls" / "server.key"
    ensure_certificate(cert_path, key_path)
    created_again = ensure_certificate(cert_path, key_path)
    assert created_again is False


def test_fingerprint_format(tmp_path) -> None:
    cert_path = tmp_path / "tls" / "server.crt"
    key_path = tmp_path / "tls" / "server.key"
    ensure_certificate(cert_path, key_path)
    fp = fingerprint(cert_path)
    assert _FINGERPRINT_RE.match(fp), fp


def test_fingerprint_missing_cert_raises(tmp_path) -> None:
    with pytest.raises(FileNotFoundError):
        fingerprint(tmp_path / "does-not-exist.crt")
