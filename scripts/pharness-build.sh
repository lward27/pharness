#!/usr/bin/env bash
set -euo pipefail

# Default release entry point. Image compilation runs through the reviewed
# lucas_engineering Tekton/BuildKit route; use pharness-build-local.sh only when
# a local Buildx build is explicitly desired.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${SCRIPT_DIR}/pharness-build-incluster.sh" "$@"
