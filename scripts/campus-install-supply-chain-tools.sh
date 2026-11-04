#!/usr/bin/env bash
set -euo pipefail

# Install the pinned cosign, syft and grype releases used by the Campus image
# approval and verification scripts. Every download is checked against the
# SHA-256 recorded here before it is unpacked or made executable, so a changed
# upstream asset fails closed instead of being trusted on first use.

COSIGN_VERSION="3.1.4"
COSIGN_SHA256="424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa"
SYFT_VERSION="1.56.0"
SYFT_SHA256="1c2493cfcf52d3367d6cb25780d9190c111c2219c34c820574c43e8bae15c528"
GRYPE_VERSION="0.121.1"
GRYPE_SHA256="bc3ea3f9e6fd56ac781c08c6c5e614271b79f438200757f6932085c19b0a4657"

INSTALL_DIR="${PHARNESS_TOOL_DIR:-${HOME}/.local/bin}"

usage() {
  cat >&2 <<EOF
Usage: $0 [--install-dir <dir>] [--print-versions]

Installs cosign ${COSIGN_VERSION}, syft ${SYFT_VERSION} and grype ${GRYPE_VERSION}
for linux/amd64 into ${INSTALL_DIR} (override with --install-dir or
PHARNESS_TOOL_DIR). No root access is used.
EOF
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir) INSTALL_DIR="${2:-}"; shift 2 ;;
    --print-versions)
      printf 'cosign=%s\nsyft=%s\ngrype=%s\n' "$COSIGN_VERSION" "$SYFT_VERSION" "$GRYPE_VERSION"
      exit 0
      ;;
    *) usage ;;
  esac
done

[[ -n "$INSTALL_DIR" ]] || usage
[[ "$(uname -s)/$(uname -m)" == "Linux/x86_64" ]] || {
  echo "only linux/amd64 hosts are supported by the pinned checksums" >&2
  exit 1
}

TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/campus-tools.XXXXXX")"
cleanup() {
  if [[ -n "${TEMP_ROOT:-}" && -d "$TEMP_ROOT" && "$TEMP_ROOT" == */campus-tools.* ]]; then
    rm -rf -- "$TEMP_ROOT"
  fi
}
trap cleanup EXIT

fetch_verified() {
  local url="$1" expected="$2" output="$3"
  curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 \
    --output "$output" "$url"
  printf '%s  %s\n' "$expected" "$output" | sha256sum --check --status || {
    echo "checksum mismatch for ${url}; refusing to install it" >&2
    return 1
  }
}

mkdir -p "$INSTALL_DIR"

fetch_verified \
  "https://github.com/sigstore/cosign/releases/download/v${COSIGN_VERSION}/cosign-linux-amd64" \
  "$COSIGN_SHA256" "${TEMP_ROOT}/cosign"
install -m 0755 "${TEMP_ROOT}/cosign" "${INSTALL_DIR}/cosign"

fetch_verified \
  "https://github.com/anchore/syft/releases/download/v${SYFT_VERSION}/syft_${SYFT_VERSION}_linux_amd64.tar.gz" \
  "$SYFT_SHA256" "${TEMP_ROOT}/syft.tar.gz"
tar --extract --gzip --file "${TEMP_ROOT}/syft.tar.gz" --directory "$TEMP_ROOT" syft
install -m 0755 "${TEMP_ROOT}/syft" "${INSTALL_DIR}/syft"

fetch_verified \
  "https://github.com/anchore/grype/releases/download/v${GRYPE_VERSION}/grype_${GRYPE_VERSION}_linux_amd64.tar.gz" \
  "$GRYPE_SHA256" "${TEMP_ROOT}/grype.tar.gz"
tar --extract --gzip --file "${TEMP_ROOT}/grype.tar.gz" --directory "$TEMP_ROOT" grype
install -m 0755 "${TEMP_ROOT}/grype" "${INSTALL_DIR}/grype"

printf 'installed cosign %s, syft %s and grype %s into %s\n' \
  "$COSIGN_VERSION" "$SYFT_VERSION" "$GRYPE_VERSION" "$INSTALL_DIR"
