"""結合テスト: dummy backend + stub VAD で WebSocket の一連の流れを検証する (設計書 §9 列S)。"""

from __future__ import annotations

import time
import uuid

import numpy as np
import pytest
from fastapi.testclient import TestClient
from starlette.websockets import WebSocketDisconnect

from voice_server.app import create_app
from voice_server.asr.dummy import DummyAsrBackend
from voice_server.config import LimitsSection, ServerConfig
from voice_server.ws import ConnectionContext

from .conftest import StubVadClassifier

DEVICE_NAME = "test-device"


def _wait_until(predicate, timeout: float = 2.0, interval: float = 0.01) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(interval)
    raise AssertionError("timed out waiting for condition")


def _build_app(tmp_path, **config_overrides):
    cfg = ServerConfig(
        tokens={"path": tmp_path / "tokens.yaml"},
        dictionary={"path": tmp_path / "dictionary.yaml"},
        asr={"backend": "dummy"},
        **config_overrides,
    )
    app = create_app(cfg, vad_classifier=StubVadClassifier(speech=True), asr_backend=DummyAsrBackend())
    token = app.state.voice.token_store.issue(DEVICE_NAME)
    return app, token


def _pcm_bytes(n_samples: int, amplitude: int = 1000) -> bytes:
    return np.full(n_samples, amplitude, dtype="<i2").tobytes()


def _start_payload(session_id: str, *, protocol_version: int = 1, mode: str = "raw") -> dict:
    return {
        "type": "start",
        "session_id": session_id,
        "protocol_version": protocol_version,
        "mode": mode,
        "sample_rate": 16000,
    }


def test_healthz_requires_no_auth(tmp_path) -> None:
    app, _ = _build_app(tmp_path)
    client = TestClient(app)
    response = client.get("/healthz")
    assert response.status_code == 200
    assert set(response.json()) == {"asr", "llm"}


@pytest.mark.parametrize("path", ["/v1/info", "/v1/modes", "/v1/dictionary"])
def test_rest_endpoints_require_auth(tmp_path, path) -> None:
    app, _ = _build_app(tmp_path)
    client = TestClient(app)
    response = client.get(path)
    assert response.status_code == 401


def test_rest_endpoints_succeed_with_valid_token(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    headers = {"Authorization": f"Bearer {token}"}
    assert client.get("/v1/info", headers=headers).status_code == 200
    assert client.get("/v1/modes", headers=headers).json() == {"modes": ["clean", "raw", "translate_en"]}
    assert client.get("/v1/dictionary", headers=headers).status_code == 200


def test_dictionary_put_then_get_roundtrip(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    headers = {"Authorization": f"Bearer {token}"}
    payload = {"terms": [{"surface": "Rust", "aliases": ["ラスト"], "replace": True}]}
    put_resp = client.put("/v1/dictionary", json=payload, headers=headers)
    assert put_resp.status_code == 200
    get_resp = client.get("/v1/dictionary", headers=headers)
    assert get_resp.json()["terms"][0]["surface"] == "Rust"


def test_dictionary_put_rejects_invalid_payload(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    headers = {"Authorization": f"Bearer {token}"}
    payload = {"terms": [{"surface": ""}]}
    response = client.put("/v1/dictionary", json=payload, headers=headers)
    assert response.status_code == 422


def test_websocket_rejects_missing_token(tmp_path) -> None:
    app, _ = _build_app(tmp_path)
    client = TestClient(app)
    with pytest.raises(Exception):  # noqa: B017 401拒否時の具体的な例外型はクライアント実装依存
        with client.websocket_connect("/v1/dictate"):
            pass


def test_websocket_full_flow_returns_ready_partial_final(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json(_start_payload(session_id))
        ready = ws.receive_json()
        assert ready == {"type": "ready", "session_id": session_id}

        ws.send_bytes(_pcm_bytes(1600))  # 100ms
        ws.send_json({"type": "end", "session_id": session_id})

        partial = ws.receive_json()
        assert partial["type"] == "partial"
        assert partial["session_id"] == session_id

        final = ws.receive_json()
        assert final["type"] == "final"
        assert final["session_id"] == session_id
        assert final["mode"] == "raw"
        assert final["text"] == final["raw_text"]
        assert final["flags"] == []
        assert set(final["timings"]) == {"asr_ms", "llm_ms", "total_ms"}


def test_websocket_silence_only_returns_empty_final(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json(_start_payload(session_id))
        assert ws.receive_json()["type"] == "ready"

        ws.send_bytes(_pcm_bytes(1600, amplitude=0))  # 無音
        ws.send_json({"type": "end", "session_id": session_id})

        partial = ws.receive_json()
        assert partial["text"] == ""
        final = ws.receive_json()
        assert final["type"] == "final"
        assert final["text"] == ""
        assert final["raw_text"] == ""


def test_websocket_unsupported_version_closes_connection(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json(_start_payload(session_id, protocol_version=99))
        error = ws.receive_json()
        assert error == {
            "type": "error",
            "session_id": session_id,
            "code": "unsupported_version",
            "message": error["message"],
            "retryable": False,
        }
        with pytest.raises(WebSocketDisconnect):
            ws.receive_json()


def test_websocket_binary_before_start_is_invalid_message(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_bytes(_pcm_bytes(1600))
        error = ws.receive_json()
        assert error["type"] == "error"
        assert error["code"] == "invalid_message"
        assert error["session_id"] is None


def test_websocket_too_long_returns_error(tmp_path) -> None:
    app, token = _build_app(tmp_path, limits=LimitsSection(max_utterance_s=1, queue_size=2, end_grace_s=5))
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json(_start_payload(session_id))
        assert ws.receive_json()["type"] == "ready"

        # max_utterance_s=1 -> 16000サンプル。20000サンプル分を一度に送って超過させる。
        ws.send_bytes(_pcm_bytes(20000))
        error = ws.receive_json()
        assert error["type"] == "error"
        assert error["code"] == "too_long"


def test_websocket_busy_when_no_queue_capacity(tmp_path) -> None:
    """queue_size=0 -> 同時受理数は 1 (処理中1件+キュー0件)。2件目は即 busy。"""
    app, token = _build_app(tmp_path, limits=LimitsSection(max_utterance_s=120, queue_size=0, end_grace_s=5))
    client = TestClient(app)
    session_a = str(uuid.uuid4())
    session_b = str(uuid.uuid4())
    headers = {"Authorization": f"Bearer {token}"}
    with client.websocket_connect("/v1/dictate", headers=headers) as ws_a:
        ws_a.send_json(_start_payload(session_a))
        assert ws_a.receive_json()["type"] == "ready"

        with client.websocket_connect("/v1/dictate", headers=headers) as ws_b:
            ws_b.send_json(_start_payload(session_b))
            error = ws_b.receive_json()
            assert error["type"] == "error"
            assert error["code"] == "busy"
            assert error["retryable"] is True


def test_websocket_busy_respects_queue_size_plus_one(tmp_path) -> None:
    """queue_size=1 -> 同時受理数は 2 (処理中1件+キュー1件)。3件目で busy になる。"""
    app, token = _build_app(tmp_path, limits=LimitsSection(max_utterance_s=120, queue_size=1, end_grace_s=5))
    client = TestClient(app)
    session_a, session_b, session_c = (str(uuid.uuid4()) for _ in range(3))
    headers = {"Authorization": f"Bearer {token}"}
    with (
        client.websocket_connect("/v1/dictate", headers=headers) as ws_a,
        client.websocket_connect("/v1/dictate", headers=headers) as ws_b,
    ):
        ws_a.send_json(_start_payload(session_a))
        assert ws_a.receive_json()["type"] == "ready"
        ws_b.send_json(_start_payload(session_b))
        assert ws_b.receive_json()["type"] == "ready"  # 2件目もキュー1件分で受理される

        with client.websocket_connect("/v1/dictate", headers=headers) as ws_c:
            ws_c.send_json(_start_payload(session_c))
            error = ws_c.receive_json()
            assert error["code"] == "busy"
            assert error["retryable"] is True


def test_websocket_unknown_session_on_end(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json({"type": "end", "session_id": str(uuid.uuid4())})
        error = ws.receive_json()
        assert error["code"] == "unknown_session"


def test_websocket_cancel_produces_no_response(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json(_start_payload(session_id))
        assert ws.receive_json()["type"] == "ready"
        ws.send_json({"type": "cancel", "session_id": session_id})

        # cancel 後は何も返らない。念のため別の正常な発話が続けられることを確認する。
        session_id_2 = str(uuid.uuid4())
        ws.send_json(_start_payload(session_id_2))
        ready = ws.receive_json()
        assert ready == {"type": "ready", "session_id": session_id_2}


def test_release_only_decrements_in_flight_once_for_same_session(tmp_path) -> None:
    """`end` 直後の `cancel` で `_finalize` と `_handle_cancel` の両方が release() を呼んでも

    二重解放されない (release() は pending から実際に取り除けたときだけ解放する)。
    """
    built_app, _ = _build_app(tmp_path, limits=LimitsSection(queue_size=0))
    state = built_app.state.voice
    assert state.in_flight.try_acquire()  # 1件分確保 (capacity=1 を使い切る)

    class _FakeWebSocket:
        pass

    fake_websocket = _FakeWebSocket()
    fake_websocket.app = built_app

    conn = ConnectionContext(fake_websocket, state)
    conn.pending["session-x"] = object()

    conn.release("session-x")  # 1回目 (例: _handle_cancel): pop 成功 -> release される
    conn.release("session-x")  # 2回目 (例: _finalize の finally): pop は None -> release されない

    assert state.in_flight._count == 0
    assert state.in_flight.try_acquire()  # 過剰解放されていれば意図せず複数回 acquire できてしまう


def test_websocket_end_then_immediate_cancel_releases_in_flight_exactly_once(tmp_path) -> None:
    app, token = _build_app(tmp_path, limits=LimitsSection(max_utterance_s=120, queue_size=0, end_grace_s=5))
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    headers = {"Authorization": f"Bearer {token}"}
    with client.websocket_connect("/v1/dictate", headers=headers) as ws:
        ws.send_json(_start_payload(session_id))
        assert ws.receive_json()["type"] == "ready"
        ws.send_bytes(_pcm_bytes(1600))
        ws.send_json({"type": "end", "session_id": session_id})
        ws.send_json({"type": "cancel", "session_id": session_id})

        _wait_until(lambda: app.state.voice.in_flight._count == 0)

        # 解放が正しく1回だけ行われたことを、次の発話が受理されることで確認する
        # (queue_size=0 なので、解放されていなければ即 busy になるはず)。
        session_id_2 = str(uuid.uuid4())
        ws.send_json(_start_payload(session_id_2))
        ready_or_error = ws.receive_json()
        assert ready_or_error == {"type": "ready", "session_id": session_id_2}


def test_websocket_binary_odd_length_is_invalid_message_and_keeps_connection(tmp_path) -> None:
    app, token = _build_app(tmp_path)
    client = TestClient(app)
    session_id = str(uuid.uuid4())
    with client.websocket_connect("/v1/dictate", headers={"Authorization": f"Bearer {token}"}) as ws:
        ws.send_json(_start_payload(session_id))
        assert ws.receive_json()["type"] == "ready"

        ws.send_bytes(b"\x01\x02\x03")  # 奇数バイト -> PCM16として解釈できない
        error = ws.receive_json()
        assert error["type"] == "error"
        assert error["code"] == "invalid_message"
        assert error["session_id"] == session_id

        # 接続は維持されている: 正常なフレームを送って end まで完了できる
        ws.send_bytes(_pcm_bytes(1600))
        ws.send_json({"type": "end", "session_id": session_id})
        partial = ws.receive_json()
        assert partial["type"] == "partial"
        final = ws.receive_json()
        assert final["type"] == "final"


def test_dictionary_put_rejects_oversized_body(tmp_path) -> None:
    app, token = _build_app(tmp_path, limits=LimitsSection(max_dictionary_body_bytes=10))
    client = TestClient(app)
    headers = {"Authorization": f"Bearer {token}"}
    payload = {"terms": [{"surface": "Rust", "aliases": ["ラスト"], "replace": True}]}
    response = client.put("/v1/dictionary", json=payload, headers=headers)
    assert response.status_code == 413


def test_dictionary_put_within_body_limit_still_succeeds(tmp_path) -> None:
    app, token = _build_app(tmp_path, limits=LimitsSection(max_dictionary_body_bytes=1_048_576))
    client = TestClient(app)
    headers = {"Authorization": f"Bearer {token}"}
    payload = {"terms": [{"surface": "Rust", "aliases": ["ラスト"], "replace": True}]}
    response = client.put("/v1/dictionary", json=payload, headers=headers)
    assert response.status_code == 200
