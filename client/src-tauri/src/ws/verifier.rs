//! サーバー証明書の SHA-256 フィンガープリント固定検証 (design.md §3.1, §5.1)。
//!
//! 自宅LAN内・自己署名証明書という前提のため、通常の CA チェーン検証は行わない。
//! 代わりに「初回接続時にユーザーが承認したフィンガープリントと一致するか」だけを見る。
//! 署名自体の暗号学的検証は rustls のデフォルト provider にそのまま委譲する
//! (検証を丸ごと無効化するわけではない)。

use std::fmt;
use std::sync::{Arc, Mutex};

use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// 証明書 (DER) の SHA-256 フィンガープリントを 16進小文字・コロン無しで表す。
/// `normalize_fingerprint` を通した値と直接比較できる形式。
pub fn fingerprint_hex(der: &CertificateDer<'_>) -> String {
    let digest = Sha256::digest(der.as_ref());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FingerprintFormatError {
    #[error("フィンガープリントは 64 桁の16進数でなければならない (コロン・空白を除いた実際の文字数: {0})")]
    WrongLength(usize),
    #[error("フィンガープリントに16進数以外の文字が含まれている: '{0}'")]
    InvalidChar(char),
}

/// `voice-server cert fingerprint` はコロン区切り・大文字 (例 `8C:4D:17:...`) で表示するが、
/// [`fingerprint_hex`] はコロン無し・小文字を返す。両方の入力形式を同じ 64 桁小文字16進へ揃える。
/// コロン・空白 (前後含む) を除去したうえで、64 桁の16進数であることを検証する。
///
/// `approve_fingerprint` (commands.rs)・[`FingerprintVerifier::new`]・UI の貼り付け経路すべてが
/// この関数を経由することで、表記ゆれによる「承認したのに一致しない」不具合を防ぐ。
pub fn normalize_fingerprint(input: &str) -> Result<String, FingerprintFormatError> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if cleaned.chars().count() != 64 {
        return Err(FingerprintFormatError::WrongLength(cleaned.chars().count()));
    }
    let mut normalized = String::with_capacity(64);
    for c in cleaned.chars() {
        if !c.is_ascii_hexdigit() {
            return Err(FingerprintFormatError::InvalidChar(c));
        }
        normalized.push(c.to_ascii_lowercase());
    }
    Ok(normalized)
}

/// [`normalize_fingerprint`] の逆方向: 内部形式 (コロン無し・小文字、64桁) を
/// UI 表示形式 (コロン区切り・大文字、`voice-server cert fingerprint` と同じ見た目) へ変換する。
/// `Status.fingerprint` (UI 契約) はこの形式で返す。
pub fn to_colon_upper(hex_lower_no_colon: &str) -> String {
    hex_lower_no_colon
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| std::str::from_utf8(pair).ok())
        .map(|s| s.to_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

/// 定数時間比較 (タイミング攻撃対策)。両者の長さが違う時点で不一致確定なので、その比較自体は
/// (秘密の内容に依存しないので) 定数時間でなくてよい。
fn constant_time_str_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub struct FingerprintVerifier {
    expected_fingerprint_hex: String,
    provider: Arc<CryptoProvider>,
}

impl fmt::Debug for FingerprintVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FingerprintVerifier")
            .field("expected_fingerprint_hex", &self.expected_fingerprint_hex)
            .finish()
    }
}

impl FingerprintVerifier {
    /// `expected_fingerprint_hex` はユーザーが承認した SHA-256。コロン区切り・大文字を含む
    /// 生の表示形式のままでもよい ([`normalize_fingerprint`] で内部的に正規化する)。
    pub fn new(expected_fingerprint_hex: &str) -> Result<Self, FingerprintFormatError> {
        Ok(Self {
            expected_fingerprint_hex: normalize_fingerprint(expected_fingerprint_hex)?,
            provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
        })
    }
}

impl ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let actual = fingerprint_hex(end_entity);
        if constant_time_str_eq(&actual, &self.expected_fingerprint_hex) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::General(format!(
                "証明書フィンガープリント不一致 (期待: {}, 実際: {actual})",
                self.expected_fingerprint_hex
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 初回接続時 (TOFU: Trust On First Use) にサーバー証明書のフィンガープリントを
/// 「見るだけ」で承認前提の検証は一切行わない検証器 (design.md §3.1, §5.8)。
///
/// **危険**: 実際の検証は一切しない。ユーザーが表示された値をサーバー側の
/// `voice-server cert fingerprint` の出力と目で見比べて承認するまでの、
/// 一度だけの「値を取得する」用途に限定して使う。承認後は必ず
/// [`FingerprintVerifier`] (実際に照合する) へ切り替える。
pub struct CapturingVerifier {
    captured: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl fmt::Debug for CapturingVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapturingVerifier").finish()
    }
}

impl CapturingVerifier {
    pub fn new() -> (Self, Arc<Mutex<Option<String>>>) {
        let captured = Arc::new(Mutex::new(None));
        (
            Self {
                captured: captured.clone(),
                provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            },
            captured,
        )
    }
}

impl ServerCertVerifier for CapturingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let fp = fingerprint_hex(end_entity);
        if let Ok(mut slot) = self.captured.lock() {
            *slot = Some(fp);
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_hex_is_64_lowercase_hex_chars() {
        let der = CertificateDer::from(vec![1, 2, 3, 4]);
        let fp = fingerprint_hex(&der);
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn fingerprint_hex_is_deterministic() {
        let der = CertificateDer::from(vec![9, 9, 9]);
        assert_eq!(fingerprint_hex(&der), fingerprint_hex(&der));
    }

    #[test]
    fn to_colon_upper_round_trips_with_normalize_fingerprint() {
        let lower = "8c4d17".to_string() + &"00".repeat(29);
        let colon_upper = to_colon_upper(&lower);
        assert_eq!(colon_upper, "8C:4D:17".to_string() + &":00".repeat(29));
        assert_eq!(normalize_fingerprint(&colon_upper).unwrap(), lower);
    }

    #[test]
    fn mismatched_fingerprint_is_rejected() {
        let der = CertificateDer::from(vec![1, 2, 3]);
        let verifier = FingerprintVerifier::new(&"00".repeat(32)).expect("valid format");
        let server_name = ServerName::try_from("localhost").unwrap();
        let result = verifier.verify_server_cert(&der, &[], &server_name, &[], UnixTime::now());
        assert!(result.is_err());
    }

    #[test]
    fn matching_fingerprint_is_accepted() {
        let der = CertificateDer::from(vec![1, 2, 3]);
        let fp = fingerprint_hex(&der);
        let verifier = FingerprintVerifier::new(&fp).expect("valid format");
        let server_name = ServerName::try_from("localhost").unwrap();
        let result = verifier.verify_server_cert(&der, &[], &server_name, &[], UnixTime::now());
        assert!(result.is_ok());
    }

    /// バグの本体: サーバー CLI が出す「コロン区切り・大文字」形式で承認しても、
    /// 実際の証明書の SHA-256 (コロン無し・小文字) と一致すること。
    #[test]
    fn matching_fingerprint_is_accepted_even_when_given_in_colon_separated_uppercase_form() {
        let der = CertificateDer::from(vec![1, 2, 3]);
        let fp_lower = fingerprint_hex(&der);
        let colon_upper: String = fp_lower
            .as_bytes()
            .chunks(2)
            .map(|pair| std::str::from_utf8(pair).unwrap().to_uppercase())
            .collect::<Vec<_>>()
            .join(":");

        let verifier = FingerprintVerifier::new(&colon_upper).expect("valid format");
        let server_name = ServerName::try_from("localhost").unwrap();
        let result = verifier.verify_server_cert(&der, &[], &server_name, &[], UnixTime::now());
        assert!(result.is_ok(), "コロン区切り大文字の承認値でも一致するはず");
    }

    #[test]
    fn new_rejects_malformed_fingerprint() {
        assert!(FingerprintVerifier::new("not-hex-at-all").is_err());
        assert!(FingerprintVerifier::new(&"ab".repeat(10)).is_err()); // 短すぎる
    }

    #[test]
    fn normalize_fingerprint_strips_colons_and_lowercases() {
        let colon_upper = "8C:4D:17:00:00:00:00:00:00:00:00:00:00:00:00:00:\
                            00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:88";
        let plain_lower = "8c4d170000000000000000000000000000000000000000000000000000000088";
        assert_eq!(normalize_fingerprint(colon_upper).unwrap(), plain_lower);
    }

    #[test]
    fn normalize_fingerprint_accepts_surrounding_whitespace() {
        let input = "  8c4d170000000000000000000000000000000000000000000000000000000088  \n";
        assert_eq!(
            normalize_fingerprint(input).unwrap(),
            "8c4d170000000000000000000000000000000000000000000000000000000088"
        );
    }

    #[test]
    fn normalize_fingerprint_rejects_wrong_length() {
        let err = normalize_fingerprint("8c4d17").unwrap_err();
        assert_eq!(err, FingerprintFormatError::WrongLength(6));
    }

    #[test]
    fn normalize_fingerprint_rejects_non_hex_characters() {
        let too_long_but_has_g = "g".repeat(64);
        let err = normalize_fingerprint(&too_long_but_has_g).unwrap_err();
        assert_eq!(err, FingerprintFormatError::InvalidChar('g'));
    }

    #[test]
    fn capturing_verifier_records_fingerprint_and_always_accepts() {
        let der = CertificateDer::from(vec![9, 8, 7]);
        let (verifier, captured) = CapturingVerifier::new();
        let server_name = ServerName::try_from("localhost").unwrap();
        let result = verifier.verify_server_cert(&der, &[], &server_name, &[], UnixTime::now());
        assert!(result.is_ok());
        assert_eq!(captured.lock().unwrap().as_deref(), Some(fingerprint_hex(&der).as_str()));
    }
}
