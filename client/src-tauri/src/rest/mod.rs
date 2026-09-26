//! `GET/PUT /v1/dictionary` (design.md §4.7, §4.4) の最小 HTTPS クライアント。
//!
//! WebSocket と同じ証明書ピン留め ([`crate::ws::verifier::FingerprintVerifier`]) を使う。
//! reqwest 等の汎用 HTTP クライアントを増やす代わりに、HTTP/1.1 を直接組み立てる
//! (依存を増やさず、TLS 検証ロジックを ws モジュールと共有できるため)。

use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
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

/// `/v1/dictionary` に加えて `/v1/pair` もこの関数を経由する。`path` はエンドポイントの
/// 絶対パス、`token` は `Authorization: Bearer` を付けるかどうか (`/v1/pair` は無認証、design.md
/// 上のサーバー契約通り)。
async fn request(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>), RestError> {
    let mut stream = connect_tls(host, port, fingerprint_hex).await?;

    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
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
    let (status, body) = request(host, port, fingerprint_hex, "GET", "/v1/dictionary", Some(token), None).await?;
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
    let (status, _) = request(
        host,
        port,
        fingerprint_hex,
        "PUT",
        "/v1/dictionary",
        Some(token),
        Some(&body),
    )
    .await?;
    if status != 200 {
        return Err(RestError::HttpStatus { status });
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct PairRequest<'a> {
    device: &'a str,
    /// サーバーがコード不要 (`{"device": str}` のみで 200 を返す) 運用になったため既定で送らない。
    /// サーバーが code モードのときの手動経路用に残す。
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
}

/// `POST /v1/pair` の成功応答 (サーバー契約: 無認証、成功 200、失敗 403)。
#[derive(Debug, Clone, Deserialize)]
pub struct PairResponse {
    pub device: String,
    pub token: String,
    /// コロン区切り・大文字 (例 `8C:4D:...:88`)。呼び出し側で `normalize_fingerprint` を通すこと。
    pub fingerprint: String,
}

/// `POST /v1/pair` (無認証)。端末名 (と、サーバーが code モードのときはペアリングコード) から
/// トークンを発行してもらう (design.md 上のサーバー契約、実装は `voice-server` 側)。現行の
/// サーバー運用は `code` 不要 (`{"device": str}` だけで 200 を返す) だが、手動経路用に
/// `code: Option<&str>` を残す。TLS は呼び出し側が TOFU で取得したフィンガープリントで固定する
/// — まだ承認済みの値ではないため、応答の `fingerprint` が同じ値であることを呼び出し側で
/// 必ず検証すること。
pub async fn pair(
    host: &str,
    port: u16,
    fingerprint_hex: &str,
    device: &str,
    code: Option<&str>,
) -> Result<PairResponse, RestError> {
    let body = serde_json::to_vec(&PairRequest { device, code })?;
    let (status, resp_body) = request(host, port, fingerprint_hex, "POST", "/v1/pair", None, Some(&body)).await?;
    if status != 200 {
        return Err(RestError::HttpStatus { status });
    }
    Ok(serde_json::from_slice(&resp_body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_request_omits_code_field_when_none() {
        let req = PairRequest { device: "my-mac", code: None };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"device":"my-mac"}"#);
    }

    #[test]
    fn pair_request_includes_code_field_when_some() {
        let req = PairRequest { device: "my-mac", code: Some("123456") };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"device":"my-mac","code":"123456"}"#);
    }

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

    /// `pair` (`POST /v1/pair`) が固定 TLS 越しに送受信できることを、ローカルの TLS
    /// モックサーバー (`ws::tests` と同様に rcgen の自己署名証明書を使う) で確認する。
    /// - 送信されたリクエストが無認証 (`Authorization` ヘッダーが無い) であること
    /// - 応答 (device/token/fingerprint) を正しくデシリアライズできること
    #[tokio::test]
    async fn pair_round_trips_over_pinned_tls_without_auth_header() {
        use rcgen::{generate_simple_self_signed, CertifiedKey};
        use rustls::pki_types::PrivatePkcs8KeyDer;
        use rustls::ServerConfig;
        use tokio::net::TcpListener;
        use tokio_rustls::TlsAcceptor;

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("自己署名証明書の生成");
        let cert_der = cert.der().clone();
        let fingerprint = crate::ws::verifier::fingerprint_hex(&cert_der);
        let key_der = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("サーバー TLS 設定");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");

            let mut raw = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let n = tls.read(&mut chunk).await.expect("読み取り");
                raw.extend_from_slice(&chunk[..n]);
                if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&raw).to_string();
            assert!(request_text.starts_with("POST /v1/pair HTTP/1.1"));
            assert!(
                !request_text.to_ascii_lowercase().contains("authorization:"),
                "/v1/pair は無認証のはずなのに Authorization ヘッダーが付いている: {request_text}"
            );
            assert!(request_text.contains("\"device\":\"my-mac\""));
            assert!(
                !request_text.contains("\"code\""),
                "code 不要のはずなのに code フィールドが送られている: {request_text}"
            );

            let body = br#"{"device":"my-mac","token":"tok-abc123","fingerprint":"8C:4D:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:88"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            tls.write_all(response.as_bytes()).await.expect("応答ヘッダー送信");
            tls.write_all(body).await.expect("応答ボディ送信");
            tls.shutdown().await.expect("TLS シャットダウン");
        });

        let response = pair(&addr.ip().to_string(), addr.port(), &fingerprint, "my-mac", None)
            .await
            .expect("pair が成功すること");
        assert_eq!(response.device, "my-mac");
        assert_eq!(response.token, "tok-abc123");
        assert_eq!(response.fingerprint, "8C:4D:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:88");

        server_task.await.expect("server task panicked");
    }

    /// サーバーが 403 (コード不正・期限切れ) を返したら `RestError::HttpStatus` になること。
    #[tokio::test]
    async fn pair_rejects_403_as_http_status_error() {
        use rcgen::{generate_simple_self_signed, CertifiedKey};
        use rustls::pki_types::PrivatePkcs8KeyDer;
        use rustls::ServerConfig;
        use tokio::net::TcpListener;
        use tokio_rustls::TlsAcceptor;

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("自己署名証明書の生成");
        let cert_der = cert.der().clone();
        let fingerprint = crate::ws::verifier::fingerprint_hex(&cert_der);
        let key_der = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("サーバー TLS 設定");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(tcp).await.expect("tls accept");
            let mut chunk = [0u8; 4096];
            let _ = tls.read(&mut chunk).await.expect("読み取り");

            let body = b"{}";
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            tls.write_all(response.as_bytes()).await.expect("応答ヘッダー送信");
            tls.write_all(body).await.expect("応答ボディ送信");
            tls.shutdown().await.expect("TLS シャットダウン");
        });

        let result = pair(&addr.ip().to_string(), addr.port(), &fingerprint, "my-mac", None).await;
        assert!(matches!(result, Err(RestError::HttpStatus { status: 403 })));

        server_task.await.expect("server task panicked");
    }
}
