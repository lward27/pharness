#!/usr/bin/env bash
set -euo pipefail

# Verify the Campus image evidence contract.
#
# Offline (CI): approved-images.json, vulnerability-exceptions.json,
# cosign.pub, the key qualification records and the generated admission
# allowlist must be mutually consistent, and no approved digest may rely on a
# missing or expired exception.
#
# Online (release check, default): for every recorded digest the SBOM and
# vulnerability attestations in registry.lucas.engineering must verify against
# deploy/campus/cosign.pub and match the recorded digests; every approved
# digest must carry a verifying approval signature for its repository and
# source revision; no blocked digest may carry one; and each key qualification
# signature must verify.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
COSIGN="${PHARNESS_COSIGN:-cosign}"
OFFLINE=false
TODAY=()

usage() {
  echo "Usage: $0 [--offline] [--root <checkout>] [--today YYYY-MM-DD]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --offline) OFFLINE=true; shift ;;
    --root) ROOT="${2:-}"; shift 2 ;;
    --today) TODAY=(--today "${2:-}"); shift 2 ;;
    *) usage ;;
  esac
done

python3 "${SCRIPT_DIR}/campus_image_gate.py" check --root "$ROOT" "${TODAY[@]}"
if [[ "$OFFLINE" == true ]]; then
  exit 0
fi

command -v "$COSIGN" >/dev/null || {
  echo "cosign was not found; run scripts/campus-install-supply-chain-tools.sh or pass --offline" >&2
  exit 1
}
expected="$("${SCRIPT_DIR}/campus-install-supply-chain-tools.sh" --print-versions | awk -F= '$1 == "cosign" { print $2 }')"
"$COSIGN" version --json 2>/dev/null | grep -Eq "\"gitVersion\": *\"v${expected}\"" || {
  echo "cosign must be the pinned v${expected}" >&2
  exit 1
}
python3 "${SCRIPT_DIR}/campus_image_gate.py" verify-online --root "$ROOT" --cosign "$COSIGN" "${TODAY[@]}"
echo "campus image signatures, attestations and evidence verified"
