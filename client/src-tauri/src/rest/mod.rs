//! `GET/PUT /v1/dictionary` (design.md §4.7, §4.4) の最小 HTTPS クライアント。
//!
//! WebSocket と同じ証明書ピン留め ([`crate::ws::verifier::FingerprintVerifier`]) を使う。
//! reqwest 等の汎用 HTTP クライアントを増やす代わりに、HTTP/1.1 を直接組み立てる
//! (依存を増やさず、TLS 検証ロジックを ws モジュールと共有できるため)。

use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::protocol::Dictionary;
use crate::ws::verifier::FingerprintVerifier;

#[derive(Debug, thiserror::Error)]
pub enum RestError {
    #[error("接続に失敗: {0}")]
    Connect(#[source] std::io::Error),
    #[error("TLS ハンドシェイクに失敗: {0}")]
    Tls(#[source] std::io::Error),
    #[error("送受信に失敗: {0}")]
    Io(#[source] std::io::Error),
    #[error("サーバーからの応答を解釈できない: {0}")]
    MalformedResponse(String),
    #[error("サーバーがエラーを返した: HTTP {status}")]
    HttpStatus { status: u16 },
    #[error("JSON の変換に失敗: {0}")]
    Json(#[from] serde_json::Error),
}

/// `wss://host:port` 形式から接続先ホスト名とポートを取り出す。REST も同じホストの `8765` を使う。
pub fn host_and_port(server_url: &str) -> Result<(String, u16), RestError> {
    let without_scheme = server_url
        .strip_prefix("wss://")
        .or_else(|| server_url.strip_prefix("https://"))
        .ok_or_else(|| RestError::MalformedResponse(format!("未対応のスキーム: {server_url}")))?;
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    match host_port.rsplit_once(':') {
        Some((host, port)) => {
            let port: u16 = port
                .parse()
                .map_err(|_| RestError::MalformedResponse(format!("不正なポート: {port}")))?;
            Ok((host.to_string(), port))
        }
        None => Ok((host_port.to_string(), 8765)),
    }
}

/// HTTP レスポンスをステータスコードとボディに分解する (Content-Length 優先、無ければ EOF まで)。
pub fn parse_http_response(raw: &[u8]) -> Result<(u16, Vec<u8>), RestError> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| RestError::MalformedResponse("ヘッダー終端が無い".to_string()))?;
    let header_text = std::str::from_utf8(&raw[..header_end])
        .map_err(|e| RestError::MalformedResponse(e.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| RestError::MalformedResponse("ステータス行が無い".to_string()))?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| RestError::MalformedResponse(format!("ステータス行を解釈できない: {status_line}")))?;

    let body_start = header_end + 4;
    let body = &raw[body_start..];

    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok());

    let body = match content_length {
        Some(len) if len <= body.len() => body[..len].to_vec(),
        _ => body.to_vec(),
    };
    Ok((status, body))
}

async fn connect_tls(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, RestError> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(RestError::Connect)?;
    let verifier = FingerprintVerifier::new(fingerprint_hex)
        .map_err(|e| RestError::MalformedResponse(e.to_string()))?;
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|e| RestError::MalformedResponse(e.to_string()))?;
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| RestError::Tls(std::io::Error::other(e)))
}

async fn request(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
    token: &str,
    method: &str,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>), RestError> {
    let mut stream = connect_tls(host, port, fingerprint_hex).await?;

    let mut request = format!(
        "{method} /v1/dictionary HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n"
    );
    if let Some(b) = body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(RestError::Io)?;
    if let Some(b) = body {
        stream.write_all(b).await.map_err(RestError::Io)?;
    }

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.map_err(RestError::Io)?;
    parse_http_response(&raw)
}

pub async fn get_dictionary(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
    token: &str,
) -> Result<Dictionary, RestError> {
    let (status, body) = request(host, port, fingerprint_hex, token, "GET", None).await?;
    if status != 200 {
        return Err(RestError::HttpStatus { status });
    }
    Ok(serde_json::from_slice(&body)?)
}

pub async fn put_dictionary(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
    token: &str,
    dictionary: &Dictionary,
) -> Result<(), RestError> {
    let body = serde_json::to_vec(dictionary)?;
    let (status, _) = request(host, port, fingerprint_hex, token, "PUT", Some(&body)).await?;
    if status != 200 {
        return Err(RestError::HttpStatus { status });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_port_parses_wss_url() {
        let (host, port) = host_and_port("wss://192.168.11.10:8765").unwrap();
        assert_eq!(host, "192.168.11.10");
        assert_eq!(port, 8765);
    }

    #[test]
    fn host_and_port_defaults_when_no_port() {
        let (host, port) = host_and_port("wss://voice.local").unwrap();
        assert_eq!(host, "voice.local");
        assert_eq!(port, 8765);
    }

    #[test]
    fn host_and_port_rejects_unknown_scheme() {
        assert!(host_and_port("ftp://example.com").is_err());
    }

    #[test]
    fn parse_http_response_reads_status_and_content_length_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello TRAILING GARBAGE";
        let (status, body) = parse_http_response(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello");
    }

    #[test]
    fn parse_http_response_falls_back_to_full_body_without_content_length() {
        let raw = b"HTTP/1.1 401 Unauthorized\r\n\r\n{}";
        let (status, body) = parse_http_response(raw).unwrap();
        assert_eq!(status, 401);
        assert_eq!(body, b"{}");
    }

    #[test]
    fn parse_http_response_rejects_missing_header_terminator() {
        assert!(parse_http_response(b"not an http response").is_err());
    }
}
