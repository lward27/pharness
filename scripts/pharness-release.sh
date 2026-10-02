#!/usr/bin/env bash
set -euo pipefail

# One-command PHarness release from current origin/main:
#   build every image in parallel on the in-cluster Tekton/BuildKit path,
#   verify the artifacts (lucas-ops, when installed), archive the live database
#   when the release adds migrations, prepare the digest pin, and open the pin PR.
# With --merge, the PR is merged after its checks pass; Argo then rolls out.
#
# Usage: scripts/pharness-release.sh [--merge] [--artifacts-dir DIR]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel)"
CONTEXT="${PHARNESS_CLUSTER_CONTEXT:-lucas_engineering}"
VALUES="deploy/helm/pharness/values.yaml"
MERGE=false
ARTIFACTS_ROOT="${PHARNESS_RELEASE_ARTIFACTS:-${REPOSITORY_ROOT}/dist/releases}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --merge) MERGE=true; shift ;;
    --artifacts-dir) ARTIFACTS_ROOT="${2:?}"; shift 2 ;;
    *) echo "usage: $0 [--merge] [--artifacts-dir DIR]" >&2; exit 2 ;;
  esac
done

cd "$REPOSITORY_ROOT"
[[ -z "$(git status --porcelain)" ]] || { echo "release requires a clean worktree" >&2; exit 1; }
git fetch --quiet origin main
REVISION="$(git rev-parse origin/main)"
git checkout --quiet --detach "$REVISION"
OUT="${ARTIFACTS_ROOT}/${REVISION}"
mkdir -p "$OUT"
echo "release ${REVISION} -> ${OUT}"

pinned_value() {
  sed -n "/^$1:/,/^[a-zA-Z]/p" "$VALUES" | sed -n "s/^    $2: //p" | head -1
}
PREVIOUS_REVISION="$(pinned_value api revision)"
PREVIOUS_RUNTIME="$(pinned_value api repository)@$(pinned_value api digest)"

# 1. Build (parallel PipelineRuns; reconciles existing runs by name on re-run).
if [[ ! -s "${OUT}/BUILD.json" ]] || ! jq -e '.status == "builds_completed"' "${OUT}/BUILD.json" >/dev/null; then
  PHARNESS_BUNDLE_OUTPUT_DIR="$OUT" "${SCRIPT_DIR}/pharness-build.sh" all --revision "$REVISION" >"${OUT}/BUILD.json"
fi
jq -e '.status == "builds_completed"' "${OUT}/BUILD.json" >/dev/null

digest_of() {
  jq -er --arg c "$1" --arg t "${2:-}" \
    '[.components[] | select(.component == $c and .build_target == $t)][0].digest' "${OUT}/BUILD.json"
}
RUNTIME="$(digest_of runtime)"; UI="$(digest_of ui)"
PYTHON="$(digest_of python-runner)"; NODE="$(digest_of node-runner)"
GATEWAY="$(digest_of model-gateway)"; EVAL="$(digest_of eval-runner)"
CODEX="$(digest_of codex-host runtime)"

# 2. Independent artifact verification, when the operator CLI is installed.
if command -v lucas-ops >/dev/null; then
  jq '{schema_version:1,source_revision:.source_revision,
       images:([.components[] | select(.build_target != "bundle")
         | {key:.component,value:{repository:(.image_url|split("/")[1]|split(":")[0]),digest:.digest}}] | from_entries),
       native_bundle:{path:.native_bundle.archive,digest:("sha256:"+.native_bundle.archive_sha256)}}' \
    "${OUT}/BUILD.json" >"${OUT}/RELEASE.json"
  rm -f "${OUT}/ARTIFACTS.json"
  lucas-ops release verify --manifest "${OUT}/RELEASE.json" --output "${OUT}/ARTIFACTS.json" >/dev/null
  jq -e '.status == "verified_artifacts"' "${OUT}/ARTIFACTS.json" >/dev/null
  echo "artifacts verified"
else
  echo "lucas-ops not installed; skipping independent artifact verification" >&2
fi

# 3. Archive the live database only when this release adds schema migrations;
#    the previous runtime cannot open a database migrated past its own schema.
if [[ -n "$(git diff --name-only --diff-filter=A "$PREVIOUS_REVISION" "$REVISION" -- crates/pharness-store/migrations)" ]]; then
  ARCHIVE="pre-release-${REVISION:0:7}-$(date -u +%Y%m%d)"
  echo "new migrations since ${PREVIOUS_REVISION:0:7}; archiving as ${ARCHIVE}"
  PHARNESS_KUBE_CONTEXT="$CONTEXT" "${SCRIPT_DIR}/pharness-data-archive-job.sh" "$PREVIOUS_RUNTIME" \
    "$(sed -n 's/^    claimName: //p' "$VALUES" | head -1)" \
    "$(sed -n '/^archivePersistence:/,/^[a-zA-Z]/p' "$VALUES" | sed -n 's/^  claimName: //p')" \
    "$ARCHIVE" | tail -1 | tee "${OUT}/ARCHIVE.json"
fi

# 4. Pin, commit and open the release PR.
"${SCRIPT_DIR}/pharness-release-pin.sh" "$REVISION" "$RUNTIME" "$UI" "$PYTHON" "$NODE" "$GATEWAY" "$EVAL" "$CODEX" \
  >"${OUT}/PIN.log"
BRANCH="release/pin-${REVISION:0:12}"
git checkout --quiet -B "$BRANCH"
git add "$VALUES" deploy/helm/pharness/files/agent-execution-registry.json
git commit --quiet -m "release: pin ${REVISION:0:7} images

$(jq -r '.components[] | select(.build_target != "bundle") | "\(.component)  \(.digest)"' "${OUT}/BUILD.json")"
git push --quiet -u origin "$BRANCH"
PR_URL="$(gh pr create --base main --head "$BRANCH" --title "release: pin ${REVISION:0:7} images" \
  --body "Digest pin for \`${REVISION}\`, built by \`scripts/pharness-release.sh\`. Previous runtime: \`${PREVIOUS_REVISION}\`.")"
echo "$PR_URL"

if [[ "$MERGE" == true ]]; then
  # GitHub registers the PR's checks a few seconds after creation.
  for _ in $(seq 1 30); do
    gh pr checks "$PR_URL" >/dev/null 2>&1 && break
    [[ "$(gh pr checks "$PR_URL" 2>&1)" == *"no checks reported"* ]] || break
    sleep 5
  done
  gh pr checks "$PR_URL" --watch --interval 10 >/dev/null
  gh pr merge "$PR_URL" --merge
  echo "merged; Argo applies the pin from main"
fi
git checkout --quiet --detach "$REVISION"
