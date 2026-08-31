#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "uninstall.sh must run as root" >&2
  exit 1
fi
systemctl disable --now pharness-codex-host.service 2>/dev/null || true
rm -f /etc/systemd/system/pharness-codex-host.service
systemctl daemon-reload
echo "Service removed. Configuration, identity, subscription session, workspaces, and versioned bundles were retained."
