#!/bin/sh
set -eu

# Official OpenAI Codex standalone release pinned by version and archive hash.
# Keep these values aligned with the immutable AgentExecutionPolicy registry.
CODEX_VERSION="${CODEX_VERSION:-0.150.1}"
CODEX_ARCHIVE_SHA256="${CODEX_ARCHIVE_SHA256:-ab308870bc7fc048c23dc49d03f6b8af9ce7fc99b9da882d6688be7a90155c7a}"
CODEX_BWRAP_ARCHIVE_SHA256="${CODEX_BWRAP_ARCHIVE_SHA256:-3a24807ebbae57a5c37438f5ef9cee2c3d5e77c59ebc23029990a1703e2b932b}"
CODEX_BWRAP_SHA256="${CODEX_BWRAP_SHA256:-c102c5f893faed17ed053ce6ceb9fe0bb03069b991a6f0390e54d82c85f1bca0}"
CODEX_CODE_MODE_HOST_ARCHIVE_SHA256="${CODEX_CODE_MODE_HOST_ARCHIVE_SHA256:-b47667846125cdf6dbc460c6fdc418afb2ef3926c54f4d999bbfbeb08dee4fc5}"
CODEX_CODE_MODE_HOST_SHA256="${CODEX_CODE_MODE_HOST_SHA256:-b3d633427c8c75057fba11dad6051714d44886440305e86ba9d2c0366f4dd63b}"
TARGETARCH="${TARGETARCH:-amd64}"
DESTINATION="${1:-/out/codex}"
SANDBOX_ALIAS="$(dirname "$DESTINATION")/codex-linux-sandbox"
CODE_MODE_HOST="$(dirname "$DESTINATION")/codex-code-mode-host"
RESOURCE_DIRECTORY="$(dirname "$DESTINATION")/codex-resources"

if [ "$TARGETARCH" != "amd64" ]; then
  echo "Codex host milestone supports only linux/amd64" >&2
  exit 1
fi

archive="$(mktemp)"
bwrap_archive="$(mktemp)"
code_mode_host_archive="$(mktemp)"
directory="$(mktemp -d)"
cleanup() {
  rm -f "$archive"
  rm -f "$bwrap_archive"
  rm -f "$code_mode_host_archive"
  rm -rf "$directory"
}
trap cleanup EXIT

url="https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/codex-x86_64-unknown-linux-musl.tar.gz"
bwrap_url="https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/bwrap-x86_64-unknown-linux-musl.tar.gz"
code_mode_host_url="https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/codex-code-mode-host-x86_64-unknown-linux-musl.tar.gz"
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$archive" "$url"
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$bwrap_archive" "$bwrap_url"
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$code_mode_host_archive" "$code_mode_host_url"
printf '%s  %s\n' "$CODEX_ARCHIVE_SHA256" "$archive" | sha256sum --check --status
printf '%s  %s\n' "$CODEX_BWRAP_ARCHIVE_SHA256" "$bwrap_archive" | sha256sum --check --status
printf '%s  %s\n' "$CODEX_CODE_MODE_HOST_ARCHIVE_SHA256" "$code_mode_host_archive" | sha256sum --check --status
tar -xzf "$archive" -C "$directory"
tar -xzf "$bwrap_archive" -C "$directory"
tar -xzf "$code_mode_host_archive" -C "$directory"
install -D -m 0755 "$directory/codex-x86_64-unknown-linux-musl" "$DESTINATION"
# App Server starts this sibling process for its local filesystem and command
# tools. Codex 0.150.1 distributes it as a separate immutable release asset.
printf '%s  %s\n' \
  "$CODEX_CODE_MODE_HOST_SHA256" \
  "$directory/codex-code-mode-host-x86_64-unknown-linux-musl" \
  | sha256sum --check --status
install -D -m 0755 \
  "$directory/codex-code-mode-host-x86_64-unknown-linux-musl" \
  "$CODE_MODE_HOST"
# Codex dispatches its Linux sandbox by argv0. The CLI normally creates a
# temporary alias at startup, but PHarness intentionally replaces the command
# PATH with a stage-scoped allowlist. Install a stable hard link beside Codex
# so bubblewrap can re-exec the exact pinned binary through that allowlist.
ln "$DESTINATION" "$SANDBOX_ALIAS"
test -x "$SANDBOX_ALIAS"
cmp -s "$DESTINATION" "$SANDBOX_ALIAS"
"$SANDBOX_ALIAS" --help >/dev/null
printf '%s  %s\n' \
  "$CODEX_BWRAP_SHA256" \
  "$directory/bwrap-x86_64-unknown-linux-musl" \
  | sha256sum --check --status
install -D -m 0755 "$directory/bwrap-x86_64-unknown-linux-musl" "$RESOURCE_DIRECTORY/bwrap"
"$DESTINATION" --version | grep -F "${CODEX_VERSION}" >/dev/null
test -x "$CODE_MODE_HOST"
printf '%s  %s\n' "$CODEX_CODE_MODE_HOST_SHA256" "$CODE_MODE_HOST" | sha256sum --check --status
printf '%s  %s\n' \
  "$(sha256sum "$DESTINATION" | awk '{print $1}')" \
  "$SANDBOX_ALIAS" \
  | sha256sum --check --status
"$RESOURCE_DIRECTORY/bwrap" --version | grep -F 'bubblewrap' >/dev/null
printf '%s  %s\n' "$CODEX_BWRAP_SHA256" "$RESOURCE_DIRECTORY/bwrap" | sha256sum --check --status
