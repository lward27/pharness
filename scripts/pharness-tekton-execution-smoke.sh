#!/usr/bin/env bash
set -euo pipefail

# Executes the narrowest production-shaped delivery path available in V2:
# an audited SDLC chain -> approved PipelineIntent -> executor Job -> inert
# Tekton PipelineRun -> durable terminal evidence. It never accesses or
# changes application resources, including the finance experiments.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAMESPACE="${PHARNESS_NAMESPACE:-pharness}"
TEKTON_NAMESPACE="${PHARNESS_TEKTON_SMOKE_NAMESPACE:-tekton-pipelines}"
PIPELINE_NAME="${PHARNESS_TEKTON_SMOKE_PIPELINE:-pharness-e2e-noop}"
LOCAL_API_PORT="${PHARNESS_TEKTON_SMOKE_API_PORT:-14778}"
ARTIFACT_ROOT="${PHARNESS_TEKTON_SMOKE_ARTIFACT_DIR:-target/tekton-execution-smoke}"
RUN_NAME="$(date -u +%Y%m%dT%H%M%SZ)"
ARTIFACT_DIR="$ROOT/$ARTIFACT_ROOT/$RUN_NAME"
API_URL="${PHARNESS_API_URL:-http://127.0.0.1:$LOCAL_API_PORT}"
PORT_FORWARD_PID=""
APPLY=0
EXTERNAL_API=0

usage() {
  cat <<'EOF'
Usage: scripts/pharness-tekton-execution-smoke.sh [--apply] [--external-api]

Without --apply, the script creates the audited SDLC chain and stops after a
successful execution preflight. With --apply, it dispatches the executor Job
for the inert pharness-e2e-noop Pipeline and waits for its terminal evidence.

Options:
  --apply         Dispatch the bounded no-op PipelineRun after preflight.
  --external-api  Use PHARNESS_API_URL; do not create a local port-forward.
  -h, --help      Show this help.

Environment:
  PHARNESS_API_TOKEN                   Required operator bearer token.
  PHARNESS_API_URL                     Required with --external-api.
  PHARNESS_TEKTON_SMOKE_NAMESPACE      Defaults to tekton-pipelines.
  PHARNESS_TEKTON_SMOKE_PIPELINE       Defaults to pharness-e2e-noop.
  PHARNESS_TEKTON_SMOKE_ARTIFACT_DIR   Defaults to target/tekton-execution-smoke.
EOF
}

log() { printf '==> %s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

cleanup() {
  if [[ -n "$PORT_FORWARD_PID" ]] && kill -0 "$PORT_FORWARD_PID" >/dev/null 2>&1; then
    kill "$PORT_FORWARD_PID" >/dev/null 2>&1 || true
    wait "$PORT_FORWARD_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

wait_for() {
  local label="$1" attempts="$2" delay="$3"
  shift 3
  for _ in $(seq 1 "$attempts"); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$delay"
  done
  fail "timed out waiting for $label"
}

api_curl() {
  curl -fsS -H "authorization: Bearer $PHARNESS_API_TOKEN" "$@"
}

post_json() {
  local path="$1" body="$2" output="$3"
  api_curl -X POST "$API_URL$path" -H 'content-type: application/json' -d "$body" | jq . >"$ARTIFACT_DIR/$output"
}

assert_jq() {
  local file="$1" filter="$2" message="$3"
  jq -e "$filter" "$file" >/dev/null || fail "$message (see $file)"
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --apply) APPLY=1 ;;
      --external-api) EXTERNAL_API=1 ;;
      -h|--help) usage; exit 0 ;;
      *) fail "unknown option: $1" ;;
    esac
    shift
  done
  if [[ "$EXTERNAL_API" == "1" ]]; then
    [[ -n "${PHARNESS_API_URL:-}" ]] || fail "PHARNESS_API_URL is required with --external-api"
  else
    API_URL="http://127.0.0.1:$LOCAL_API_PORT"
  fi
}

ensure_contract() {
  api_curl "$API_URL/api/pipeline-contracts?namespace=$TEKTON_NAMESPACE&pipeline_ref=$PIPELINE_NAME&status=active&limit=10" \
    | jq . >"$ARTIFACT_DIR/pipeline-contracts.json"
  local count
  count="$(jq '.count' "$ARTIFACT_DIR/pipeline-contracts.json")"
  if [[ "$count" == "0" ]]; then
    post_json "/api/pipeline-contracts" \
      "$(jq -cn --arg namespace "$TEKTON_NAMESPACE" --arg pipeline_ref "$PIPELINE_NAME" '{namespace:$namespace,pipeline_ref:$pipeline_ref,version:"e2e-v1",contract_json:{params:[],workspaces:[]},actor:"tekton-e2e-smoke",reason:"bounded inert Tekton execution smoke"}')" \
      "pipeline-contract.json"
    PIPELINE_CONTRACT_ID="$(jq -r '.id' "$ARTIFACT_DIR/pipeline-contract.json")"
    return
  fi
  [[ "$count" == "1" ]] || fail "expected at most one active contract for $TEKTON_NAMESPACE/$PIPELINE_NAME"
  assert_jq "$ARTIFACT_DIR/pipeline-contracts.json" \
    '.pipeline_contracts[0].contract_json == {"params":[],"workspaces":[]}' \
    "existing pipeline contract must match the inert fixture inputs"
  PIPELINE_CONTRACT_ID="$(jq -r '.pipeline_contracts[0].id' "$ARTIFACT_DIR/pipeline-contracts.json")"
  cp "$ARTIFACT_DIR/pipeline-contracts.json" "$ARTIFACT_DIR/pipeline-contract.json"
}

wait_for_terminal_execution() {
  local intent_id="$1"
  for _ in $(seq 1 80); do
    api_curl "$API_URL/api/pipeline-intents/$intent_id" | jq . >"$ARTIFACT_DIR/pipeline-intent-terminal.json"
    local status
    status="$(jq -r '.execution_evidence.status // empty' "$ARTIFACT_DIR/pipeline-intent-terminal.json")"
    case "$status" in
      succeeded) return 0 ;;
      failed) fail "Tekton executor reported failure: $(jq -r '.execution_evidence.error // "unknown failure"' "$ARTIFACT_DIR/pipeline-intent-terminal.json")" ;;
    esac
    sleep 3
  done
  fail "PipelineIntent $intent_id did not report terminal execution evidence"
}

main() {
  parse_args "$@"
  command -v kubectl >/dev/null || fail "kubectl is required"
  command -v curl >/dev/null || fail "curl is required"
  command -v jq >/dev/null || fail "jq is required"
  [[ -n "${PHARNESS_API_TOKEN:-}" ]] || fail "PHARNESS_API_TOKEN is required"

  mkdir -p "$ARTIFACT_DIR"
  log "artifacts in $ARTIFACT_ROOT/$RUN_NAME"
  if [[ "$EXTERNAL_API" == "0" ]]; then
    kubectl -n "$NAMESPACE" port-forward "svc/pharness-api" "$LOCAL_API_PORT:4777" >"$ARTIFACT_DIR/port-forward.log" 2>&1 &
    PORT_FORWARD_PID="$!"
  fi
  wait_for "Pharness API health" 30 1 curl -fsS "$API_URL/health"
  kubectl -n "$TEKTON_NAMESPACE" get pipeline "$PIPELINE_NAME" -o json >"$ARTIFACT_DIR/fixture-pipeline.json" \
    || fail "fixture Pipeline $TEKTON_NAMESPACE/$PIPELINE_NAME is missing; wait for GitOps sync before running this smoke"
  assert_jq "$ARTIFACT_DIR/fixture-pipeline.json" \
    '.metadata.labels["pharness.lucas.engineering/fixture"] == "tekton-e2e"' \
    "fixture Pipeline must carry the Pharness e2e label"

  log "creating audited control-plane chain"
  post_json "/api/observations" \
    "$(jq -cn --arg namespace "$TEKTON_NAMESPACE" --arg pipeline "$PIPELINE_NAME" '{source:"tekton_e2e_smoke",kind:"pipeline_execution_request",subject:"finance-experiment-safety-check",summary:"Bounded execution smoke; finance experiment resources are observation-only and unchanged",resource_namespace:$namespace,resource_kind:"Pipeline",resource_name:$pipeline,resource_ref:{apiVersion:"tekton.dev/v1",kind:"Pipeline",namespace:$namespace,name:$pipeline},data_json:{fixture:true,application_resources_changed:false},actor:"tekton-e2e-smoke",reason:"create bounded execution smoke"}')" \
    "observation.json"
  OBSERVATION_ID="$(jq -r '.id' "$ARTIFACT_DIR/observation.json")"
  post_json "/api/incidents" \
    "$(jq -cn --arg observation_id "$OBSERVATION_ID" '{observation_id:$observation_id,severity:"low",title:"Validate bounded Tekton execution",summary:"Exercise the inert Pharness delivery path without changing an application",data_json:{fixture:true},actor:"tekton-e2e-smoke",reason:"create bounded execution smoke"}')" \
    "incident.json"
  INCIDENT_ID="$(jq -r '.id' "$ARTIFACT_DIR/incident.json")"
  post_json "/api/remediation-plans" \
    "$(jq -cn --arg incident_id "$INCIDENT_ID" '{incident_id:$incident_id,title:"Execute inert Tekton fixture",summary:"Preflight and execute a no-op PipelineRun; retain durable evidence",risk_level:"medium",requires_approval:true,plan_json:{steps:["verify contract","dispatch inert PipelineRun","record terminal evidence"],approval_gates:[{kind:"pipeline_mutation",required_before:"starting the inert PipelineRun"},{kind:"cluster_mutation",required_before:"creating the inert PipelineRun"}]},actor:"tekton-e2e-smoke",reason:"create bounded execution smoke"}')" \
    "remediation-plan.json"
  REMEDIATION_PLAN_ID="$(jq -r '.id' "$ARTIFACT_DIR/remediation-plan.json")"

  api_curl "$API_URL/api/approval-gates?remediation_plan_id=$REMEDIATION_PLAN_ID&limit=10" | jq . >"$ARTIFACT_DIR/gates.json"
  for gate_id in $(jq -r '.approval_gates[] | select(.gate_kind == "pipeline_mutation" or .gate_kind == "cluster_mutation") | .id' "$ARTIFACT_DIR/gates.json"); do
    post_json "/api/approval-gates/$gate_id/satisfy" \
      '{"decided_by":"tekton-e2e-smoke","reason":"explicit bounded e2e approval"}' \
      "gate-$gate_id.json"
  done

  post_json "/api/work-plans/from-remediation-plan" "$(jq -cn --arg remediation_plan_id "$REMEDIATION_PLAN_ID" '{remediation_plan_id:$remediation_plan_id}')" "work-plan.json"
  WORK_PLAN_ID="$(jq -r '.work_plan.id' "$ARTIFACT_DIR/work-plan.json")"
  for status in proposed approved; do
    post_json "/api/work-plans/$WORK_PLAN_ID/transition" "$(jq -cn --arg target_status "$status" '{target_status:$target_status,actor:"tekton-e2e-smoke",reason:"bounded e2e workflow"}')" "work-plan-$status.json"
  done

  post_json "/api/change-sets" \
    "$(jq -cn --arg work_plan_id "$WORK_PLAN_ID" '{work_plan_id:$work_plan_id,title:"Bounded Tekton e2e change",summary:"No application code or configuration changes",risk_level:"medium",change_set_json:{changes:[],fixture:"pharness-e2e-noop",application_resources_changed:false},actor:"tekton-e2e-smoke",reason:"create bounded execution smoke"}')" \
    "change-set.json"
  CHANGE_SET_ID="$(jq -r '.change_set.id' "$ARTIFACT_DIR/change-set.json")"
  for status in proposed approved; do
    post_json "/api/change-sets/$CHANGE_SET_ID/transition" "$(jq -cn --arg target_status "$status" '{target_status:$target_status,actor:"tekton-e2e-smoke",reason:"bounded e2e workflow"}')" "change-set-$status.json"
  done
  post_json "/api/change-sets/$CHANGE_SET_ID/trusted-envelope" \
    "$(jq -cn --arg namespace "$TEKTON_NAMESPACE" '{created_by:"tekton-e2e-smoke",reason:"bounded inert execution only",environment:"homelab",namespace:$namespace,production_impacting:false}')" \
    "change-set-envelope.json"

  post_json "/api/pipeline-intents/from-change-set" \
    "$(jq -cn --arg change_set_id "$CHANGE_SET_ID" --arg namespace "$TEKTON_NAMESPACE" --arg pipeline_ref "$PIPELINE_NAME" '{change_set_id:$change_set_id,title:"Execute inert Tekton fixture",summary:"No-op pipeline that only emits a marker",risk_level:"medium",intent_kind:"build_test_package",intent_json:{execution:{enabled:true,namespace:$namespace,pipeline_ref:$pipeline_ref,production_impacting:false,params:{},workspaces:[]}},actor:"tekton-e2e-smoke",reason:"create bounded execution smoke"}')" \
    "pipeline-intent.json"
  PIPELINE_INTENT_ID="$(jq -r '.pipeline_intent.id' "$ARTIFACT_DIR/pipeline-intent.json")"
  post_json "/api/pipeline-intents/$PIPELINE_INTENT_ID/transition" \
    '{"target_status":"approved","actor":"tekton-e2e-smoke","reason":"bounded e2e workflow"}' \
    "pipeline-intent-approved.json"
  ensure_contract
  post_json "/api/pipeline-intents/$PIPELINE_INTENT_ID/trusted-envelope" \
    '{"created_by":"tekton-e2e-smoke","reason":"allow only this inert PipelineIntent"}' \
    "pipeline-intent-envelope.json"
  post_json "/api/pipeline-intents/$PIPELINE_INTENT_ID/execute" \
    '{"dry_run":true,"actor":"tekton-e2e-smoke","reason":"verify bounded execution preflight"}' \
    "execution-preview.json"
  assert_jq "$ARTIFACT_DIR/execution-preview.json" \
    '.ready == true and .status == "ready" and .dry_run == true and .manifest.metadata.namespace == "tekton-pipelines"' \
    "execution preflight must be ready and target the bounded namespace"

  if [[ "$APPLY" != "1" ]]; then
    log "preflight passed; rerun with --apply to dispatch the inert PipelineRun"
    exit 0
  fi
  log "dispatching inert PipelineRun"
  post_json "/api/pipeline-intents/$PIPELINE_INTENT_ID/execute" \
    '{"dry_run":false,"actor":"tekton-e2e-smoke","reason":"explicit bounded e2e execution"}' \
    "execution-dispatch.json"
  assert_jq "$ARTIFACT_DIR/execution-dispatch.json" \
    '.status == "dispatched" and .dry_run == false and .executor_job_name != null' \
    "execution should dispatch a dedicated executor Job"
  wait_for_terminal_execution "$PIPELINE_INTENT_ID"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-terminal.json" \
    '.execution_evidence.status == "succeeded" and .execution_evidence.pipeline_run.name != null' \
    "executor must report durable successful evidence"
  PIPELINE_RUN_NAME="$(jq -r '.execution_evidence.pipeline_run.name' "$ARTIFACT_DIR/pipeline-intent-terminal.json")"
  kubectl -n "$TEKTON_NAMESPACE" get pipelinerun "$PIPELINE_RUN_NAME" -o json >"$ARTIFACT_DIR/pipeline-run.json"
  assert_jq "$ARTIFACT_DIR/pipeline-run.json" \
    '.status.conditions[] | select(.type == "Succeeded" and .status == "True")' \
    "Tekton PipelineRun must finish successfully"

  jq -n \
    --arg pipeline_intent_id "$PIPELINE_INTENT_ID" \
    --arg pipeline_contract_id "$PIPELINE_CONTRACT_ID" \
    --arg pipeline_run_name "$PIPELINE_RUN_NAME" \
    --arg namespace "$TEKTON_NAMESPACE" \
    '{smoke:"tekton-execution",pipeline_intent_id:$pipeline_intent_id,pipeline_contract_id:$pipeline_contract_id,pipeline_run:{namespace:$namespace,name:$pipeline_run_name},application_resources_changed:false}' \
    >"$ARTIFACT_DIR/manifest.json"
  jq . "$ARTIFACT_DIR/manifest.json"
  log "Tekton execution smoke passed; artifacts in $ARTIFACT_ROOT/$RUN_NAME"
}

main "$@"
