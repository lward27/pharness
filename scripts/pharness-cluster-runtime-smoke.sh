#!/usr/bin/env bash
set -euo pipefail

# Cluster runtime smoke for the deployed pharness control plane (V2 Alpha).
#
# Validates, against the live homelab deployment:
#   - deployment and rollout health in the pharness namespace
#   - operator token auth gating (401 without, 200 with, /health open)
#   - kubernetes_job dispatcher configuration
#   - the deterministic control-plane contract via the existing e2e smoke
#     in --external-api mode (SDLC chain, readiness, SSE cursor)
#   - worker Job lifecycle: submit -> Job created -> durable outcome ingested
#   - cancellation deletes the worker Job and lands run.cancelled
#   - operator console reachable and proxying with injected identity
#
# Model-backed checks are an explicit operator choice. This script never reads
# credential values from Kubernetes Secrets.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAMESPACE="${PHARNESS_NAMESPACE:-pharness}"
LOCAL_API_PORT="${PHARNESS_SMOKE_API_PORT:-14777}"
LOCAL_UI_PORT="${PHARNESS_SMOKE_UI_PORT:-18080}"
ARTIFACT_ROOT="${PHARNESS_SMOKE_ARTIFACT_DIR:-target/cluster-smoke}"
RUN_NAME="$(date -u +%Y%m%dT%H%M%SZ)"
ARTIFACT_DIR="$ROOT/$ARTIFACT_ROOT/$RUN_NAME"
API_URL="http://127.0.0.1:$LOCAL_API_PORT"
UI_URL="http://127.0.0.1:$LOCAL_UI_PORT"
PF_PIDS=()
MODEL_STATUS="${PHARNESS_SMOKE_MODEL_CHECKS:-skipped}"

log() { printf '==> %s\n' "$*"; }
fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  for pid in "${PF_PIDS[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

assert_jq() {
  local file="$1" expr="$2" message="$3"
  if ! jq -e "$expr" "$file" >/dev/null; then
    fail "$message (see $file)"
  fi
}

api_curl() {
  curl -fsS -H "authorization: Bearer $PHARNESS_API_TOKEN" "$@"
}

wait_for() {
  local label="$1" attempts="$2" delay="$3"
  shift 3
  local attempt
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$delay"
  done
  fail "timed out waiting for $label"
}

wait_for_run_status() {
  local run_id="$1" attempts="$2" delay="$3"
  shift 3
  local attempt status
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    status="$(api_curl "$API_URL/api/runs/$run_id" | jq -r '.status')"
    for expected in "$@"; do
      if [[ "$status" == "$expected" ]]; then
        echo "$status"
        return 0
      fi
    done
    sleep "$delay"
  done
  fail "run $run_id never reached one of: $* (last status: $status)"
}

mkdir -p "$ARTIFACT_DIR"
log "artifacts in $ARTIFACT_ROOT/$RUN_NAME"

# --- preflight -------------------------------------------------------------
log "preflight: deployments in namespace $NAMESPACE"
kubectl -n "$NAMESPACE" rollout status deployment/pharness-api --timeout=180s
kubectl -n "$NAMESPACE" rollout status deployment/pharness-ui --timeout=180s
kubectl -n "$NAMESPACE" get pods -o wide >"$ARTIFACT_DIR/pods.txt"

[[ -n "${PHARNESS_API_TOKEN:-}" ]] || fail "PHARNESS_API_TOKEN must be supplied by the operator"
export PHARNESS_API_TOKEN

case "$MODEL_STATUS" in
  enabled|skipped) ;;
  *) fail "PHARNESS_SMOKE_MODEL_CHECKS must be enabled or skipped" ;;
esac
log "model checks: $MODEL_STATUS"

# --- port-forwards ---------------------------------------------------------
log "port-forwarding services"
kubectl -n "$NAMESPACE" port-forward svc/pharness-api "$LOCAL_API_PORT:4777" \
  >"$ARTIFACT_DIR/pf-api.log" 2>&1 &
PF_PIDS+=($!)
kubectl -n "$NAMESPACE" port-forward svc/pharness-ui "$LOCAL_UI_PORT:80" \
  >"$ARTIFACT_DIR/pf-ui.log" 2>&1 &
PF_PIDS+=($!)
wait_for "api health" 30 1 curl -fsS "$API_URL/health"
wait_for "ui root" 30 1 curl -fsS "$UI_URL/"

# --- auth gating -----------------------------------------------------------
log "auth: /health open, /api gated"
curl -fsS "$API_URL/health" | jq . >"$ARTIFACT_DIR/health.json"
assert_jq "$ARTIFACT_DIR/health.json" '.ok == true' "health should be ok"

unauth_code="$(curl -s -o /dev/null -w '%{http_code}' "$API_URL/api/runs")"
[[ "$unauth_code" == "401" ]] || fail "unauthenticated /api/runs should be 401, got $unauth_code"

wrong_code="$(curl -s -o /dev/null -w '%{http_code}' \
  -H "authorization: Bearer not-the-token" "$API_URL/api/runs")"
[[ "$wrong_code" == "401" ]] || fail "wrong-token /api/runs should be 401, got $wrong_code"

api_curl "$API_URL/api/config/effective" | jq . >"$ARTIFACT_DIR/config.json"
assert_jq "$ARTIFACT_DIR/config.json" '.worker.mode == "kubernetes_job" and .worker.enabled == true' \
  "deployed api should dispatch runs to kubernetes jobs"
assert_jq "$ARTIFACT_DIR/config.json" '.operator.auth_required == true and .operator.name == "lucas"' \
  "operator identity should resolve from the bearer token"

# --- deterministic control-plane contract ----------------------------------
log "deterministic control-plane contract via e2e smoke (--external-api)"
(
  cd "$ROOT"
  PHARNESS_API_URL="$API_URL" \
  PHARNESS_E2E_ALLOW_EXISTING_RUNS=1 \
  PHARNESS_E2E_ARTIFACT_DIR="$ARTIFACT_ROOT/$RUN_NAME/e2e" \
    scripts/pharness-e2e-smoke.sh --external-api --no-model
)

# --- worker job lifecycle ---------------------------------------------------
log "worker job lifecycle: submit run, expect Job, expect durable outcome"
api_curl -X POST "$API_URL/api/runs" \
  -H "content-type: application/json" \
  -d '{"task":"List the top-level files, then finish with one sentence.","cwd":".","max_turns":6}' \
  | jq . >"$ARTIFACT_DIR/job-run-create.json"
job_run_id="$(jq -r '.id' "$ARTIFACT_DIR/job-run-create.json")"
[[ "$job_run_id" == run_* ]] || fail "run creation should return a run id"
log "submitted $job_run_id"

job_label="pharness.lucas.engineering/run-id=${job_run_id//_/-}"
wait_for "worker job for $job_run_id" 60 2 \
  bash -c "kubectl -n '$NAMESPACE' get jobs -l '$job_label' -o name | grep -q job"
kubectl -n "$NAMESPACE" get jobs -l "$job_label" -o json >"$ARTIFACT_DIR/job-run-job.json"

if [[ "$MODEL_STATUS" == "enabled" ]]; then
  final_status="$(wait_for_run_status "$job_run_id" 120 5 completed failed)"
  [[ "$final_status" == "completed" ]] || fail "model-backed run should complete, got $final_status"
else
  # Placeholder key: the worker starts, the provider call fails, and the
  # worker must still report a durable failed outcome through ingest.
  final_status="$(wait_for_run_status "$job_run_id" 120 5 failed)"
fi
api_curl "$API_URL/api/runs/$job_run_id" | jq . >"$ARTIFACT_DIR/job-run-final.json"
api_curl "$API_URL/api/runs/$job_run_id/events" | jq . >"$ARTIFACT_DIR/job-run-events.json"
assert_jq "$ARTIFACT_DIR/job-run-events.json" \
  '[.events[] | select(.type == "run.queued")] | length == 1' \
  "job run should have a queued event"
assert_jq "$ARTIFACT_DIR/job-run-events.json" \
  '[.events[] | select(.type == "run.started")] | length >= 1' \
  "worker attempt should mark the run started through ingest"
log "job run reached $final_status with durable events"

# --- cancellation -----------------------------------------------------------
log "cancellation: cancel deletes the worker job"
api_curl -X POST "$API_URL/api/runs" \
  -H "content-type: application/json" \
  -d '{"task":"Cancellation smoke: wait quietly.","cwd":".","max_turns":6}' \
  | jq . >"$ARTIFACT_DIR/cancel-run-create.json"
cancel_run_id="$(jq -r '.id' "$ARTIFACT_DIR/cancel-run-create.json")"
cancel_label="pharness.lucas.engineering/run-id=${cancel_run_id//_/-}"
wait_for "worker job for $cancel_run_id" 60 2 \
  bash -c "kubectl -n '$NAMESPACE' get jobs -l '$cancel_label' -o name | grep -q job"
api_curl -X POST "$API_URL/api/runs/$cancel_run_id/cancel" -H "content-type: application/json" -d '{}' \
  | jq . >"$ARTIFACT_DIR/cancel-run-cancelled.json"
assert_jq "$ARTIFACT_DIR/cancel-run-cancelled.json" '.status == "cancelled"' \
  "cancel should mark the run cancelled"
wait_for "worker job deletion for $cancel_run_id" 60 2 \
  bash -c "! kubectl -n '$NAMESPACE' get jobs -l '$cancel_label' -o name | grep -q job"
log "cancellation removed the worker job"

# --- console ----------------------------------------------------------------
log "console: static shell and identity-injecting proxy"
curl -fsS "$UI_URL/" >"$ARTIFACT_DIR/ui-index.html"
grep -q '<div id="root">' "$ARTIFACT_DIR/ui-index.html" || fail "console should serve the app shell"
curl -fsS "$UI_URL/api/config/effective" | jq . >"$ARTIFACT_DIR/ui-config.json"
assert_jq "$ARTIFACT_DIR/ui-config.json" '.operator.name == "lucas"' \
  "console proxy should authenticate as the console operator"

# --- manifest ----------------------------------------------------------------
jq -n \
  --arg namespace "$NAMESPACE" \
  --arg model "$MODEL_STATUS" \
  --arg job_run "$job_run_id" \
  --arg job_run_status "$final_status" \
  --arg cancel_run "$cancel_run_id" \
  '{
    smoke: "cluster-runtime",
    namespace: $namespace,
    model_checks: $model,
    checks: [
      "rollout_health",
      "operator_auth_gating",
      "kubernetes_job_dispatcher_config",
      "deterministic_control_plane_contract",
      "worker_job_lifecycle",
      "worker_outcome_ingest",
      "cancellation_deletes_job",
      "console_shell_and_proxy_identity"
    ],
    job_run: { id: $job_run, final_status: $job_run_status },
    cancel_run: { id: $cancel_run }
  }' >"$ARTIFACT_DIR/manifest.json"
jq . "$ARTIFACT_DIR/manifest.json"
log "cluster runtime smoke passed; artifacts in $ARTIFACT_ROOT/$RUN_NAME"
