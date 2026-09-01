#!/bin/sh
set -eu

# Official OpenAI Codex standalone release pinned by version and archive hash.
# Keep these values aligned with the immutable AgentExecutionPolicy registry.
CODEX_VERSION="${CODEX_VERSION:-0.150.1}"
CODEX_ARCHIVE_SHA256="${CODEX_ARCHIVE_SHA256:-ab308870bc7fc048c23dc49d03f6b8af9ce7fc99b9da882d6688be7a90155c7a}"
CODEX_BWRAP_ARCHIVE_SHA256="${CODEX_BWRAP_ARCHIVE_SHA256:-3a24807ebbae57a5c37438f5ef9cee2c3d5e77c59ebc23029990a1703e2b932b}"
CODEX_BWRAP_SHA256="${CODEX_BWRAP_SHA256:-c102c5f893faed17ed053ce6ceb9fe0bb03069b991a6f0390e54d82c85f1bca0}"
TARGETARCH="${TARGETARCH:-amd64}"
DESTINATION="${1:-/out/codex}"
RESOURCE_DIRECTORY="$(dirname "$DESTINATION")/codex-resources"

if [ "$TARGETARCH" != "amd64" ]; then
  echo "Codex host milestone supports only linux/amd64" >&2
  exit 1
fi

archive="$(mktemp)"
bwrap_archive="$(mktemp)"
directory="$(mktemp -d)"
cleanup() {
  rm -f "$archive"
  rm -f "$bwrap_archive"
  rm -rf "$directory"
}
trap cleanup EXIT

url="https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/codex-x86_64-unknown-linux-musl.tar.gz"
bwrap_url="https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/bwrap-x86_64-unknown-linux-musl.tar.gz"
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$archive" "$url"
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$bwrap_archive" "$bwrap_url"
printf '%s  %s\n' "$CODEX_ARCHIVE_SHA256" "$archive" | sha256sum --check --status
printf '%s  %s\n' "$CODEX_BWRAP_ARCHIVE_SHA256" "$bwrap_archive" | sha256sum --check --status
tar -xzf "$archive" -C "$directory"
tar -xzf "$bwrap_archive" -C "$directory"
install -D -m 0755 "$directory/codex-x86_64-unknown-linux-musl" "$DESTINATION"
printf '%s  %s\n' \
  "$CODEX_BWRAP_SHA256" \
  "$directory/bwrap-x86_64-unknown-linux-musl" \
  | sha256sum --check --status
install -D -m 0755 "$directory/bwrap-x86_64-unknown-linux-musl" "$RESOURCE_DIRECTORY/bwrap"
"$DESTINATION" --version | grep -F "${CODEX_VERSION}" >/dev/null
"$RESOURCE_DIRECTORY/bwrap" --version | grep -F 'bubblewrap' >/dev/null
printf '%s  %s\n' "$CODEX_BWRAP_SHA256" "$RESOURCE_DIRECTORY/bwrap" | sha256sum --check --status
