#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "login.sh must run as root so authentication is owned by the service account" >&2
  exit 1
fi
install -d -o pharness-codex -g pharness-codex -m 0700 /var/lib/pharness-codex-host/session
exec runuser -u pharness-codex -- env \
  HOME=/var/lib/pharness-codex-host \
  CODEX_HOME=/var/lib/pharness-codex-host/session \
  /opt/pharness-codex-host/current/bin/codex login --device-auth
