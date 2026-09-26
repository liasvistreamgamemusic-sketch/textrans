//! WebSocket プロトコル (protocol_version = 1) のメッセージ型。
//!
//! 正本は `protocol/v1/*.schema.json` と `protocol/README.md`。
//! ここでの型定義はスキーマの写しであり、変更する場合は両方を同時に直す。
//!
//! 実装メモ: `#[serde(tag = "type")]` の内部タグ付き enum で newtype variant
//! (`Variant(PayloadStruct)`) を使うと、タグ自身がペイロード側の
//! `deny_unknown_fields` に未知フィールドとして引っかかる (serde の既知の相互作用)。
//! そのため各メッセージは struct variant として直接フィールドを持つ。

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// クライアントが対応するプロトコル版数 (`protocol/README.md` の版数表)。
pub const PROTOCOL_VERSION: u32 = 1;

/// `sample_rate` は常に 16000 (`start.schema.json` の `const: 16000`)。
/// 誤った値からの誤動作を防ぐため、値自体を型で固定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleRate16k;

impl Serialize for SampleRate16k {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(16_000)
    }
}

impl<'de> Deserialize<'de> for SampleRate16k {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value == 16_000 {
            Ok(SampleRate16k)
        } else {
            Err(de::Error::custom(format!(
                "sample_rate は 16000 固定 (受け取った値: {value})"
            )))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Clean,
    Raw,
    TranslateEn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    LlmSkipped,
    LlmRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    TooLong,
    Busy,
    UnsupportedVersion,
    InvalidMessage,
    UnknownSession,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timings {
    pub asr_ms: u32,
    pub llm_ms: u32,
    pub total_ms: u32,
}

/// C→S メッセージ (テキストフレーム)。バイナリフレーム (PCM) は別経路 (`ws` モジュール) で扱う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Start {
        session_id: Uuid,
        protocol_version: u32,
        mode: Mode,
        sample_rate: SampleRate16k,
    },
    End {
        session_id: Uuid,
    },
    Cancel {
        session_id: Uuid,
    },
}

impl ClientMessage {
    pub fn start(session_id: Uuid, mode: Mode) -> Self {
        ClientMessage::Start {
            session_id,
            protocol_version: PROTOCOL_VERSION,
            mode,
            sample_rate: SampleRate16k,
        }
    }

    pub fn end(session_id: Uuid) -> Self {
        ClientMessage::End { session_id }
    }

    pub fn cancel(session_id: Uuid) -> Self {
        ClientMessage::Cancel { session_id }
    }

    pub fn session_id(&self) -> Uuid {
        match self {
            ClientMessage::Start { session_id, .. }
            | ClientMessage::End { session_id }
            | ClientMessage::Cancel { session_id } => *session_id,
        }
    }
}

/// S→C メッセージ (テキストフレーム)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    Ready {
        session_id: Uuid,
    },
    Partial {
        session_id: Uuid,
        seq: u32,
        text: String,
    },
    Final {
        session_id: Uuid,
        raw_text: String,
        text: String,
        mode: Mode,
        flags: Vec<Flag>,
        timings: Timings,
    },
    Error {
        session_id: Option<Uuid>,
        code: ErrorCode,
        message: String,
        retryable: bool,
    },
}

impl ServerMessage {
    pub fn session_id(&self) -> Option<Uuid> {
        match self {
            ServerMessage::Ready { session_id }
            | ServerMessage::Partial { session_id, .. }
            | ServerMessage::Final { session_id, .. } => Some(*session_id),
            ServerMessage::Error { session_id, .. } => *session_id,
        }
    }

    /// `final` で挿入すべきテキストが無い (空発話) かどうか。§5.3 の `final(空)` 遷移で使う。
    pub fn is_empty_final(&self) -> bool {
        matches!(self, ServerMessage::Final { text, .. } if text.is_empty())
    }
}

// ---- 辞書 (GET/PUT /v1/dictionary、server/config/dictionary.yaml と同形) ----

fn validate_term_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() || s.chars().count() > 100 {
        return Err(de::Error::custom(
            "辞書の表記は 1〜100 文字でなければならない",
        ));
    }
    Ok(s)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DictionaryTerm {
    #[serde(deserialize_with = "validate_term_text")]
    pub surface: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dictionary {
    pub terms: Vec<DictionaryTerm>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn fixtures_dir(kind: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("protocol")
            .join("fixtures")
            .join(kind)
    }

    /// ファイル名の先頭 `_` までがスキーマ名 (`protocol/README.md` の規則)。
    fn schema_name_of(file_name: &str) -> &str {
        let stem = file_name.strip_suffix(".json").unwrap_or(file_name);
        stem.split('_').next().unwrap_or(stem)
    }

    fn try_parse(schema: &str, raw: &str) -> Result<(), String> {
        match schema {
            "start" | "end" | "cancel" => {
                let parsed: ClientMessage = serde_json::from_str(raw).map_err(|e| e.to_string())?;
                let round = serde_json::to_string(&parsed).map_err(|e| e.to_string())?;
                let reparsed: ClientMessage =
                    serde_json::from_str(&round).map_err(|e| e.to_string())?;
                assert_eq!(parsed, reparsed, "round trip で内容が変わった");
                Ok(())
            }
            "ready" | "partial" | "final" | "error" => {
                let parsed: ServerMessage = serde_json::from_str(raw).map_err(|e| e.to_string())?;
                let round = serde_json::to_string(&parsed).map_err(|e| e.to_string())?;
                let reparsed: ServerMessage =
                    serde_json::from_str(&round).map_err(|e| e.to_string())?;
                assert_eq!(parsed, reparsed, "round trip で内容が変わった");
                Ok(())
            }
            "dictionary" => {
                let parsed: Dictionary = serde_json::from_str(raw).map_err(|e| e.to_string())?;
                let round = serde_json::to_string(&parsed).map_err(|e| e.to_string())?;
                let reparsed: Dictionary =
                    serde_json::from_str(&round).map_err(|e| e.to_string())?;
                assert_eq!(parsed, reparsed, "round trip で内容が変わった");
                Ok(())
            }
            other => panic!("未知の fixture スキーマ名: {other}"),
        }
    }

    #[test]
    fn valid_fixtures_round_trip() {
        let dir = fixtures_dir("valid");
        let entries = fs::read_dir(&dir).expect("protocol/fixtures/valid を読めること");
        let mut count = 0;
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
            let schema = schema_name_of(&file_name).to_string();
            let raw = fs::read_to_string(&path).expect("fixture を読めること");
            let result = try_parse(&schema, &raw);
            assert!(
                result.is_ok(),
                "{file_name} ({schema}) は valid fixture のはずが失敗: {result:?}"
            );
            count += 1;
        }
        assert!(count > 0, "valid fixtures が1件も見つからなかった");
    }

    #[test]
    fn invalid_fixtures_are_rejected() {
        let dir = fixtures_dir("invalid");
        let entries = fs::read_dir(&dir).expect("protocol/fixtures/invalid を読めること");
        let mut count = 0;
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
            let schema = schema_name_of(&file_name).to_string();
            let raw = fs::read_to_string(&path).expect("fixture を読めること");
            let result = try_parse(&schema, &raw);
            assert!(
                result.is_err(),
                "{file_name} ({schema}) は invalid fixture のはずが成功した"
            );
            count += 1;
        }
        assert!(count > 0, "invalid fixtures が1件も見つからなかった");
    }

    #[test]
    fn protocol_version_is_1() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
