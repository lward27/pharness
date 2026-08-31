#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel)"
REVISION=""
BUILDER="${PHARNESS_BUILDX_BUILDER:-lucas-desktop}"
OUTPUT_DIR="${PHARNESS_BUNDLE_OUTPUT_DIR:-${REPOSITORY_ROOT}/dist}"

usage() {
  echo "Usage: $0 --revision <40-char-sha> [--builder lucas-desktop] [--output-dir <path>]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --revision) REVISION="${2:-}"; shift 2 ;;
    --builder) BUILDER="${2:-}"; shift 2 ;;
    --output-dir) OUTPUT_DIR="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ "$REVISION" =~ ^[0-9a-f]{40}$ ]] || usage
[[ "$BUILDER" == "lucas-desktop" ]] || {
  echo "native host bundles are built only on the dedicated lucas-desktop builder" >&2
  exit 1
}

verification="$("${SCRIPT_DIR}/pharness-verify-build-revision.sh" \
  --repo "$REPOSITORY_ROOT" \
  --remote "${PHARNESS_BUILD_REMOTE:-origin}" \
  --branch "${PHARNESS_BUILD_BRANCH:-main}" \
  --revision "$REVISION")"
printf '%s\n' "$verification"
grep -Fq "verified_revision=${REVISION}" <<<"$verification"

temporary="$(mktemp -d "${TMPDIR:-/tmp}/pharness-codex-bundle.XXXXXX")"
cleanup() {
  if [[ -n "${temporary:-}" && -d "$temporary" && "$temporary" == */pharness-codex-bundle.* ]]; then
    rm -rf -- "$temporary"
  fi
}
trap cleanup EXIT

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  --pull \
  --target bundle \
  --file "${REPOSITORY_ROOT}/deploy/docker/Dockerfile.codex-host" \
  --build-arg "PHARNESS_BUILD_REVISION=${REVISION}" \
  --build-arg TARGETARCH=amd64 \
  --output "type=local,dest=${temporary}/root" \
  "$REPOSITORY_ROOT"

bundle_root="${temporary}/root/pharness-codex-host"
test "$(cat "${bundle_root}/REVISION")" = "$REVISION"
test "$("${bundle_root}/bin/pharness-codex-host" --version | awk '{print $2}')" = "0.1.0"
"${bundle_root}/bin/codex" --version | grep -F '0.150.1' >/dev/null
find "$bundle_root" -type f -print0 | sort -z | xargs -0 sha256sum >"${bundle_root}/CHECKSUMS.sha256"

mkdir -p "$OUTPUT_DIR"
archive="${OUTPUT_DIR}/pharness-codex-host-${REVISION}-linux-amd64.tar.gz"
tar -C "${temporary}/root" -czf "$archive" pharness-codex-host
sha256sum "$archive" >"${archive}.sha256"
printf '{"revision":"%s","platform":"linux/amd64","codex_version":"0.150.1","archive":"%s","sha256":"%s"}\n' \
  "$REVISION" \
  "$(basename "$archive")" \
  "$(sha256sum "$archive" | awk '{print $1}')"
