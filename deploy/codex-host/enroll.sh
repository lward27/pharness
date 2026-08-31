#!/bin/sh
set -eu

: "${PHARNESS_AGENT_HOST_ENROLLMENT_ID:?set the enrollment ID}"
: "${PHARNESS_AGENT_HOST_ENROLLMENT_TOKEN:?set the one-time enrollment token}"

if [ "$(id -u)" -ne 0 ]; then
  echo "enroll.sh must run as root so the identity is owned by the service account" >&2
  exit 1
fi
exec runuser -u pharness-codex -- env \
  PHARNESS_AGENT_HOST_ENROLLMENT_TOKEN="$PHARNESS_AGENT_HOST_ENROLLMENT_TOKEN" \
  /opt/pharness-codex-host/current/bin/pharness-codex-host enroll \
    --config /etc/pharness-codex-host/config.toml \
    --enrollment-id "$PHARNESS_AGENT_HOST_ENROLLMENT_ID"
