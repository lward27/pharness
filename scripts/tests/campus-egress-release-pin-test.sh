#!/usr/bin/env bash
set -euo pipefail

# Tests scripts/campus-egress-release-pin.sh against a scratch Pharness root
# and a scratch GitOps checkout with a minimal campus-lab chart.

SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/campus-egress-release-pin.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

OLD="sha256:$(printf '1%.0s' {1..64})"
NEW="sha256:$(printf '2%.0s' {1..64})"
OTHER="sha256:$(printf '3%.0s' {1..64})"

mkdir -p "$WORK/pharness/deploy/helm/pharness"
cat >"$WORK/pharness/deploy/helm/pharness/values.yaml" <<VALUES
api:
  image:
    repository: registry.lucas.engineering/pharness-runtime
    digest: ${NEW}
ui:
  image:
    digest: ${OTHER}
VALUES

GITOPS="$WORK/gitops"
mkdir -p "$GITOPS/charts/campus-lab/templates"
cat >"$GITOPS/charts/campus-lab/Chart.yaml" <<'CHART'
apiVersion: v2
name: campus-lab
version: 0.1.0
CHART
cat >"$GITOPS/charts/campus-lab/values.yaml" <<VALUES
runtime:
  image: registry.lucas.engineering/pharness-runtime@${OLD}
egress:
  enabled: true
  image: registry.lucas.engineering/pharness-runtime@${OLD}
  port: 3128
VALUES
cat >"$GITOPS/charts/campus-lab/templates/egress.yaml" <<'TEMPLATE'
apiVersion: v1
kind: ConfigMap
metadata:
  name: egress
data:
  image: {{ .Values.egress.image | quote }}
TEMPLATE
# A checkout whose files are all ignored reads as clean without a commit.
git -C "$GITOPS" init -q
echo '*' >"$GITOPS/.git/info/exclude"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

cd "$WORK/pharness"

if "$SCRIPT" "$GITOPS" "$OTHER" >/dev/null 2>&1; then
  fail "a digest other than the pinned runtime digest was accepted"
fi
if "$SCRIPT" "$GITOPS" "sha256:ABC" >/dev/null 2>&1; then
  fail "a malformed digest was accepted"
fi
if "$SCRIPT" "$WORK/missing" "$NEW" >/dev/null 2>&1; then
  fail "a missing GitOps checkout was accepted"
fi

"$SCRIPT" "$GITOPS" "$NEW" >"$WORK/out" 2>&1 || {
  cat "$WORK/out" >&2
  fail "pinning the release runtime digest failed"
}
grep -Fqx "  image: registry.lucas.engineering/pharness-runtime@${NEW}" "$GITOPS/charts/campus-lab/values.yaml" ||
  fail "egress.image was not pinned"
grep -Fqx "  image: registry.lucas.engineering/pharness-runtime@${OLD}" "$GITOPS/charts/campus-lab/values.yaml" ||
  fail "an image outside the egress section was changed"

: >"$GITOPS/.git/info/exclude"
if "$SCRIPT" "$GITOPS" "$NEW" >/dev/null 2>&1; then
  fail "a dirty GitOps checkout was accepted"
fi

echo "campus-egress-release-pin tests passed"
