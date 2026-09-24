#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_SCRIPT="${SCRIPT_DIR}/../pharness-build-incluster.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/pharness-incluster-build-test.XXXXXX")"
REVISION=0123456789abcdef0123456789abcdef01234567

cleanup() {
  if [[ -n "${TEST_ROOT:-}" && -d "$TEST_ROOT" && "$TEST_ROOT" == */pharness-incluster-build-test.* ]]; then
    rm -rf -- "$TEST_ROOT"
  fi
}
trap cleanup EXIT

mkdir -p "${TEST_ROOT}/bin"
cat >"${TEST_ROOT}/bin/git" <<'MOCK_GIT'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "-C" ]] || exit 90
shift 2
case "$*" in
  'rev-parse --show-toplevel') echo "$PHARNESS_TEST_REPO" ;;
  'remote get-url origin') echo 'https://github.com/lward27/pharness.git' ;;
  'status --porcelain=v1 --untracked-files=all') ;;
  'rev-parse HEAD') echo "$PHARNESS_TEST_REVISION" ;;
  'fetch --quiet --no-tags origin refs/heads/main') ;;
  'rev-parse FETCH_HEAD') echo "$PHARNESS_TEST_REVISION" ;;
  'ls-remote --exit-code origin refs/heads/main') printf '%s\trefs/heads/main\n' "$PHARNESS_TEST_REVISION" ;;
  "cat-file -e ${PHARNESS_TEST_REVISION}^{commit}") ;;
  "show -s --format=%H ${PHARNESS_TEST_REVISION}") echo "$PHARNESS_TEST_REVISION" ;;
  *) echo "unexpected git invocation: $*" >&2; exit 91 ;;
esac
MOCK_GIT
chmod +x "${TEST_ROOT}/bin/git"

cat >"${TEST_ROOT}/bin/kubectl" <<'MOCK_KUBECTL'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$PHARNESS_TEST_KUBECTL_CALLS"
[[ "${1:-}" == "--context" && "${2:-}" == "lucas_engineering" && "${3:-}" == "-n" && "${4:-}" == "tekton-pipelines" ]] || {
  echo "kubectl did not use the explicit lucas_engineering context" >&2
  exit 92
}
shift 4
case "${1:-}" in
  get)
    case "${2:-}" in
      pipeline/clone-build-push) echo 'pipeline.tekton.dev/clone-build-push' ;;
      serviceaccount/tekton-ci-build) printf '%s\n' '{"automountServiceAccountToken":false}' ;;
      endpointslices)
        printf '%s\n' '{"items":[{"endpoints":[{"conditions":{"ready":true},"targetRef":{"kind":"Pod","name":"k3s-buildkit-test"}}]}]}'
        ;;
      pod/k3s-buildkit-test)
        printf '%s\n' '{"status":{"conditions":[{"type":"Ready","status":"True"}]},"spec":{"nodeName":"builder-test"}}'
        ;;
      node/builder-test)
        printf '%s\n' '{"metadata":{"labels":{"workload":"build","kubernetes.io/arch":"amd64"}}}'
        ;;
      pipelinerun/*)
        if [[ -f "${PHARNESS_TEST_RUN_RECORD:-}" ]]; then
          jq -c '.' "$PHARNESS_TEST_RUN_RECORD"
        else
          printf '%s\n' '{}'
        fi
        ;;
      taskrun/*)
        taskrun_name="${2#taskrun/}"
        run_json="$(cat "$PHARNESS_TEST_RUN_RECORD")"
        if [[ "$taskrun_name" == *-fetch-source ]]; then
          jq -n --arg service_account tekton-ci-build --arg revision "$PHARNESS_TEST_REVISION" \
            '{spec:{serviceAccountName:$service_account},status:{results:[{name:"commit",value:$revision}]}}'
        else
          image_url="$(jq -r '[.spec.params[] | select(.name == "image-reference")][0].value' <<<"$run_json")"
          jq -n --arg service_account tekton-ci-build --arg image "$image_url" \
            --arg digest "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" \
            '{spec:{serviceAccountName:$service_account},status:{results:[{name:"IMAGE_URL",value:$image},{name:"IMAGE_DIGEST",value:$digest}]}}'
        fi
        ;;
      *) echo "unexpected kubectl get: $*" >&2; exit 93 ;;
    esac
    ;;
  create)
    if [[ "${2:-}" == "--dry-run=server" && "${3:-}" == "-f" ]]; then
      cp "${4:?manifest path missing}" "$PHARNESS_TEST_MANIFEST"
      echo 'pipelinerun.tekton.dev/pharness-runtime-0123456789ab'
    elif [[ "${2:-}" == "-f" ]]; then
      run_manifest="${3:?manifest path missing}"
      run_name="$(jq -r '.metadata.name' "$run_manifest")"
      jq --arg run "$run_name" \
        '.status={conditions:[{type:"Succeeded",status:"True"}],childReferences:[
          {kind:"TaskRun",pipelineTaskName:"fetch-source",name:($run+"-fetch-source")},
          {kind:"TaskRun",pipelineTaskName:"build-push",name:($run+"-build-push")}]} ' \
        "$run_manifest" >"$PHARNESS_TEST_RUN_RECORD"
      echo "pipelinerun.tekton.dev/${run_name}"
    else
      echo "unexpected or persisted dry-run kubectl create: $*" >&2
      exit 94
    fi
    ;;
  *) echo "unexpected kubectl invocation: $*" >&2; exit 95 ;;
esac
MOCK_KUBECTL
chmod +x "${TEST_ROOT}/bin/kubectl"

if output="$(PATH="${TEST_ROOT}/bin:${PATH}" \
  PHARNESS_TEST_REPO="$(cd "${SCRIPT_DIR}/../.." && pwd)" \
  PHARNESS_TEST_REVISION="$REVISION" \
  PHARNESS_TEST_KUBECTL_CALLS="${TEST_ROOT}/kubectl-calls.log" \
  PHARNESS_TEST_MANIFEST="${TEST_ROOT}/pipelinerun.json" \
  PHARNESS_KUBECTL="${TEST_ROOT}/bin/kubectl" \
  bash "$BUILD_SCRIPT" runtime --revision "$REVISION" --preflight-only 2>&1)"; then
  :
else
  echo "$output" >&2
  exit 1
fi

jq -e '.status == "preflight_passed" and .cluster_context == "lucas_engineering"
  and .components[0].preflight == "server_dry_run_passed" and .components[0].image_push == false' \
  <<<"$output" >/dev/null
jq -e '.kind == "PipelineRun" and .metadata.namespace == "tekton-pipelines"
  and .metadata.annotations["pharness.lucas.engineering/source-commit"] == "'"$REVISION"'"
  and .spec.pipelineRef.name == "clone-build-push"
  and .spec.taskRunTemplate.serviceAccountName == "tekton-ci-build"
  and ([.spec.params[] | select(.name == "revision" and .value == "'"$REVISION"'")] | length) == 1
  and ([.spec.params[] | select(.name == "image-reference" and .value == "registry.lucas.engineering/pharness-runtime:git-'"$REVISION"'")] | length) == 1
  and ([.spec.params[] | select(.name == "build-target" and .value == "")] | length) == 1
  and ([.spec.params[] | select(.name == "deployment" and .value == "")] | length) == 1' \
  "${TEST_ROOT}/pipelinerun.json" >/dev/null
if rg -q 'create -f ' "${TEST_ROOT}/kubectl-calls.log"; then
  echo "preflight persisted a PipelineRun" >&2
  exit 1
fi

# A named codex-host target must preflight both runtime and bundle builds in
# Tekton; neither may fall back to local Buildx.
if output="$(PATH="${TEST_ROOT}/bin:${PATH}" \
  PHARNESS_TEST_REPO="$(cd "${SCRIPT_DIR}/../.." && pwd)" \
  PHARNESS_TEST_REVISION="$REVISION" \
  PHARNESS_TEST_KUBECTL_CALLS="${TEST_ROOT}/bundle-calls.log" \
  PHARNESS_TEST_MANIFEST="${TEST_ROOT}/bundle-pipelinerun.json" \
  PHARNESS_KUBECTL="${TEST_ROOT}/bin/kubectl" \
  bash "$BUILD_SCRIPT" codex-host --revision "$REVISION" --preflight-only 2>&1)"; then
  :
else
  echo "$output" >&2
  exit 1
fi
[[ "$(rg -c 'create --dry-run=server' "${TEST_ROOT}/bundle-calls.log")" -eq 2 ]]
jq -e '.metadata.name == "pharness-codex-host-bundle-0123456789ab"
  and ([.spec.params[] | select(.name == "build-target" and .value == "bundle")] | length) == 1
  and ([.spec.params[] | select(.name == "image-reference" and .value == "registry.lucas.engineering/pharness-codex-host-bundle:git-'"$REVISION"'")] | length) == 1' \
  "${TEST_ROOT}/bundle-pipelinerun.json" >/dev/null

# Exercise a single completed PipelineRun without contacting a real cluster.
if output="$(PATH="${TEST_ROOT}/bin:${PATH}" \
  PHARNESS_TEST_REPO="$(cd "${SCRIPT_DIR}/../.." && pwd)" \
  PHARNESS_TEST_REVISION="$REVISION" \
  PHARNESS_TEST_KUBECTL_CALLS="${TEST_ROOT}/run-calls.log" \
  PHARNESS_TEST_MANIFEST="${TEST_ROOT}/run-manifest.json" \
  PHARNESS_TEST_RUN_RECORD="${TEST_ROOT}/run-record.json" \
  PHARNESS_KUBECTL="${TEST_ROOT}/bin/kubectl" \
  bash "$BUILD_SCRIPT" runtime --revision "$REVISION" 2>&1)"; then
  :
else
  echo "$output" >&2
  exit 1
fi
jq -e '.status == "builds_completed" and .production_rollout == false
  and .components[0].source_revision == "'"$REVISION"'"
  and .components[0].digest == "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  and .components[0].immutable_ref == "registry.lucas.engineering/pharness-runtime@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"' \
  <<<"$output" >/dev/null

echo "in-cluster build target renders immutable, non-deploying Tekton runs"
