"""`voice-server` CLI (設計書 §4.8): serve / token / cert。"""

from __future__ import annotations

from pathlib import Path

import typer
import uvicorn

from .app import create_app
from .auth import TokenStore
from .config import ServerConfig
from .pairing import PairingStore
from .tls import ensure_certificate
from .tls import fingerprint as compute_fingerprint

app = typer.Typer(help="voice-server: 音声入力システム Gateway")
token_app = typer.Typer(help="端末トークンの発行・失効・一覧")
cert_app = typer.Typer(help="TLS 自己署名証明書")
app.add_typer(token_app, name="token")
app.add_typer(cert_app, name="cert")

_DEFAULT_CONFIG_PATH = Path("config/server.yaml")
_DEFAULT_PAIR_TTL_S = 300


@app.command()
def serve(config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス")) -> None:
    """Gateway (FastAPI + uvicorn) を起動する。"""
    cfg = ServerConfig.load(config)
    ensure_certificate(cfg.server.tls_cert_path, cfg.server.tls_key_path)
    fastapi_app = create_app(cfg)
    uvicorn.run(
        fastapi_app,
        host=cfg.server.bind,
        port=cfg.server.port,
        ssl_certfile=str(cfg.server.tls_cert_path),
        ssl_keyfile=str(cfg.server.tls_key_path),
    )


@app.command()
def pair(
    config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス"),
    ttl: int = typer.Option(_DEFAULT_PAIR_TTL_S, "--ttl", help="コードの有効期限 (秒)"),
) -> None:
    """ペアリング用の6桁コードを生成する。平文コードと有効期限をこの1回だけ表示する。"""
    cfg = ServerConfig.load(config)
    pairing_code = PairingStore(cfg.pairing.path).issue(ttl)
    typer.echo(f"code={pairing_code.code}")
    typer.echo(f"expires_at={pairing_code.expires_at.isoformat()}")
    typer.echo("このコードは端末側で1回だけ入力できます。クライアントでペアリングを行ってください。")


@token_app.command("issue")
def token_issue(
    device_name: str,
    config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス"),
) -> None:
    """端末用トークンを発行する。平文はこの1回だけ表示する。"""
    cfg = ServerConfig.load(config)
    raw_token = TokenStore(cfg.tokens.path).issue(device_name)
    typer.echo(f"device={device_name}")
    typer.echo(f"token={raw_token}")
    typer.echo("この平文トークンは二度と表示されません。クライアント設定に控えてください。")


@token_app.command("revoke")
def token_revoke(
    device_name: str,
    config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス"),
) -> None:
    """端末のトークンを失効させる。"""
    cfg = ServerConfig.load(config)
    revoked = TokenStore(cfg.tokens.path).revoke(device_name)
    if revoked:
        typer.echo(f"device={device_name} を失効させました")
    else:
        typer.echo(f"device={device_name} は見つかりませんでした")
        raise typer.Exit(code=1)


@token_app.command("list")
def token_list(
    config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス"),
) -> None:
    """発行済みの端末名と発行日時を一覧する (トークン本体は表示しない)。"""
    cfg = ServerConfig.load(config)
    records = TokenStore(cfg.tokens.path).list()
    if not records:
        typer.echo("(トークンは発行されていません)")
        return
    for record in records:
        typer.echo(f"{record.device_name}\tissued_at={record.issued_at}")


@cert_app.command("fingerprint")
def cert_fingerprint(
    config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス"),
) -> None:
    """証明書の SHA-256 フィンガープリントを表示する。"""
    cfg = ServerConfig.load(config)
    typer.echo(compute_fingerprint(cfg.server.tls_cert_path))


@cert_app.command("ensure")
def cert_ensure(
    config: Path = typer.Option(_DEFAULT_CONFIG_PATH, "--config", help="server.yaml のパス"),
) -> None:
    """証明書が無ければ自己署名証明書を生成する。"""
    cfg = ServerConfig.load(config)
    created = ensure_certificate(cfg.server.tls_cert_path, cfg.server.tls_key_path)
    if created:
        typer.echo(f"証明書を生成しました: {cfg.server.tls_cert_path}")
    else:
        typer.echo(f"証明書は既に存在します: {cfg.server.tls_cert_path}")
    typer.echo(f"fingerprint={compute_fingerprint(cfg.server.tls_cert_path)}")


if __name__ == "__main__":
    app()
