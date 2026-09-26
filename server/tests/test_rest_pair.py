"""結合テスト: POST /v1/pair (設計書外・追加機能)。"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta

import pytest
import yaml
from fastapi.testclient import TestClient

from voice_server.app import create_app
from voice_server.asr.dummy import DummyAsrBackend
from voice_server.config import ServerConfig
from voice_server.pairing import _hash_code
from voice_server.tls import ensure_certificate
from voice_server.tls import fingerprint as compute_fingerprint

from .conftest import StubVadClassifier

DEVICE_NAME = "test-device"


def _build_app(tmp_path, pairing_mode="code", **config_overrides):
    cert_path = tmp_path / "tls" / "server.crt"
    key_path = tmp_path / "tls" / "server.key"
    ensure_certificate(cert_path, key_path)
    cfg = ServerConfig(
        server={"tls_cert_path": cert_path, "tls_key_path": key_path},
        tokens={"path": tmp_path / "tokens.yaml"},
        dictionary={"path": tmp_path / "dictionary.yaml"},
        pairing={"path": tmp_path / "pairing.yaml", "mode": pairing_mode},
        asr={"backend": "dummy"},
        **config_overrides,
    )
    app = create_app(cfg, vad_classifier=StubVadClassifier(speech=True), asr_backend=DummyAsrBackend())
    return app, cert_path


@pytest.fixture(autouse=True)
def _no_pair_failure_delay(monkeypatch):
    """失敗時のスリープをテストで待たないよう 0 にする。"""
    monkeypatch.setattr("voice_server.rest.asyncio.sleep", _instant_sleep)


async def _instant_sleep(_seconds: float) -> None:
    return None


def test_pair_with_valid_code_returns_token_and_fingerprint_and_consumes_file(tmp_path) -> None:
    app, cert_path = _build_app(tmp_path)
    pairing_code = app.state.voice.pairing_store.issue(ttl_s=300)
    client = TestClient(app)

    response = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": pairing_code.code})

    assert response.status_code == 200
    body = response.json()
    assert set(body) == {"device", "token", "fingerprint"}
    assert body["device"] == DEVICE_NAME
    assert body["fingerprint"] == compute_fingerprint(cert_path)
    assert app.state.voice.token_store.verify(body["token"]) == DEVICE_NAME
    assert not app.state.voice.config.pairing.path.exists()


def test_pair_reused_code_is_rejected(tmp_path) -> None:
    app, _ = _build_app(tmp_path)
    pairing_code = app.state.voice.pairing_store.issue(ttl_s=300)
    client = TestClient(app)

    first = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": pairing_code.code})
    assert first.status_code == 200

    second = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": pairing_code.code})
    assert second.status_code == 403
    assert second.json() == {"detail": "pairing code invalid or expired"}


def test_pair_expired_code_is_rejected(tmp_path) -> None:
    app, _ = _build_app(tmp_path)
    pairing_path = app.state.voice.config.pairing.path
    expired_at = datetime.now(UTC) - timedelta(seconds=1)
    pairing_path.parent.mkdir(parents=True, exist_ok=True)
    pairing_path.write_text(
        yaml.safe_dump({"code_hash": _hash_code("123456"), "expires_at": expired_at.isoformat()}),
        encoding="utf-8",
    )
    client = TestClient(app)

    response = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": "123456"})

    assert response.status_code == 403
    assert response.json() == {"detail": "pairing code invalid or expired"}


def test_pair_no_pairing_file_is_rejected(tmp_path) -> None:
    app, _ = _build_app(tmp_path)
    client = TestClient(app)

    response = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": "123456"})

    assert response.status_code == 403
    assert response.json() == {"detail": "pairing code invalid or expired"}


@pytest.mark.parametrize(
    "device",
    ["", "a" * 65, "has space", "has/slash", "has:colon"],
)
def test_pair_rejects_invalid_device_name(tmp_path, device) -> None:
    app, _ = _build_app(tmp_path)
    pairing_code = app.state.voice.pairing_store.issue(ttl_s=300)
    client = TestClient(app)

    response = client.post("/v1/pair", json={"device": device, "code": pairing_code.code})

    assert response.status_code == 422


def test_token_issued_via_pair_works_for_v1_info(tmp_path) -> None:
    app, _ = _build_app(tmp_path)
    pairing_code = app.state.voice.pairing_store.issue(ttl_s=300)
    client = TestClient(app)

    pair_response = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": pairing_code.code})
    token = pair_response.json()["token"]

    info_response = client.get("/v1/info", headers={"Authorization": f"Bearer {token}"})
    assert info_response.status_code == 200


def test_pair_open_mode_without_code_returns_token(tmp_path) -> None:
    app, cert_path = _build_app(tmp_path, pairing_mode="open")
    client = TestClient(app)

    response = client.post("/v1/pair", json={"device": DEVICE_NAME})

    assert response.status_code == 200
    body = response.json()
    assert set(body) == {"device", "token", "fingerprint"}
    assert body["device"] == DEVICE_NAME
    assert body["fingerprint"] == compute_fingerprint(cert_path)
    assert app.state.voice.token_store.verify(body["token"]) == DEVICE_NAME


def test_pair_open_mode_ignores_code(tmp_path) -> None:
    app, _ = _build_app(tmp_path, pairing_mode="open")
    client = TestClient(app)

    response = client.post("/v1/pair", json={"device": DEVICE_NAME, "code": "000000"})

    assert response.status_code == 200
    assert app.state.voice.token_store.verify(response.json()["token"]) == DEVICE_NAME


def test_pair_open_mode_reissue_for_same_device_revokes_old_token(tmp_path) -> None:
    app, _ = _build_app(tmp_path, pairing_mode="open")
    client = TestClient(app)

    first = client.post("/v1/pair", json={"device": DEVICE_NAME})
    old_token = first.json()["token"]

    second = client.post("/v1/pair", json={"device": DEVICE_NAME})
    new_token = second.json()["token"]

    assert old_token != new_token
    info_with_old_token = client.get("/v1/info", headers={"Authorization": f"Bearer {old_token}"})
    assert info_with_old_token.status_code == 401
    info_with_new_token = client.get("/v1/info", headers={"Authorization": f"Bearer {new_token}"})
    assert info_with_new_token.status_code == 200
