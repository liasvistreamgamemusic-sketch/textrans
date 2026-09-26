

def test_yaml_paths_with_tilde_are_expanded(tmp_path):
    """YAML に書いた `~/...` は既定値と同様に展開される (実機で `~` という名前のディレクトリが出来た回帰)。"""
    from pathlib import Path

    from voice_server.config import ServerConfig

    cfg_file = tmp_path / "server.yaml"
    cfg_file.write_text(
        "server:\n  tls_cert_path: '~/x/server.crt'\ntokens:\n  path: '~/x/tokens.yaml'\n"
        "recording:\n  dir: '~/x/data'\nasr:\n  model_path: '~/x/model'\n",
        encoding="utf-8",
    )
    cfg = ServerConfig.load(cfg_file)
    for p in (cfg.server.tls_cert_path, cfg.tokens.path, cfg.recording.dir, cfg.asr.model_path):
        assert "~" not in str(p)
        assert p.is_absolute()
        assert str(p).startswith(str(Path.home()))
