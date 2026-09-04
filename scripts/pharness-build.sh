#!/usr/bin/env bash
set -euo pipefail

# Compatibility entry point. The caller explicitly chooses a builder through
# --builder or PHARNESS_BUILDX_BUILDER; unavailable builders never fall back.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${SCRIPT_DIR}/pharness-build-local.sh" "$@"
