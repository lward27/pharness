#!/usr/bin/env bash
set -euo pipefail

# Build one immutable Pharness revision. This script never restarts a
# Deployment; the resulting digests must be reviewed and committed to Helm.
#
# Usage: scripts/pharness-build.sh <runtime|ui|python-runner|all> --revision <40-char-sha> [--node <hostname>]

NAMESPACE="tekton-pipelines"
KUBE_CONTEXT="${PHARNESS_KUBE_CONTEXT:-lucas_engineering}"
NODE="${PHARNESS_BUILD_NODE:-ubuntu-lucas-engineering-2}"
TARGET="${1:-}"
REVISION=""
RUNS=()

usage() {
  echo "Usage: $0 <runtime|ui|python-runner|all> --revision <40-char-sha> [--node <hostname>]" >&2
  exit 1
}

[[ "$TARGET" =~ ^(runtime|ui|python-runner|all)$ ]] || usage
shift
while [[ $# -gt 0 ]]; do
  case "$1" in
    --revision) REVISION="${2:-}"; shift 2 ;;
    --node) NODE="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ "$REVISION" =~ ^[0-9a-f]{40}$ ]] || {
  echo "--revision must be a full lowercase 40-character Git SHA" >&2
  exit 1
}

trigger() {
  local component="$1" storage="$2" dockerfile="$3"
  local image="registry.lucas.engineering/pharness-${component}:git-${REVISION}"
  local run
  run="$(kubectl --context "$KUBE_CONTEXT" create -n "$NAMESPACE" -o name -f - <<MANIFEST
apiVersion: tekton.dev/v1
kind: PipelineRun
metadata:
  generateName: pharness-${component}-run-
  namespace: ${NAMESPACE}
  labels:
    app.kubernetes.io/part-of: tekton-ci
    app.kubernetes.io/component: pharness-${component}
    pharness.dev/source-revision: ${REVISION}
spec:
  pipelineRef:
    name: clone-build-push
  taskRunTemplate:
    podTemplate:
      nodeSelector:
        kubernetes.io/hostname: ${NODE}
  workspaces:
    - name: shared-data
      volumeClaimTemplate:
        spec:
          accessModes: [ReadWriteOnce]
          resources:
            requests:
              storage: ${storage}
  params:
    - name: repo-url
      value: https://github.com/lward27/pharness.git
    - name: revision
      value: ${REVISION}
    - name: image-reference
      value: ${image}
    - name: dockerfile
      value: ${dockerfile}
    - name: context
      value: ./
    - name: build-args
      value:
        - PHARNESS_BUILD_REVISION=${REVISION}
        - TARGETARCH=amd64
    - name: deployment
      value: ""
MANIFEST
)"
  RUNS+=("${component}:${run#pipelinerun.tekton.dev/}")
}

wait_for_pipeline() {
  local run="$1"
  local status=""
  local reason=""
  local deadline=$((SECONDS + 1800))

  while ((SECONDS < deadline)); do
    status="$(kubectl --context "$KUBE_CONTEXT" get pipelinerun -n "$NAMESPACE" "$run" -o jsonpath='{.status.conditions[?(@.type=="Succeeded")].status}')"
    case "$status" in
      True)
        return 0
        ;;
      False)
        reason="$(kubectl --context "$KUBE_CONTEXT" get pipelinerun -n "$NAMESPACE" "$run" -o jsonpath='{.status.conditions[?(@.type=="Succeeded")].reason}')"
        echo "PipelineRun ${run} failed (${reason:-unknown reason})" >&2
        return 1
        ;;
    esac
    sleep 5
  done

  echo "PipelineRun ${run} did not complete within 30 minutes" >&2
  return 1
}

case "$TARGET" in
  runtime) trigger runtime 8Gi ./deploy/docker/Dockerfile.runtime ;;
  ui) trigger ui 1Gi ./deploy/docker/Dockerfile.ui ;;
  python-runner) trigger python-runner 8Gi ./deploy/docker/Dockerfile.python-runner ;;
  all)
    trigger runtime 8Gi ./deploy/docker/Dockerfile.runtime
    trigger ui 1Gi ./deploy/docker/Dockerfile.ui
    trigger python-runner 8Gi ./deploy/docker/Dockerfile.python-runner
    ;;
esac

for entry in "${RUNS[@]}"; do
  component="${entry%%:*}"
  run="${entry#*:}"
  wait_for_pipeline "$run"
  task_run="$(kubectl --context "$KUBE_CONTEXT" get taskruns -n "$NAMESPACE" -l "tekton.dev/pipelineRun=${run}" -o json | jq -r '.items[] | select(.metadata.labels["tekton.dev/pipelineTask"] == "build-push") | .metadata.name' | head -1)"
  digest="$(kubectl --context "$KUBE_CONTEXT" get taskrun -n "$NAMESPACE" "$task_run" -o json | jq -r '.status.results[] | select(.name == "IMAGE_DIGEST") | .value')"
  image_url="$(kubectl --context "$KUBE_CONTEXT" get taskrun -n "$NAMESPACE" "$task_run" -o json | jq -r '.status.results[] | select(.name == "IMAGE_URL") | .value')"
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "PipelineRun ${run} did not return an immutable image digest" >&2
    exit 1
  }
  jq -n --arg component "$component" --arg revision "$REVISION" --arg pipeline_run "$run" --arg image_url "$image_url" --arg digest "$digest" \
    '{component:$component,revision:$revision,pipeline_run:$pipeline_run,image_url:$image_url,digest:$digest,immutable_ref:($image_url+"@"+$digest)}'
done
