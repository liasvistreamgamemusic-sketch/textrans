#!/usr/bin/env bash
# sudo が必要な手順のみ。design.md §4.8 手順2。
# サーバー機のパスワードは自動化から呼べないため、ユーザーが手で1回だけ実行する。
#
# 使い方(サーバー機上で、手動で):
#   ~/voice/app/deploy/setup-root.sh
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./deploy.env
source "$SCRIPT_DIR/deploy.env"

echo "[setup-root] このスクリプトは sudo を要求する。パスワード入力が必要。"

echo "[setup-root] loginctl enable-linger ${LINGER_USER}(ログインなしでユーザーサービスを起動可能にする)"
sudo loginctl enable-linger "$LINGER_USER"

echo "[setup-root] ufw: LAN(${LAN_SUBNET})からの SSH(22/tcp)と ${GATEWAY_PORT}/tcp のみ許可"
# SSH を先に許可しないと、enable 直後に既定の deny incoming で締め出される
sudo ufw allow from "$LAN_SUBNET" to any port 22 proto tcp
sudo ufw allow from "$LAN_SUBNET" to any port "$GATEWAY_PORT" proto tcp
sudo ufw --force enable

echo "[setup-root] 自動スリープを無効化(サーバー用途のため)"
sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target

if command -v gsettings >/dev/null 2>&1; then
    echo "[setup-root] GNOME: AC電源接続時にスリープしない設定"
    # sudo bash で起動された場合や SSH 経由でも本人のセッションバスに書けるようにする
    export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u "$LINGER_USER")/bus"
    sudo -u "$LINGER_USER" --preserve-env=DBUS_SESSION_BUS_ADDRESS \
        gsettings set org.gnome.settings-daemon.plugins.power sleep-inactive-ac-type 'nothing' || \
        echo "[setup-root] WARNING: gsettings の設定に失敗(GNOMEセッション外での実行など)。手動確認が必要" >&2
else
    echo "[setup-root] gsettings が無いため GNOME のスリープ設定はスキップ(GNOME以外のデスクトップ環境の可能性)"
fi

echo "[setup-root] 完了。"
