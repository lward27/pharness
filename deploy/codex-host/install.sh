#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "install.sh must run as root" >&2
  exit 1
fi
command -v podman >/dev/null 2>&1 || {
  echo "rootless Podman must be installed before the PHarness host bundle" >&2
  exit 1
}

bundle_root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
revision="${PHARNESS_HOST_BUNDLE_REVISION:-$(cat "$bundle_root/REVISION")}" 
install_root="/opt/pharness-codex-host/${revision}"
if ! id pharness-codex >/dev/null 2>&1; then
  useradd --system --create-home --home-dir /var/lib/pharness-codex-host --shell /bin/bash pharness-codex
fi
if ! grep -q '^pharness-codex:' /etc/subuid; then
  usermod --add-subuids "${PHARNESS_CODEX_SUBUID_RANGE:-200000-265535}" pharness-codex
fi
if ! grep -q '^pharness-codex:' /etc/subgid; then
  usermod --add-subgids "${PHARNESS_CODEX_SUBGID_RANGE:-200000-265535}" pharness-codex
fi

install -d -m 0755 "$install_root/bin" "$install_root/libexec" /usr/lib/pharness-codex-host
install -m 0755 "$bundle_root/bin/pharness-codex-host" "$install_root/bin/pharness-codex-host"
install -m 0755 "$bundle_root/bin/codex" "$install_root/bin/codex"
install -m 0755 "$bundle_root/libexec/git-askpass" "$install_root/libexec/git-askpass"
ln -sfn /opt/pharness-codex-host/current/libexec/git-askpass /usr/lib/pharness-codex-host/git-askpass
ln -sfn "$install_root" /opt/pharness-codex-host/current
install -d -o pharness-codex -g pharness-codex -m 0700 /var/lib/pharness-codex-host/session /var/lib/pharness-codex-host/workspaces
install -d -m 0750 /etc/pharness-codex-host
if [ ! -e /etc/pharness-codex-host/config.toml ]; then
  install -m 0640 -o root -g pharness-codex "$bundle_root/etc/config.toml.example" /etc/pharness-codex-host/config.toml
fi
install -m 0644 "$bundle_root/lib/systemd/system/pharness-codex-host.service" /etc/systemd/system/pharness-codex-host.service
systemctl daemon-reload
echo "Installed PHarness Codex host bundle. Configure it, run login.sh and enroll.sh, then enable the service."
