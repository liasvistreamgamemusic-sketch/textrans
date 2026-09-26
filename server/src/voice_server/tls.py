"""自己署名 TLS 証明書の生成とフィンガープリント表示 (設計書 §3.1)。"""

from __future__ import annotations

import datetime
import ipaddress
import logging
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

logger = logging.getLogger(__name__)

_CERT_VALIDITY_DAYS = 3650
_KEY_SIZE = 2048


def ensure_certificate(cert_path: Path, key_path: Path, common_name: str = "voice-server") -> bool:
    """証明書/鍵が無ければ自己署名証明書を生成する。生成したら True を返す。"""
    cert_path = Path(cert_path)
    key_path = Path(key_path)
    if cert_path.exists() and key_path.exists():
        return False

    cert_path.parent.mkdir(parents=True, exist_ok=True)
    key_path.parent.mkdir(parents=True, exist_ok=True)

    key = rsa.generate_private_key(public_exponent=65537, key_size=_KEY_SIZE)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, common_name)])
    now = datetime.datetime.now(datetime.UTC)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + datetime.timedelta(days=_CERT_VALIDITY_DAYS))
        .add_extension(
            x509.SubjectAlternativeName(
                [x509.DNSName(common_name), x509.IPAddress(ipaddress.ip_address("127.0.0.1"))]
            ),
            critical=False,
        )
        .sign(key, hashes.SHA256())
    )

    key_path.write_bytes(
        key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.TraditionalOpenSSL,
            encryption_algorithm=serialization.NoEncryption(),
        )
    )
    cert_path.write_bytes(cert.public_bytes(serialization.Encoding.PEM))
    for path in (key_path, cert_path):
        try:
            path.chmod(0o600)
        except OSError:
            logger.warning("証明書ファイルの権限設定に失敗しました (path=%s)", path)
    return True


def fingerprint(cert_path: Path) -> str:
    """SHA-256 フィンガープリントをコロン区切り大文字 (例: AB:CD:...) で返す。"""
    cert_path = Path(cert_path)
    if not cert_path.exists():
        raise FileNotFoundError(f"証明書が見つかりません: {cert_path}. 先に `cert ensure` を実行してください")
    cert = x509.load_pem_x509_certificate(cert_path.read_bytes())
    digest = cert.fingerprint(hashes.SHA256())
    return ":".join(f"{byte:02X}" for byte in digest)
