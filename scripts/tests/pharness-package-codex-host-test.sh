#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_SCRIPT="${SCRIPT_DIR}/../pharness-package-codex-host.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/pharness-bundle-checksum.XXXXXX")"

cleanup() {
  if [[ -n "${TEST_ROOT:-}" && -d "$TEST_ROOT" && "$TEST_ROOT" == */pharness-bundle-checksum.* ]]; then
    rm -rf -- "$TEST_ROOT"
  fi
}
trap cleanup EXIT

# The packaging client may be macOS, whose sha256sum accepts the portable
# short check flag but rejects GNU-only long options such as --check and
# --status. Keep the release path compatible with both implementations.
if grep -Eq 'sha256sum[[:space:]]+--(check|status)' "$PACKAGE_SCRIPT"; then
  echo "native bundle packaging uses non-portable sha256sum long options" >&2
  exit 1
fi

printf 'portable bundle checksum\n' >"${TEST_ROOT}/payload"
(
  cd "$TEST_ROOT"
  sha256sum payload >CHECKSUMS.sha256
  sha256sum -c CHECKSUMS.sha256 >/dev/null
)

# The BSD sha256sum shipped by macOS requires an explicit `-` operand when
# the checksum list arrives on stdin. GNU sha256sum accepts the same form.
payload_sha256="$(sha256sum "${TEST_ROOT}/payload" | awk '{print $1}')"
printf '%s  %s\n' "$payload_sha256" "${TEST_ROOT}/payload" \
  | sha256sum -c - >/dev/null

echo "pharness native bundle checksum portability tests passed"
