#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERIFY_SCRIPT="${SCRIPT_DIR}/../pharness-verify-build-revision.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/pharness-build-revision.XXXXXX")"

cleanup() {
  if [[ -n "${TEST_ROOT:-}" && -d "$TEST_ROOT" && "$TEST_ROOT" == */pharness-build-revision.* ]]; then
    rm -rf -- "$TEST_ROOT"
  fi
}
trap cleanup EXIT

REMOTE_REPOSITORY="${TEST_ROOT}/remote.git"
SOURCE_REPOSITORY="${TEST_ROOT}/source"
git init --quiet --bare "$REMOTE_REPOSITORY"
git init --quiet --initial-branch=main "$SOURCE_REPOSITORY"
git -C "$SOURCE_REPOSITORY" config user.name "PHarness test"
git -C "$SOURCE_REPOSITORY" config user.email "pharness-test@example.invalid"
printf 'first\n' >"${SOURCE_REPOSITORY}/fixture.txt"
git -C "$SOURCE_REPOSITORY" add fixture.txt
git -C "$SOURCE_REPOSITORY" commit --quiet -m "first"
git -C "$SOURCE_REPOSITORY" remote add origin "$REMOTE_REPOSITORY"
git -C "$SOURCE_REPOSITORY" push --quiet -u origin main
REVISION="$(git -C "$SOURCE_REPOSITORY" rev-parse HEAD)"

OUTPUT="$("$VERIFY_SCRIPT" --repo "$SOURCE_REPOSITORY" --revision "$REVISION")"
grep -q "^verified_revision=${REVISION}$" <<<"$OUTPUT"

if "$VERIFY_SCRIPT" --repo "$SOURCE_REPOSITORY" --revision 0000000000000000000000000000000000000000 >/dev/null 2>&1; then
  echo "fabricated full revision unexpectedly passed" >&2
  exit 1
fi

if "$VERIFY_SCRIPT" --repo "$SOURCE_REPOSITORY" --revision "${REVISION:0:7}" >/dev/null 2>&1; then
  echo "abbreviated revision unexpectedly passed" >&2
  exit 1
fi

printf 'dirty\n' >"${SOURCE_REPOSITORY}/untracked.txt"
if "$VERIFY_SCRIPT" --repo "$SOURCE_REPOSITORY" --revision "$REVISION" >/dev/null 2>&1; then
  echo "dirty worktree unexpectedly passed" >&2
  exit 1
fi
rm "${SOURCE_REPOSITORY}/untracked.txt"

printf 'second\n' >>"${SOURCE_REPOSITORY}/fixture.txt"
git -C "$SOURCE_REPOSITORY" add fixture.txt
git -C "$SOURCE_REPOSITORY" commit --quiet -m "second"
git -C "$SOURCE_REPOSITORY" push --quiet origin main
if "$VERIFY_SCRIPT" --repo "$SOURCE_REPOSITORY" --revision "$REVISION" >/dev/null 2>&1; then
  echo "stale branch-head revision unexpectedly passed" >&2
  exit 1
fi

echo "pharness build revision verification tests passed"
