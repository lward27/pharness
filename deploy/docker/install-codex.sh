#!/bin/sh
set -eu

# Official OpenAI Codex standalone release pinned by version and archive hash.
# Keep these values aligned with the immutable AgentExecutionPolicy registry.
CODEX_VERSION="${CODEX_VERSION:-0.150.1}"
CODEX_ARCHIVE_SHA256="${CODEX_ARCHIVE_SHA256:-ab308870bc7fc048c23dc49d03f6b8af9ce7fc99b9da882d6688be7a90155c7a}"
TARGETARCH="${TARGETARCH:-amd64}"
DESTINATION="${1:-/out/codex}"

if [ "$TARGETARCH" != "amd64" ]; then
  echo "Codex host milestone supports only linux/amd64" >&2
  exit 1
fi

archive="$(mktemp)"
directory="$(mktemp -d)"
cleanup() {
  rm -f "$archive"
  rm -rf "$directory"
}
trap cleanup EXIT

url="https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/codex-x86_64-unknown-linux-musl.tar.gz"
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$archive" "$url"
printf '%s  %s\n' "$CODEX_ARCHIVE_SHA256" "$archive" | sha256sum --check --status
tar -xzf "$archive" -C "$directory"
install -D -m 0755 "$directory/codex-x86_64-unknown-linux-musl" "$DESTINATION"
"$DESTINATION" --version | grep -F "${CODEX_VERSION}" >/dev/null
