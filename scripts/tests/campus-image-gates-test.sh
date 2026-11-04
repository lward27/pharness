#!/usr/bin/env bash
set -euo pipefail

# Offline checks for the Campus image evidence contract: the committed
# evidence, exceptions, public key and generated admission allowlist agree,
# a tampered allowlist is rejected, and the TaskRun tool images use the same
# releases as the pinned workstation installer.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/campus-image-gates.XXXXXX")"
cleanup() {
  if [[ -n "${TEST_ROOT:-}" && -d "$TEST_ROOT" && "$TEST_ROOT" == */campus-image-gates.* ]]; then
    rm -rf -- "$TEST_ROOT"
  fi
}
trap cleanup EXIT
export PYTHONDONTWRITEBYTECODE=1

"${ROOT}/scripts/campus-image-verify.sh" --offline --root "$ROOT"

mkdir -p "${TEST_ROOT}/deploy"
cp -R "${ROOT}/deploy/campus" "${TEST_ROOT}/deploy/campus"
manifest="${TEST_ROOT}/deploy/campus/admission/campus-lab-image-allowlist.yaml"
python3 - "$manifest" <<'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1])
text = path.read_text()
marker = '- name: "allowedImages"\n      expression: "['
assert marker in text, "allowlist variable not found"
tampered = "'registry.lucas.engineering/pharness-runtime@sha256:" + "0" * 64 + "', "
path.write_text(text.replace(marker, marker + tampered, 1))
PY
if "${ROOT}/scripts/campus-image-verify.sh" --offline --root "$TEST_ROOT" >/dev/null 2>&1; then
  echo "a hand-edited admission allowlist unexpectedly passed" >&2
  exit 1
fi

cp "${ROOT}/deploy/campus/admission/campus-lab-image-allowlist.yaml" "$manifest"
jq '.signing.public_key_sha256 = "sha256:'"$(printf '0%.0s' {1..64})"'"' \
  "${ROOT}/deploy/campus/approved-images.json" >"${TEST_ROOT}/deploy/campus/approved-images.json"
if "${ROOT}/scripts/campus-image-verify.sh" --offline --root "$TEST_ROOT" >/dev/null 2>&1; then
  echo "an evidence record for a different signing key unexpectedly passed" >&2
  exit 1
fi

versions="$("${ROOT}/scripts/campus-install-supply-chain-tools.sh" --print-versions)"
for tool in cosign syft grype; do
  version="$(awk -F= -v tool="$tool" '$1 == tool { print $2 }' <<<"$versions")"
  grep -Eq "/${tool}:v${version}@sha256:[0-9a-f]{64}\"" "${ROOT}/scripts/campus_image_gate.py" || {
    echo "the approval TaskRun ${tool} image does not match the pinned CLI v${version}" >&2
    exit 1
  }
done

echo "campus image gate offline tests passed"
