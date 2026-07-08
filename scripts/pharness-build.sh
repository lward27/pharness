#!/usr/bin/env bash
set -euo pipefail

# Trigger Tekton clone-build-push runs for pharness images.
#
# Usage: scripts/pharness-build.sh <runtime|ui|all> [--node <hostname>]
#
# Image references are kept literal inside the manifest heredoc on purpose:
# an interpolated `$VAR:latest` once parsed as the zsh `${VAR:l}` lowercase
# modifier and silently pushed images to `pharness-uiatest`. Do not
# reintroduce variable expansion around the colon.

NAMESPACE="tekton-pipelines"
NODE="${PHARNESS_BUILD_NODE:-ubuntu-lucas-engineering-2}"

usage() {
  echo "Usage: $0 <runtime|ui|all> [--node <hostname>]" >&2
  exit 1
}

TARGET="${1:-}"
[[ -n "$TARGET" ]] || usage
shift || true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --node)
      NODE="$2"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

trigger_runtime() {
  kubectl create -n "$NAMESPACE" -f - <<MANIFEST
apiVersion: tekton.dev/v1
kind: PipelineRun
metadata:
  generateName: pharness-runtime-run-
  namespace: $NAMESPACE
  labels:
    app.kubernetes.io/part-of: tekton-ci
    app.kubernetes.io/component: pharness-runtime
spec:
  pipelineRef:
    name: clone-build-push
  taskRunTemplate:
    podTemplate:
      nodeSelector:
        kubernetes.io/hostname: $NODE
  workspaces:
    - name: shared-data
      volumeClaimTemplate:
        spec:
          accessModes:
            - ReadWriteOnce
          resources:
            requests:
              storage: 8Gi
  params:
    - name: repo-url
      value: "https://github.com/lward27/pharness.git"
    - name: image-reference
      value: "registry.lucas.engineering/pharness-runtime:latest"
    - name: dockerfile
      value: "./deploy/docker/Dockerfile.runtime"
    - name: context
      value: "./"
    - name: kaniko-extra-args
      value:
        - --skip-tls-verify
    - name: deployment
      value: pharness-api
    - name: deployment-namespace
      value: pharness
MANIFEST
}

trigger_ui() {
  kubectl create -n "$NAMESPACE" -f - <<MANIFEST
apiVersion: tekton.dev/v1
kind: PipelineRun
metadata:
  generateName: pharness-ui-run-
  namespace: $NAMESPACE
  labels:
    app.kubernetes.io/part-of: tekton-ci
    app.kubernetes.io/component: pharness-ui
spec:
  pipelineRef:
    name: clone-build-push
  taskRunTemplate:
    podTemplate:
      nodeSelector:
        kubernetes.io/hostname: $NODE
  workspaces:
    - name: shared-data
      volumeClaimTemplate:
        spec:
          accessModes:
            - ReadWriteOnce
          resources:
            requests:
              storage: 1Gi
  params:
    - name: repo-url
      value: "https://github.com/lward27/pharness.git"
    - name: image-reference
      value: "registry.lucas.engineering/pharness-ui:latest"
    - name: dockerfile
      value: "./deploy/docker/Dockerfile.ui"
    - name: context
      value: "./"
    - name: kaniko-extra-args
      value:
        - --skip-tls-verify
    - name: deployment
      value: pharness-ui
    - name: deployment-namespace
      value: pharness
MANIFEST
}

case "$TARGET" in
  runtime) trigger_runtime ;;
  ui) trigger_ui ;;
  all)
    trigger_runtime
    trigger_ui
    ;;
  *) usage ;;
esac

kubectl get pipelineruns -n "$NAMESPACE" --sort-by=.metadata.creationTimestamp | tail -3
