#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_PATH="."
REMOTE_NAME="origin"
BRANCH_NAME="main"
REQUESTED_REVISION=""

usage() {
  echo "Usage: $0 --revision <40-char-sha> [--repo <path>] [--remote <name>] [--branch <name>]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) REPOSITORY_PATH="${2:-}"; shift 2 ;;
    --remote) REMOTE_NAME="${2:-}"; shift 2 ;;
    --branch) BRANCH_NAME="${2:-}"; shift 2 ;;
    --revision) REQUESTED_REVISION="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$REPOSITORY_PATH" && -n "$REMOTE_NAME" && -n "$BRANCH_NAME" ]] || usage
[[ "$REQUESTED_REVISION" =~ ^[0-9a-f]{40}$ ]] || {
  echo "--revision must be a full lowercase 40-character Git commit SHA" >&2
  exit 1
}

REPOSITORY_ROOT="$(git -C "$REPOSITORY_PATH" rev-parse --show-toplevel)"
git -C "$REPOSITORY_ROOT" remote get-url "$REMOTE_NAME" >/dev/null

if [[ -n "$(git -C "$REPOSITORY_ROOT" status --porcelain=v1 --untracked-files=all)" ]]; then
  echo "build revision verification requires a clean worktree" >&2
  exit 1
fi

CHECKOUT_REVISION="$(git -C "$REPOSITORY_ROOT" rev-parse HEAD)"
[[ "$CHECKOUT_REVISION" == "$REQUESTED_REVISION" ]] || {
  echo "requested revision does not equal the build worktree HEAD" >&2
  exit 1
}

BRANCH_REF="refs/heads/${BRANCH_NAME}"
git -C "$REPOSITORY_ROOT" fetch --quiet --no-tags "$REMOTE_NAME" "$BRANCH_REF"
FETCHED_REVISION="$(git -C "$REPOSITORY_ROOT" rev-parse FETCH_HEAD)"
REMOTE_LINE="$(git -C "$REPOSITORY_ROOT" ls-remote --exit-code "$REMOTE_NAME" "$BRANCH_REF")"
REMOTE_REVISION="${REMOTE_LINE%%[[:space:]]*}"

[[ "$FETCHED_REVISION" =~ ^[0-9a-f]{40}$ && "$REMOTE_REVISION" =~ ^[0-9a-f]{40}$ ]] || {
  echo "remote branch did not resolve to one full commit SHA" >&2
  exit 1
}
[[ "$FETCHED_REVISION" == "$REMOTE_REVISION" ]] || {
  echo "remote branch moved during build revision verification; retry from a fresh preflight" >&2
  exit 1
}
[[ "$REQUESTED_REVISION" == "$REMOTE_REVISION" ]] || {
  echo "requested revision does not equal the current remote branch head" >&2
  exit 1
}

git -C "$REPOSITORY_ROOT" cat-file -e "${REQUESTED_REVISION}^{commit}"
OBJECT_REVISION="$(git -C "$REPOSITORY_ROOT" show -s --format=%H "$REQUESTED_REVISION")"
[[ "$OBJECT_REVISION" == "$REQUESTED_REVISION" ]] || {
  echo "verified object identity does not match the requested revision" >&2
  exit 1
}

printf 'repository_root=%s\n' "$REPOSITORY_ROOT"
printf 'remote=%s\n' "$REMOTE_NAME"
printf 'branch=%s\n' "$BRANCH_NAME"
printf 'verified_revision=%s\n' "$REQUESTED_REVISION"
printf 'worktree_clean=true\n'
