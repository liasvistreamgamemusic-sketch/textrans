"""共有プロトコル定義 (`protocol/v1/*.schema.json`) の読み込みと検証。

`protocol/` がプロトコルの正本 (`protocol/README.md`)。サーバー側でメッセージ形を
再定義せず、このモジュールを経由してスキーマを読み込み検証する。
"""

from __future__ import annotations

import json
import os
from functools import lru_cache
from pathlib import Path

import jsonschema

_PROTOCOL_DIR_ENV = "VOICE_SERVER_PROTOCOL_DIR"


def find_protocol_v1_dir() -> Path:
    """`protocol/v1/` を探す。環境変数で明示的に指定できる (deploy 時の配置差異対策)。"""
    override = os.environ.get(_PROTOCOL_DIR_ENV)
    if override:
        candidate = Path(override)
        if candidate.is_dir():
            return candidate
        raise FileNotFoundError(f"{_PROTOCOL_DIR_ENV}={override} はディレクトリではありません")

    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "protocol" / "v1"
        if candidate.is_dir():
            return candidate
    raise FileNotFoundError(
        "protocol/v1 が見つかりません。モノレポ構成 (server/ の上位に protocol/) で実行するか、"
        f"環境変数 {_PROTOCOL_DIR_ENV} でディレクトリを指定してください。"
    )


@lru_cache(maxsize=1)
def _schema_store() -> dict[str, dict]:
    v1_dir = find_protocol_v1_dir()
    store: dict[str, dict] = {}
    for path in sorted(v1_dir.glob("*.schema.json")):
        schema = json.loads(path.read_text(encoding="utf-8"))
        store[path.name] = schema
        schema_id = schema.get("$id")
        if schema_id:
            store[schema_id] = schema
    return store


def load_schema(name: str) -> dict:
    """`name` (拡張子・パス無し。例: "start") のスキーマを返す。"""
    store = _schema_store()
    key = f"{name}.schema.json"
    if key not in store:
        raise KeyError(f"未知のスキーマ: {name}")
    return store[key]


def _resolver_for(schema: dict) -> jsonschema.RefResolver:
    store = _schema_store()
    base_uri = schema.get("$id", "")
    return jsonschema.RefResolver(base_uri=base_uri, referrer=schema, store=store)


def validate_message(name: str, instance: dict) -> None:
    """`instance` がスキーマ `name` に違反する場合 `jsonschema.ValidationError` を投げる。"""
    schema = load_schema(name)
    resolver = _resolver_for(schema)
    validator = jsonschema.Draft202012Validator(schema, resolver=resolver)
    validator.validate(instance)


def is_valid_message(name: str, instance: dict) -> bool:
    try:
        validate_message(name, instance)
    except jsonschema.ValidationError:
        return False
    return True
