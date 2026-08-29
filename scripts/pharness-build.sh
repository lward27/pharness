#!/usr/bin/env bash
set -euo pipefail

# Compatibility entry point. PHarness release builds are intentionally routed
# only to the dedicated lucas-desktop BuildKit builder. If that builder is
# unavailable this command fails; it never falls back to Kubernetes nodes.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${SCRIPT_DIR}/pharness-build-local.sh" "$@" --builder lucas-desktop
