#!/usr/bin/env bash
set -euo pipefail

# Prepare the separate Helm release commit after both images were built from
# the exact merged source revision. This script does not commit, push, merge,
# or sync Argo; those remain reviewed operator actions.
#
# Usage:
#   scripts/pharness-release-pin.sh <40-char-main-sha> <runtime-sha256> <ui-sha256> <python-runner-sha256>

REVISION="${1:-}"
RUNTIME_DIGEST="${2:-}"
UI_DIGEST="${3:-}"
PYTHON_RUNNER_DIGEST="${4:-}"
VALUES_FILE="deploy/helm/pharness/values.yaml"

[[ "$REVISION" =~ ^[0-9a-f]{40}$ ]] || {
  echo "revision must be a full lowercase 40-character Git SHA" >&2
  exit 1
}
for digest in "$RUNTIME_DIGEST" "$UI_DIGEST" "$PYTHON_RUNNER_DIGEST"; do
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "runtime, UI, and Python runner image digests must be lowercase sha256 digests" >&2
    exit 1
  }
done
[[ -f "$VALUES_FILE" ]] || {
  echo "run this script from the Pharness repository root" >&2
  exit 1
}
[[ -z "$(git status --porcelain)" ]] || {
  echo "release pinning requires a clean worktree" >&2
  exit 1
}

git fetch --quiet origin main
MAIN_REVISION="$(git rev-parse origin/main)"
[[ "$MAIN_REVISION" == "$REVISION" ]] || {
  echo "revision must equal the current merged origin/main ($MAIN_REVISION)" >&2
  exit 1
}

export PHARNESS_RELEASE_REVISION="$REVISION"
export PHARNESS_RUNTIME_DIGEST="$RUNTIME_DIGEST"
export PHARNESS_UI_DIGEST="$UI_DIGEST"
export PHARNESS_PYTHON_RUNNER_DIGEST="$PYTHON_RUNNER_DIGEST"
perl -i -pe '
  if (/^api:/) { $section = "api" }
  elsif (/^ui:/) { $section = "ui" }
  elsif (/^environmentProfiles:/) { $section = "environment_profiles"; $profile = "" }
  elsif (/^[a-zA-Z]/) { $section = "" }
  if ($section eq "environment_profiles" && /^  - id: (.+)$/) { $profile = $1 }
  if ($section eq "api" && /^    digest:/) { $_ = "    digest: $ENV{PHARNESS_RUNTIME_DIGEST}\n" }
  if ($section eq "api" && /^    revision:/) { $_ = "    revision: $ENV{PHARNESS_RELEASE_REVISION}\n" }
  if ($section eq "ui" && /^    digest:/) { $_ = "    digest: $ENV{PHARNESS_UI_DIGEST}\n" }
  if ($section eq "ui" && /^    revision:/) { $_ = "    revision: $ENV{PHARNESS_RELEASE_REVISION}\n" }
  if ($section eq "environment_profiles" && $profile eq "python-3.11" && /^    active:/) { $_ = "    active: true\n" }
  if ($section eq "environment_profiles" && $profile eq "python-3.11" && /^    image:/) { $_ = "    image: registry.lucas.engineering/pharness-python-runner\@$ENV{PHARNESS_PYTHON_RUNNER_DIGEST}\n" }
  if ($section eq "environment_profiles" && $profile eq "python-3.11" && /^    revision:/) { $_ = "    revision: $ENV{PHARNESS_RELEASE_REVISION}\n" }
' "$VALUES_FILE"

helm lint deploy/helm/pharness
RENDERED="$(mktemp)"
trap 'rm -f "$RENDERED"' EXIT
helm template pharness deploy/helm/pharness >"$RENDERED"
if grep -Eq 'registry\.lucas\.engineering/pharness-(runtime|ui|python-runner):latest' "$RENDERED"; then
  echo "rendered release contains a mutable Pharness image" >&2
  exit 1
fi
grep -Fq "registry.lucas.engineering/pharness-runtime@${RUNTIME_DIGEST}" "$RENDERED"
grep -Fq "registry.lucas.engineering/pharness-ui@${UI_DIGEST}" "$RENDERED"
grep -Fq "registry.lucas.engineering/pharness-python-runner@${PYTHON_RUNNER_DIGEST}" "$RENDERED"
RUNNER_PROFILE="$(grep -A3 -F -- "- id: python-3.11" "$VALUES_FILE")"
grep -Fq "active: true" <<<"$RUNNER_PROFILE"
grep -Fq "image: registry.lucas.engineering/pharness-python-runner@${PYTHON_RUNNER_DIGEST}" <<<"$RUNNER_PROFILE"
grep -Fq "revision: ${REVISION}" <<<"$RUNNER_PROFILE"

echo "Prepared digest-pinning release values for ${REVISION}."
echo "Review the diff, commit it on a separate release branch, and merge it manually before Argo sync."
git diff -- "$VALUES_FILE"
