#!/usr/bin/env bash
set -euo pipefail

# Pin the Campus egress proxy to the release runtime image, after
# scripts/pharness-release-pin.sh. The proxy is the campus_egress_proxy
# execution kind of pharness-worker in the runtime image, and it is deployed
# by charts/campus-lab in the GitOps repository, so this edits a clean GitOps
# checkout. It does not commit, push, merge, or sync Argo.
#
# Usage (from the Pharness repository root, on the release commit):
#   scripts/campus-egress-release-pin.sh <gitops-checkout> <runtime-sha256>

GITOPS="${1:-}"
RUNTIME_DIGEST="${2:-}"
PHARNESS_VALUES="deploy/helm/pharness/values.yaml"
CAMPUS_LAB_VALUES="${GITOPS}/charts/campus-lab/values.yaml"

[[ "$RUNTIME_DIGEST" =~ ^sha256:[0-9a-f]{64}$ ]] || {
  echo "the runtime image digest must be a lowercase sha256 digest" >&2
  exit 1
}
[[ -f "$PHARNESS_VALUES" ]] || {
  echo "run this script from the Pharness repository root" >&2
  exit 1
}
[[ -n "$GITOPS" && -f "$CAMPUS_LAB_VALUES" ]] || {
  echo "the first argument must be a GitOps checkout containing charts/campus-lab" >&2
  exit 1
}
[[ -z "$(git -C "$GITOPS" status --porcelain)" ]] || {
  echo "egress release pinning requires a clean GitOps worktree" >&2
  exit 1
}
API_VALUES="$(sed -n '/^api:/,/^[[:alnum:]][[:alnum:]_-]*:/p' "$PHARNESS_VALUES")"
grep -Fq "digest: ${RUNTIME_DIGEST}" <<<"$API_VALUES" || {
  echo "the digest must be the runtime digest pinned in ${PHARNESS_VALUES}" >&2
  exit 1
}

export PHARNESS_RUNTIME_DIGEST="$RUNTIME_DIGEST"
perl -i -pe '
  if (/^egress:/) { $section = "egress" }
  elsif (/^[a-zA-Z]/) { $section = "" }
  if ($section eq "egress" && /^  image: registry\.lucas\.engineering\/pharness-runtime@/) {
    $_ = "  image: registry.lucas.engineering/pharness-runtime\@$ENV{PHARNESS_RUNTIME_DIGEST}\n"
  }
' "$CAMPUS_LAB_VALUES"

helm lint "${GITOPS}/charts/campus-lab"
RENDERED="$(mktemp)"
trap 'rm -f "$RENDERED"' EXIT
helm template campus-lab "${GITOPS}/charts/campus-lab" >"$RENDERED"
grep -Fq "image: \"registry.lucas.engineering/pharness-runtime@${RUNTIME_DIGEST}\"" "$RENDERED" || {
  echo "the rendered egress proxy does not use the release runtime image" >&2
  exit 1
}

echo "Pinned the Campus egress proxy to ${RUNTIME_DIGEST}."
echo "Review the diff, commit it on a GitOps branch, and merge it after the Pharness release commit."
git -C "$GITOPS" diff -- charts/campus-lab/values.yaml
