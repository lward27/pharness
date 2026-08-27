#!/usr/bin/env bash
set -euo pipefail

# Run the immutable runtime image's offline SQLite archive command against one
# exact retained database claim. The Job never mounts PHarness credentials and
# never deletes either claim.
#
# Usage:
#   scripts/pharness-data-archive-job.sh \
#     <runtime-repository@sha256:digest> \
#     <database-claim> <archive-claim> <archive-name> \
#     [accepted-work-item-id ...]

KUBE_CONTEXT="${PHARNESS_KUBE_CONTEXT:-lucas_engineering}"
NAMESPACE="${PHARNESS_NAMESPACE:-pharness}"
IMAGE="${1:-}"
DATABASE_CLAIM="${2:-}"
ARCHIVE_CLAIM="${3:-}"
ARCHIVE_NAME="${4:-}"
shift $(( $# >= 4 ? 4 : $# ))
DRY_RUN="${PHARNESS_ARCHIVE_JOB_DRY_RUN:-}"

[[ "$IMAGE" =~ ^[a-zA-Z0-9._/:=-]+@sha256:[0-9a-f]{64}$ ]] || {
  echo "runtime image must be an immutable repository@sha256:digest reference" >&2
  exit 1
}
for resource in "$DATABASE_CLAIM" "$ARCHIVE_CLAIM" "$ARCHIVE_NAME"; do
  [[ "$resource" =~ ^[a-z0-9]([-a-z0-9]*[a-z0-9])?$ ]] || {
    echo "claim and archive names must be normalized Kubernetes resource names" >&2
    exit 1
  }
done

WORK_ITEM_ARGS=()
for work_item_id in "$@"; do
  [[ "$work_item_id" =~ ^witem_[A-Za-z0-9._-]+$ ]] || {
    echo "invalid WorkItem ID: $work_item_id" >&2
    exit 1
  }
  WORK_ITEM_ARGS+=("--work-item-id" "$work_item_id")
done

JOB_NAME="pharness-data-archive-${ARCHIVE_NAME}"
case "$DRY_RUN" in
  "") KUBECTL_CREATE=(kubectl --context "$KUBE_CONTEXT" --namespace "$NAMESPACE" create -f -) ;;
  client|server)
    KUBECTL_CREATE=(
      kubectl --context "$KUBE_CONTEXT" --namespace "$NAMESPACE"
      create "--dry-run=${DRY_RUN}" -o yaml -f -
    )
    ;;
  *) echo "PHARNESS_ARCHIVE_JOB_DRY_RUN must be client, server, or unset" >&2; exit 1 ;;
esac
"${KUBECTL_CREATE[@]}" <<MANIFEST
apiVersion: batch/v1
kind: Job
metadata:
  name: ${JOB_NAME}
  labels:
    app.kubernetes.io/part-of: pharness
    app.kubernetes.io/component: data-archive
    pharness.dev/archive-name: ${ARCHIVE_NAME}
spec:
  backoffLimit: 0
  activeDeadlineSeconds: 1800
  template:
    metadata:
      labels:
        app.kubernetes.io/part-of: pharness
        app.kubernetes.io/component: data-archive
        pharness.dev/archive-name: ${ARCHIVE_NAME}
    spec:
      automountServiceAccountToken: false
      restartPolicy: Never
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: archive
          image: ${IMAGE}
          imagePullPolicy: IfNotPresent
          command: ["/usr/local/bin/pharness-admin"]
          args:
            - data
            - archive
            - --database
            - /source/pharness.db
            - --output-dir
            - /archive/${ARCHIVE_NAME}
$(for value in "${WORK_ITEM_ARGS[@]}"; do printf '            - %s\n' "$value"; done)
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: ["ALL"]
          volumeMounts:
            - name: source
              mountPath: /source
              readOnly: true
            - name: archive
              mountPath: /archive
            - name: tmp
              mountPath: /tmp
      volumes:
        - name: source
          persistentVolumeClaim:
            claimName: ${DATABASE_CLAIM}
            readOnly: true
        - name: archive
          persistentVolumeClaim:
            claimName: ${ARCHIVE_CLAIM}
        - name: tmp
          emptyDir: {}
MANIFEST

if [[ -n "$DRY_RUN" ]]; then
  exit 0
fi

deadline=$((SECONDS + 1800))
while (( SECONDS < deadline )); do
  JOB_SUCCEEDED=""
  JOB_FAILED=""
  read -r JOB_SUCCEEDED JOB_FAILED <<<"$(
    kubectl --context "$KUBE_CONTEXT" --namespace "$NAMESPACE" \
      get "job/${JOB_NAME}" -o jsonpath='{.status.succeeded} {.status.failed}'
  )"
  if [[ "$JOB_SUCCEEDED" == "1" ]]; then
    break
  fi
  if [[ -n "$JOB_FAILED" && "$JOB_FAILED" != "0" ]]; then
    kubectl --context "$KUBE_CONTEXT" --namespace "$NAMESPACE" \
      logs "job/${JOB_NAME}" --container archive || true
    echo "archive Job ${JOB_NAME} failed" >&2
    exit 1
  fi
  sleep 2
done
[[ "$JOB_SUCCEEDED" == "1" ]] || {
  echo "archive Job ${JOB_NAME} did not complete within 1800 seconds" >&2
  exit 1
}
kubectl --context "$KUBE_CONTEXT" --namespace "$NAMESPACE" logs "job/${JOB_NAME}" --container archive
