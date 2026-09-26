"""サーバー設定 (server.yaml) の読み込みとスキーマ。

設計書 §4 の既定値をここに集約する。値のハードコードを避け、全てこの
モジュールを経由して読む (code-quality.md: config化)。
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Literal

import yaml
from pydantic import BaseModel, Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class _PathSection(BaseModel):
    """`~` を含むパスを YAML から読んだときも展開する (既定値だけでなく設定値も)。"""

    @field_validator("*", mode="before")
    @classmethod
    def _expand_user(cls, value: Any) -> Any:
        if isinstance(value, str) and value.startswith("~"):
            return Path(value).expanduser()
        return value


class ServerSection(_PathSection):
    bind: str = "0.0.0.0"
    port: int = 8765
    tls_cert_path: Path = Path("~/voice/config/tls/server.crt").expanduser()
    tls_key_path: Path = Path("~/voice/config/tls/server.key").expanduser()


class TokensSection(_PathSection):
    path: Path = Path("~/voice/config/tokens.yaml").expanduser()


class DictionarySection(_PathSection):
    path: Path = Path("~/voice/config/dictionary.yaml").expanduser()


class AsrSection(_PathSection):
    backend: Literal["faster_whisper", "qwen3_asr", "cohere_transcribe", "dummy"] = "faster_whisper"
    model: str = "large-v3-turbo"
    model_path: Path | None = None
    strategy: Literal["segmented", "whole"] = "segmented"
    context_chars: int = 100
    compute_type: str = "float16"
    device: str = "cuda"


class VadSection(BaseModel):
    silence_ms: int = 600
    max_segment_s: int = 15
    min_segment_s: int = 1


class LlmSection(BaseModel):
    base_url: str = "http://127.0.0.1:8081"
    timeout_base_ms: int = 1500
    timeout_per_char_ms: int = 15
    temperature: float = 0.1
    # 実機 (列 D) 確認済み: llama-server は --reasoning-budget 0 -rea off で起動しており、
    # レスポンスの content には清書結果のみが入る。出力ガードの <think> 除去は保険として残す。
    model: str = "Qwen3.5-4B-Q4_K_M.gguf"


class LimitsSection(BaseModel):
    max_utterance_s: int = 120
    queue_size: int = 2
    end_grace_s: int = 5
    max_dictionary_body_bytes: int = 1_048_576


class RecordingSection(_PathSection):
    enabled: bool = False
    dir: Path = Path("~/voice/data").expanduser()


class LoggingSection(BaseModel):
    level: str = "INFO"


class ServerConfig(BaseSettings):
    """`server.yaml` に対応する設定。値は設計書 §4 の既定値を維持する。"""

    model_config = SettingsConfigDict(extra="forbid")

    server: ServerSection = Field(default_factory=ServerSection)
    tokens: TokensSection = Field(default_factory=TokensSection)
    dictionary: DictionarySection = Field(default_factory=DictionarySection)
    asr: AsrSection = Field(default_factory=AsrSection)
    vad: VadSection = Field(default_factory=VadSection)
    llm: LlmSection = Field(default_factory=LlmSection)
    limits: LimitsSection = Field(default_factory=LimitsSection)
    recording: RecordingSection = Field(default_factory=RecordingSection)
    logging: LoggingSection = Field(default_factory=LoggingSection)

    @classmethod
    def load(cls, path: Path | str) -> ServerConfig:
        """YAML ファイルから設定を読む。存在しない場合は既定値のみで構築する。"""
        path = Path(path)
        if not path.exists():
            raise FileNotFoundError(f"設定ファイルが見つかりません: {path}")
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
        return cls.model_validate(raw)
