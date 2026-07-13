#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_ROOT="${PHARNESS_E2E_ARTIFACT_DIR:-target/e2e-smoke}"
RUN_NAME="$(date -u +%Y%m%dT%H%M%SZ)"
ARTIFACT_DIR="$ROOT/$ARTIFACT_ROOT/$RUN_NAME"
LATEST_DIR="$ROOT/$ARTIFACT_ROOT/latest"
API_LOG="$ARTIFACT_DIR/pharness-api.log"
DB_PATH="$ARTIFACT_DIR/pharness.sqlite"
API_PID=""
API_URL=""
EXTERNAL_API=0
RUN_CLUSTER=0
RUN_MODEL="${PHARNESS_E2E_MODEL:-auto}"
MODEL_STATUS="skipped"
CLUSTER_STATUS="skipped"
PIPELINE_EVIDENCE_STATUS="missing"
DEPLOYMENT_EVIDENCE_STATUS="missing"
RELEASE_OBSERVABILITY_STATUS="missing"
RELEASE_OBSERVABILITY_REMEDIATION_STATUS="missing"
RELEASE_OBSERVABILITY_INCIDENT_ID=""
RELEASE_OBSERVABILITY_REMEDIATION_PLAN_ID=""

usage() {
  cat <<'EOF'
Usage: scripts/pharness-e2e-smoke.sh [options]

Options:
  --cluster   Run live read-only cluster checks after deterministic local checks.
  --model     Require a Fireworks-backed model run. Fails if FIREWORKS_API_KEY is missing.
  --no-model  Skip the Fireworks-backed model run even when FIREWORKS_API_KEY is set.
  --external-api
             Use an already-running API at PHARNESS_API_URL instead of starting one.
  -h, --help  Show this help text.

Environment:
  PHARNESS_API_URL               Required with --external-api.
  PHARNESS_E2E_PORT              Fixed API port. Defaults to an ephemeral local port.
  PHARNESS_E2E_ARTIFACT_DIR      Artifact root. Defaults to target/e2e-smoke.
  PHARNESS_E2E_ARGO_APP          Optional Argo CD Application for --cluster.
  PHARNESS_E2E_LOKI_QUERY        Optional Loki query for --cluster.
  PHARNESS_E2E_MODEL_TIMEOUT_MS  Model run wait timeout. Defaults to 300000.
  PHARNESS_PROMETHEUS_URL        Enables Prometheus inventory for --cluster.
  PHARNESS_LOKI_URL              Enables Loki log summary for --cluster when query is set.
EOF
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --cluster)
        RUN_CLUSTER=1
        ;;
      --model)
        RUN_MODEL="always"
        ;;
      --no-model)
        RUN_MODEL="never"
        ;;
      --external-api)
        EXTERNAL_API=1
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        echo "unknown option: $1" >&2
        usage >&2
        exit 2
        ;;
    esac
    shift
  done
}

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

pick_port() {
  if [[ -n "${PHARNESS_E2E_PORT:-}" ]]; then
    echo "$PHARNESS_E2E_PORT"
    return
  fi

  python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

wait_for_api() {
  local health_url="$1/health"
  for _ in $(seq 1 120); do
    if curl -fsS "$health_url" >/dev/null 2>&1; then
      return 0
    fi
    if [[ -n "$API_PID" ]] && ! kill -0 "$API_PID" >/dev/null 2>&1; then
      echo "pharness-api exited before health check passed" >&2
      tail -80 "$API_LOG" >&2 || true
      return 1
    fi
    sleep 0.5
  done

  echo "timed out waiting for pharness-api at $health_url" >&2
  tail -80 "$API_LOG" >&2 || true
  return 1
}

cleanup() {
  if [[ -n "$API_PID" ]] && kill -0 "$API_PID" >/dev/null 2>&1; then
    kill "$API_PID" >/dev/null 2>&1 || true
    wait "$API_PID" >/dev/null 2>&1 || true
  fi
}

run_json() {
  local name="$1"
  shift
  local out="$ARTIFACT_DIR/$name.json"

  echo "==> $name"
  (
    cd "$ROOT"
    PHARNESS_API_URL="$API_URL" "$@"
  ) >"$out"
  jq type "$out" >/dev/null
  echo "    wrote $ARTIFACT_ROOT/$RUN_NAME/$name.json"
}

assert_jq() {
  local file="$1"
  local filter="$2"
  local message="$3"

  if ! jq -e "$filter" "$file" >/dev/null; then
    echo "assertion failed: $message" >&2
    echo "file: $file" >&2
    jq . "$file" >&2 || true
    exit 1
  fi
}

prepare_artifacts() {
  mkdir -p "$ARTIFACT_DIR"
  rm -f "$LATEST_DIR"
  ln -s "$RUN_NAME" "$LATEST_DIR"
}

start_api() {
  if [[ "$EXTERNAL_API" == "1" ]]; then
    if [[ -z "${PHARNESS_API_URL:-}" ]]; then
      echo "PHARNESS_API_URL is required with --external-api" >&2
      exit 2
    fi
    API_URL="${PHARNESS_API_URL%/}"
    echo "==> using external pharness-api at $API_URL"
    wait_for_api "$API_URL"
    return
  fi

  local port
  port="$(pick_port)"
  API_URL="http://127.0.0.1:$port"

  echo "==> starting pharness-api at $API_URL"
  (
    cd "$ROOT"
    PHARNESS_BIND="127.0.0.1:$port" \
      PHARNESS_DB_PATH="$DB_PATH" \
      RUST_LOG="${RUST_LOG:-pharness_api=info,tower_http=info}" \
      cargo run -q -p pharness-api
  ) >"$API_LOG" 2>&1 &
  API_PID="$!"

  wait_for_api "$API_URL"
}

check_local_contract() {
  curl -fsS "$API_URL/health" | jq . >"$ARTIFACT_DIR/health.json"
  assert_jq "$ARTIFACT_DIR/health.json" '.ok == true' "health endpoint should be ok"

  run_json config cargo run -q -p pharness-cli -- config
  assert_jq "$ARTIFACT_DIR/config.json" '.api.name == "pharness-api" and .policy.deny_secret_access == true' "effective config should expose stable API and policy state"
  if [[ "$RUN_MODEL" == "always" || ( "$RUN_MODEL" == "auto" && -n "${FIREWORKS_API_KEY:-}" ) ]]; then
    assert_jq "$ARTIFACT_DIR/config.json" '.worker.enabled == true' "Fireworks-backed model smoke requires the running API to have an enabled worker"
  fi

  run_json runs-summary cargo run -q -p pharness-cli -- runs summary
  if [[ "${PHARNESS_E2E_ALLOW_EXISTING_RUNS:-0}" == "1" ]]; then
    assert_jq "$ARTIFACT_DIR/runs-summary.json" '.summary.total >= 0' "run summary should be readable on a persistent database"
  else
    assert_jq "$ARTIFACT_DIR/runs-summary.json" '.summary.total == 0' "fresh smoke database should start with zero runs"
  fi

  run_json secret-denial cargo run -q -p pharness-cli -- capabilities kubernetes-get \
    --resource secrets \
    --namespace default \
    --timeout-ms 30000
  assert_jq "$ARTIFACT_DIR/secret-denial.json" '.status == "denied" and .executed == false and .decision.decision == "deny"' "secret reads should be denied before execution"

  run_json registry-inspect cargo run -q -p pharness-cli -- capabilities registry-inspect-image \
    --image-ref team/checkout-api:v0.1.0-smoke \
    --timeout-ms 30000
  assert_jq "$ARTIFACT_DIR/registry-inspect.json" '.status == "ok" and .executed == true and .result.content.image.repository == "team/checkout-api"' "registry inspection should execute and normalize image metadata"

  run_json audit-secret-denial cargo run -q -p pharness-cli -- audit-events \
    --resource-kind capability \
    --resource-id kubernetes_get
  assert_jq "$ARTIFACT_DIR/audit-secret-denial.json" '[.events[] | select(.kind == "direct_capability.denied")] | length >= 1' "secret denial should be audited"

  run_json audit-registry cargo run -q -p pharness-cli -- audit-events \
    --resource-kind capability \
    --resource-id registry_inspect_image
  assert_jq "$ARTIFACT_DIR/audit-registry.json" '[.events[] | select(.kind == "direct_capability.executed")] | length >= 1' "registry inspection should be audited"

  check_event_stream_cursor
}

check_event_stream_cursor() {
  run_json event-stream-cursor-create cargo run -q -p pharness-cli -- run \
    --task "e2e smoke event stream cursor only" \
    --cwd "." \
    --max-turns 1 \
    --no-wait

  local run_id
  run_id="$(jq -r '.run.id' "$ARTIFACT_DIR/event-stream-cursor-create.json")"
  run_json event-stream-cursor-cancel cargo run -q -p pharness-cli -- runs cancel \
    --run-id "$run_id" \
    --with-events

  run_json event-stream-cursor-events cargo run -q -p pharness-cli -- runs events \
    --run-id "$run_id"
  assert_jq "$ARTIFACT_DIR/event-stream-cursor-events.json" '.events | length >= 2' "cursor smoke run should have at least queued and cancelled events"

  local after_seq
  after_seq="$(jq -r '.events[0].seq' "$ARTIFACT_DIR/event-stream-cursor-events.json")"
  echo "==> event-stream-cursor-streamed"
  (
    cd "$ROOT"
    PHARNESS_API_URL="$API_URL" cargo run -q -p pharness-cli -- runs events \
      --run-id "$run_id" \
      --after-seq "$after_seq" \
      --stream \
      --timeout-ms 10000
  ) >"$ARTIFACT_DIR/event-stream-cursor-streamed.jsonl"
  jq -s '.' "$ARTIFACT_DIR/event-stream-cursor-streamed.jsonl" \
    >"$ARTIFACT_DIR/event-stream-cursor-streamed.json"
  echo "    wrote $ARTIFACT_ROOT/$RUN_NAME/event-stream-cursor-streamed.jsonl"
  echo "    wrote $ARTIFACT_ROOT/$RUN_NAME/event-stream-cursor-streamed.json"
  assert_jq "$ARTIFACT_DIR/event-stream-cursor-streamed.json" 'length >= 1' "SSE cursor should replay events after the cursor"
  assert_jq "$ARTIFACT_DIR/event-stream-cursor-streamed.json" "all(.[]; .seq > $after_seq)" "SSE cursor should not replay events at or before after_seq"
  assert_jq "$ARTIFACT_DIR/event-stream-cursor-streamed.json" '[.[] | select(.type == "run.cancelled")] | length == 1' "SSE cursor should include the post-cursor cancellation event"
}

check_sdlc_roots() {
  run_json observation-create cargo run -q -p pharness-cli -- observations create \
    --source smoke \
    --kind pipeline_run_analysis \
    --subject checkout-api \
    --summary "smoke observation for deterministic SDLC chain" \
    --resource-namespace apps-dev \
    --resource-kind PipelineRun \
    --resource-name pr-smoke \
    --resource-ref-json '{"apiVersion":"tekton.dev/v1","kind":"PipelineRun","namespace":"apps-dev","name":"pr-smoke"}' \
    --data-json '{"status":"running","finding":"pipeline still running"}' \
    --actor smoke \
    --reason "e2e smoke root chain"
  assert_jq "$ARTIFACT_DIR/observation-create.json" '.source == "smoke" and .kind == "pipeline_run_analysis" and .resource_namespace == "apps-dev"' "observation create should persist resource metadata"
  OBSERVATION_ID="$(jq -r '.id' "$ARTIFACT_DIR/observation-create.json")"

  run_json incident-create cargo run -q -p pharness-cli -- incidents create \
    --observation-id "$OBSERVATION_ID" \
    --severity medium \
    --title "Pipeline needs review" \
    --summary "PipelineRun pr-smoke is still running" \
    --data-json '{"reason":"pipeline still running"}' \
    --actor smoke \
    --reason "e2e smoke root chain"
  assert_jq "$ARTIFACT_DIR/incident-create.json" '.observation_id == "'"$OBSERVATION_ID"'" and .status == "candidate" and .resource_namespace == "apps-dev"' "incident create should inherit observation resource metadata"
  INCIDENT_ID="$(jq -r '.id' "$ARTIFACT_DIR/incident-create.json")"

  run_json remediation-plan-create cargo run -q -p pharness-cli -- remediation-plans create \
    --incident-id "$INCIDENT_ID" \
    --title "Review pipeline evidence" \
    --summary "Collect read-only pipeline evidence before any mutation" \
    --risk-level medium \
    --plan-json '{"steps":["inspect PipelineRun","inspect TaskRuns","review policy gates"],"approval_gates":[{"kind":"pipeline_mutation","required_before":"starting a Tekton PipelineRun"},{"kind":"cluster_mutation","required_before":"creating a cluster build resource"}]}' \
    --actor smoke \
    --reason "e2e smoke root chain"
  assert_jq "$ARTIFACT_DIR/remediation-plan-create.json" '.incident_id == "'"$INCIDENT_ID"'" and .status == "draft" and .requires_approval == true' "remediation plan create should produce an approval-requiring draft"
  REMEDIATION_PLAN_ID="$(jq -r '.id' "$ARTIFACT_DIR/remediation-plan-create.json")"

  run_json pipeline-execution-gates cargo run -q -p pharness-cli -- approval-gates list \
    --remediation-plan-id "$REMEDIATION_PLAN_ID" \
    --limit 10
  assert_jq "$ARTIFACT_DIR/pipeline-execution-gates.json" '(.approval_gates | length) == 2 and ([.approval_gates[] | select(.gate_kind == "pipeline_mutation" or .gate_kind == "cluster_mutation")] | length) == 2' "remediation plan should create the Tekton execution gates"
  PIPELINE_MUTATION_GATE_ID="$(jq -r '.approval_gates[] | select(.gate_kind == "pipeline_mutation") | .id' "$ARTIFACT_DIR/pipeline-execution-gates.json")"
  CLUSTER_MUTATION_GATE_ID="$(jq -r '.approval_gates[] | select(.gate_kind == "cluster_mutation") | .id' "$ARTIFACT_DIR/pipeline-execution-gates.json")"
  run_json pipeline-mutation-gate-satisfy cargo run -q -p pharness-cli -- approval-gates satisfy \
    --gate-id "$PIPELINE_MUTATION_GATE_ID" \
    --decided-by smoke \
    --reason "e2e smoke pipeline mutation gate"
  run_json cluster-mutation-gate-satisfy cargo run -q -p pharness-cli -- approval-gates satisfy \
    --gate-id "$CLUSTER_MUTATION_GATE_ID" \
    --decided-by smoke \
    --reason "e2e smoke cluster mutation gate"

  run_json remediation-plan-list cargo run -q -p pharness-cli -- remediation-plans list \
    --incident-id "$INCIDENT_ID" \
    --limit 10
  assert_jq "$ARTIFACT_DIR/remediation-plan-list.json" '.count == 1 and .remediation_plans[0].id == "'"$REMEDIATION_PLAN_ID"'"' "remediation plan list should return the created plan"

  run_json audit-observation cargo run -q -p pharness-cli -- audit-events \
    --resource-kind observation \
    --resource-id "$OBSERVATION_ID"
  assert_jq "$ARTIFACT_DIR/audit-observation.json" '[.events[] | select(.kind == "observation.created" and .actor == "smoke")] | length == 1' "observation creation should be audited"

  run_json audit-incident cargo run -q -p pharness-cli -- audit-events \
    --resource-kind incident \
    --resource-id "$INCIDENT_ID"
  assert_jq "$ARTIFACT_DIR/audit-incident.json" '[.events[] | select(.kind == "incident.created" and .actor == "smoke")] | length == 1' "incident creation should be audited"

  run_json audit-remediation-plan cargo run -q -p pharness-cli -- audit-events \
    --resource-kind remediation_plan \
    --resource-id "$REMEDIATION_PLAN_ID"
  assert_jq "$ARTIFACT_DIR/audit-remediation-plan.json" '[.events[] | select(.kind == "remediation_plan.created" and .actor == "smoke")] | length == 1' "remediation plan creation should be audited"
}

check_sdlc_downstream() {
  run_json work-plan-create cargo run -q -p pharness-cli -- work-plans create-from-remediation-plan \
    --remediation-plan-id "$REMEDIATION_PLAN_ID"
  assert_jq "$ARTIFACT_DIR/work-plan-create.json" '.created == true and .work_plan.remediation_plan_id == "'"$REMEDIATION_PLAN_ID"'" and .work_plan.status == "draft"' "work plan create should produce a draft from the remediation plan"
  WORK_PLAN_ID="$(jq -r '.work_plan.id' "$ARTIFACT_DIR/work-plan-create.json")"

  run_json work-plan-propose cargo run -q -p pharness-cli -- work-plans transition \
    --work-plan-id "$WORK_PLAN_ID" \
    --target-status proposed \
    --actor smoke \
    --reason "e2e smoke work plan proposed"
  assert_jq "$ARTIFACT_DIR/work-plan-propose.json" '.work_plan.id == "'"$WORK_PLAN_ID"'" and .work_plan.status == "proposed"' "work plan should transition to proposed"

  run_json work-plan-approve cargo run -q -p pharness-cli -- work-plans transition \
    --work-plan-id "$WORK_PLAN_ID" \
    --target-status approved \
    --actor smoke \
    --reason "e2e smoke work plan approved"
  assert_jq "$ARTIFACT_DIR/work-plan-approve.json" '.work_plan.id == "'"$WORK_PLAN_ID"'" and .work_plan.status == "approved"' "work plan should transition to approved"

  run_json work-plan-flow cargo run -q -p pharness-cli -- work-plans flow \
    --work-plan-id "$WORK_PLAN_ID"
  assert_jq "$ARTIFACT_DIR/work-plan-flow.json" '.resource_kind == "work_plan" and .resource_id == "'"$WORK_PLAN_ID"'" and .work_plan.id == "'"$WORK_PLAN_ID"'" and .change_set == null and .pipeline_intent == null and ([.readiness.warnings[] | select(.code == "missing_change_set")] | length) == 1 and ([.remediation_plans[] | select(.id == "'"$REMEDIATION_PLAN_ID"'")] | length) == 1' "WorkPlan flow should aggregate pre-ChangeSet SDLC state"

  run_json change-set-create cargo run -q -p pharness-cli -- change-sets create \
    --work-plan-id "$WORK_PLAN_ID" \
    --title "ChangeSet: smoke source change" \
    --summary "Exercise deterministic SDLC control-plane chain" \
    --risk-level medium \
    --change-set-json '{"changes":[{"path":"deploy/checkout-api.yaml","diff":"--- before\n+++ after\n-replicas: 1\n+replicas: 2"}],"rollback":"restore replicas to 1"}' \
    --actor smoke \
    --reason "e2e smoke change set"
  assert_jq "$ARTIFACT_DIR/change-set-create.json" '.created == true and .change_set.work_plan_id == "'"$WORK_PLAN_ID"'" and .change_set.status == "draft"' "change set create should produce a draft"
  CHANGE_SET_ID="$(jq -r '.change_set.id' "$ARTIFACT_DIR/change-set-create.json")"

  run_json change-set-propose cargo run -q -p pharness-cli -- change-sets transition \
    --change-set-id "$CHANGE_SET_ID" \
    --target-status proposed \
    --actor smoke \
    --reason "e2e smoke change set proposed"
  assert_jq "$ARTIFACT_DIR/change-set-propose.json" '.change_set.id == "'"$CHANGE_SET_ID"'" and .change_set.status == "proposed"' "change set should transition to proposed"

  run_json change-set-approve cargo run -q -p pharness-cli -- change-sets transition \
    --change-set-id "$CHANGE_SET_ID" \
    --target-status approved \
    --actor smoke \
    --reason "e2e smoke change set approved"
  assert_jq "$ARTIFACT_DIR/change-set-approve.json" '.change_set.id == "'"$CHANGE_SET_ID"'" and .change_set.status == "approved"' "change set should transition to approved"

  run_json readiness-missing-envelope cargo run -q -p pharness-cli -- change-sets readiness \
    --change-set-id "$CHANGE_SET_ID"
  assert_jq "$ARTIFACT_DIR/readiness-missing-envelope.json" '.ready == false and ([.blockers[] | select(.code == "missing_active_trusted_envelope")] | length) == 1' "approved change set without trusted envelope should be blocked"

  run_json change-set-envelope cargo run -q -p pharness-cli -- change-sets create-trusted-envelope \
    --change-set-id "$CHANGE_SET_ID" \
    --created-by smoke \
    --reason "e2e smoke trusted ChangeSet envelope" \
    --environment local \
    --namespace apps-dev \
    --repo git@example.test/team/pharness.git \
    --branch smoke/e2e
  assert_jq "$ARTIFACT_DIR/change-set-envelope.json" '.grant.status == "active" and (.grant.scope.change_set_ids | index("'"$CHANGE_SET_ID"'")) != null' "approved change set should create an active trusted envelope"
  CHANGE_SET_GRANT_ID="$(jq -r '.grant.id' "$ARTIFACT_DIR/change-set-envelope.json")"

  run_json pipeline-intent-create cargo run -q -p pharness-cli -- pipeline-intents create-from-change-set \
    --change-set-id "$CHANGE_SET_ID" \
    --intent-json '{"execution":{"enabled":true,"namespace":"tekton-pipelines","pipeline_ref":"clone-build-push","production_impacting":false,"params":{},"workspaces":[]}}' \
    --actor smoke \
    --reason "e2e smoke pipeline intent"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-create.json" '.created == true and .pipeline_intent.change_set_id == "'"$CHANGE_SET_ID"'" and .pipeline_intent.status == "proposed"' "pipeline intent create should produce proposed intent"
  PIPELINE_INTENT_ID="$(jq -r '.pipeline_intent.id' "$ARTIFACT_DIR/pipeline-intent-create.json")"

  run_json pipeline-intent-approve cargo run -q -p pharness-cli -- pipeline-intents transition \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --target-status approved \
    --actor smoke \
    --reason "e2e smoke pipeline intent approved"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-approve.json" '.pipeline_intent.id == "'"$PIPELINE_INTENT_ID"'" and .pipeline_intent.status == "approved"' "pipeline intent should transition to approved"

  run_json pipeline-contract-create cargo run -q -p pharness-cli -- pipeline-contracts create \
    --namespace tekton-pipelines \
    --pipeline-ref clone-build-push \
    --version v1 \
    --contract-json '{"params":[],"workspaces":[]}' \
    --actor smoke \
    --reason "e2e smoke Pipeline contract"
  assert_jq "$ARTIFACT_DIR/pipeline-contract-create.json" '.status == "active" and .namespace == "tekton-pipelines" and .pipeline_ref == "clone-build-push"' "Pipeline contract should permit the smoke PipelineIntent inputs"
  PIPELINE_CONTRACT_ID="$(jq -r '.id' "$ARTIFACT_DIR/pipeline-contract-create.json")"

  run_json pipeline-contract-get cargo run -q -p pharness-cli -- pipeline-contracts get \
    --pipeline-contract-id "$PIPELINE_CONTRACT_ID"
  assert_jq "$ARTIFACT_DIR/pipeline-contract-get.json" '.id == "'"$PIPELINE_CONTRACT_ID"'" and .contract_json.params == [] and .contract_json.workspaces == []' "Pipeline contract should round-trip through the API"

  run_json pipeline-intent-envelope cargo run -q -p pharness-cli -- pipeline-intents create-trusted-envelope \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --created-by smoke \
    --reason "e2e smoke Tekton execution envelope"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-envelope.json" '.grant.status == "active" and (.grant.scope.pipeline_intent_ids | index("'"$PIPELINE_INTENT_ID"'")) != null and .grant.policy.policy_mode == "supervised_autonomy"' "approved PipelineIntent should create a supervised execution envelope"

  run_json pipeline-intent-execution-preview cargo run -q -p pharness-cli -- pipeline-intents execute \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --actor smoke \
    --reason "e2e smoke dry run"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-execution-preview.json" '.status == "ready" and .ready == true and .dry_run == true and .manifest.kind == "PipelineRun" and .manifest.metadata.namespace == "tekton-pipelines" and .pipeline_intent.status == "approved" and ([.checks[] | select(.code == "active_pipeline_contract" and .passed)] | length) == 1 and ([.checks[] | select(.code == "pipeline_contract_inputs" and .passed)] | length) == 1' "PipelineIntent preview should validate contract inputs without creating a PipelineRun"

  run_json pipeline-contract-replace cargo run -q -p pharness-cli -- pipeline-contracts replace \
    --pipeline-contract-id "$PIPELINE_CONTRACT_ID" \
    --version v2 \
    --contract-json '{"params":[],"workspaces":[]}' \
    --actor smoke \
    --reason "e2e smoke atomic contract replacement"
  assert_jq "$ARTIFACT_DIR/pipeline-contract-replace.json" '.retired_contract.status == "retired" and .retired_contract.version == "v1" and .pipeline_contract.status == "active" and .pipeline_contract.version == "v2"' "Pipeline contract replacement should retire v1 and activate v2 atomically"
  PIPELINE_CONTRACT_ID="$(jq -r '.pipeline_contract.id' "$ARTIFACT_DIR/pipeline-contract-replace.json")"

  run_json pipeline-intent-execution-replacement-contract cargo run -q -p pharness-cli -- pipeline-intents execute \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --actor smoke \
    --reason "e2e smoke replacement contract preflight"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-execution-replacement-contract.json" '.status == "ready" and .ready == true and ([.checks[] | select(.code == "active_pipeline_contract" and .passed and (.summary | contains("version v2")))] | length) == 1' "replacement Pipeline contract should keep the matching intent ready"

  run_json pipeline-contract-retire cargo run -q -p pharness-cli -- pipeline-contracts retire \
    --pipeline-contract-id "$PIPELINE_CONTRACT_ID" \
    --actor smoke \
    --reason "e2e smoke contract lifecycle"
  assert_jq "$ARTIFACT_DIR/pipeline-contract-retire.json" '.id == "'"$PIPELINE_CONTRACT_ID"'" and .status == "retired"' "Pipeline contract retirement should preserve the durable record"

  run_json pipeline-intent-execution-retired-contract cargo run -q -p pharness-cli -- pipeline-intents execute \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --actor smoke \
    --reason "e2e smoke retired contract preflight"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-execution-retired-contract.json" '.status == "blocked" and .ready == false and ([.checks[] | select(.code == "active_pipeline_contract" and (.passed | not) and (.summary | contains("retired")))] | length) == 1' "retired Pipeline contract should block execution preflight"

  check_cluster_capabilities
  ensure_deploy_ready_pipeline_evidence

  run_json deployment-intent-create cargo run -q -p pharness-cli -- deployment-intents create-from-pipeline-intent \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --target-environment dev \
    --target-namespace apps-dev \
    --argo-application checkout-api \
    --actor smoke \
    --reason "e2e smoke deployment intent"
  assert_jq "$ARTIFACT_DIR/deployment-intent-create.json" '.created == true and .deployment_intent.pipeline_intent_id == "'"$PIPELINE_INTENT_ID"'" and .deployment_intent.status == "proposed" and .deployment_intent.intent_json.pipeline_evidence.status == "'"$PIPELINE_EVIDENCE_STATUS"'" and .deployment_intent.intent_json.pipeline_evidence.deploy_ready == ("'"$PIPELINE_EVIDENCE_STATUS"'" == "satisfied")' "deployment intent create should produce proposed intent with inherited pipeline evidence state"
  DEPLOYMENT_INTENT_ID="$(jq -r '.deployment_intent.id' "$ARTIFACT_DIR/deployment-intent-create.json")"

  run_json deployment-intent-approve cargo run -q -p pharness-cli -- deployment-intents transition \
    --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
    --target-status approved \
    --actor smoke \
    --reason "e2e smoke deployment intent approved"
  assert_jq "$ARTIFACT_DIR/deployment-intent-approve.json" '.deployment_intent.id == "'"$DEPLOYMENT_INTENT_ID"'" and .deployment_intent.status == "approved"' "deployment intent should transition to approved"

  check_deployment_evidence

  run_json release-create cargo run -q -p pharness-cli -- releases create-from-deployment-intent \
    --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
    --version v0.1.0-smoke \
    --commit-sha abc1234 \
    --image-digest sha256:deadbeef \
    --rollback-ref previous-release \
    --actor smoke \
    --reason "e2e smoke release"
  assert_jq "$ARTIFACT_DIR/release-create.json" '.created == true and .release.deployment_intent_id == "'"$DEPLOYMENT_INTENT_ID"'" and .release.status == "proposed" and .release.release_json.deployment_evidence.status == "'"$DEPLOYMENT_EVIDENCE_STATUS"'" and .release.release_json.deployment_evidence.release_ready == ("'"$DEPLOYMENT_EVIDENCE_STATUS"'" == "satisfied")' "release create should produce proposed release with inherited deployment evidence state"
  RELEASE_ID="$(jq -r '.release.id' "$ARTIFACT_DIR/release-create.json")"

  run_json release-approve cargo run -q -p pharness-cli -- releases transition \
    --release-id "$RELEASE_ID" \
    --target-status approved \
    --actor smoke \
    --reason "e2e smoke release approved"
  assert_jq "$ARTIFACT_DIR/release-approve.json" '.release.id == "'"$RELEASE_ID"'" and .release.status == "approved"' "release should transition to approved"

  check_release_observability_evidence

  run_json registry-evidence-create cargo run -q -p pharness-cli -- registry-evidence create-from-inspection \
    --release-id "$RELEASE_ID" \
    --image-ref team/checkout-api:v0.1.0-smoke \
    --actor smoke \
    --reason "e2e smoke registry inspection evidence"
  assert_jq "$ARTIFACT_DIR/registry-evidence-create.json" '.created == true and .registry_evidence.release_id == "'"$RELEASE_ID"'" and .registry_evidence.status == "proposed" and .registry_evidence.source == "registry_inspect_image" and .registry_evidence.verification_status == "unknown" and .inspection.status == "ok" and .inspection.executed == true' "registry evidence create should inspect image identity and produce proposed evidence"
  REGISTRY_EVIDENCE_ID="$(jq -r '.registry_evidence.id' "$ARTIFACT_DIR/registry-evidence-create.json")"

  run_json registry-evidence-verify cargo run -q -p pharness-cli -- registry-evidence transition \
    --evidence-id "$REGISTRY_EVIDENCE_ID" \
    --target-status verified \
    --actor smoke \
    --reason "e2e smoke registry evidence verified"
  assert_jq "$ARTIFACT_DIR/registry-evidence-verify.json" '.registry_evidence.id == "'"$REGISTRY_EVIDENCE_ID"'" and .registry_evidence.status == "verified"' "registry evidence should transition to verified"

  run_json change-set-readiness cargo run -q -p pharness-cli -- change-sets readiness \
    --change-set-id "$CHANGE_SET_ID"
  assert_jq "$ARTIFACT_DIR/change-set-readiness.json" '.ready == true and .change_set.id == "'"$CHANGE_SET_ID"'" and .registry_evidence.status == "verified" and (.blockers | length) == 0 and ([.warnings[] | select(.code == "registry_evidence_verification_not_verified")] | length) == 1 and ([.warnings[] | select(.code == "missing_release_observability_evidence")] | length) == 0 and ([.warnings[] | select(.code == "release_observability_attention_required")] | length) == 1' "approved chain should be unblocked while retaining supply-chain and release observability warnings"

  run_json change-set-flow cargo run -q -p pharness-cli -- change-sets flow \
    --change-set-id "$CHANGE_SET_ID"
  assert_jq "$ARTIFACT_DIR/change-set-flow.json" '.resource_kind == "change_set" and .resource_id == "'"$CHANGE_SET_ID"'" and .readiness.ready == true and .change_set.id == "'"$CHANGE_SET_ID"'" and .work_plan.id == "'"$WORK_PLAN_ID"'" and .pipeline_intent.id == "'"$PIPELINE_INTENT_ID"'" and .deployment_intent.id == "'"$DEPLOYMENT_INTENT_ID"'" and .release.id == "'"$RELEASE_ID"'" and .registry_evidence.id == "'"$REGISTRY_EVIDENCE_ID"'" and ([.incidents[] | select(.id == "'"$RELEASE_OBSERVABILITY_INCIDENT_ID"'")] | length) == 1 and ([.remediation_plans[] | select(.id == "'"$RELEASE_OBSERVABILITY_REMEDIATION_PLAN_ID"'")] | length) == 1 and ([.approval_gates[] | select(.remediation_plan_id == "'"$RELEASE_OBSERVABILITY_REMEDIATION_PLAN_ID"'" and .gate_kind == "cluster_mutation")] | length) == 1 and ([.audit_events[] | select(.kind == "remediation_plan.created" and .resource_id == "'"$RELEASE_OBSERVABILITY_REMEDIATION_PLAN_ID"'")] | length) == 1' "ChangeSet flow should aggregate the SDLC chain, release observability remediation, gates, and audit events"

  run_json change-set-revise cargo run -q -p pharness-cli -- change-sets revise \
    --change-set-id "$CHANGE_SET_ID" \
    --summary "Exercise deterministic SDLC control-plane chain after material revision" \
    --change-set-json '{"changes":[{"path":"deploy/checkout-api.yaml","diff":"--- before\n+++ after\n-replicas: 1\n+replicas: 3"}],"rollback":"restore replicas to 1"}' \
    --actor smoke \
    --reason "e2e smoke material change invalidation"
  assert_jq "$ARTIFACT_DIR/change-set-revise.json" '.material_hash_changed == true and .change_set.status == "draft" and .invalidated_pipeline_intent.status == "stale" and .invalidated_deployment_intent.status == "stale" and .invalidated_release.status == "stale" and .invalidated_registry_evidence.status == "stale"' "material change set revision should stale downstream intents, release, and registry evidence"

  run_json readiness-after-revision cargo run -q -p pharness-cli -- change-sets readiness \
    --change-set-id "$CHANGE_SET_ID"
  assert_jq "$ARTIFACT_DIR/readiness-after-revision.json" '.ready == false and ([.blockers[] | select(.code == "change_set_not_approved")] | length) == 1 and ([.blockers[] | select(.code == "missing_active_trusted_envelope")] | length) == 1 and ([.warnings[] | select(.code == "stale_trusted_envelope")] | length) >= 2 and .pipeline_intent.status == "stale" and .deployment_intent.status == "stale" and .release.status == "stale" and .registry_evidence.status == "stale"' "material change should block readiness and expose stale filesystem and Tekton execution envelopes plus stale downstream evidence"

  run_json audit-stale-envelope cargo run -q -p pharness-cli -- audit-events \
    --resource-kind permission_grant \
    --resource-id "$CHANGE_SET_GRANT_ID"
  assert_jq "$ARTIFACT_DIR/audit-stale-envelope.json" '[.events[] | select(.kind == "permission_grant.stale")] | length == 1' "material change should audit stale trusted envelope"
}

check_deployment_evidence() {
  if [[ "$RUN_CLUSTER" != "1" ]]; then
    echo "==> deployment evidence skipped; pass --cluster and PHARNESS_E2E_ARGO_APP to attach live Argo evidence"
    return
  fi

  if [[ -z "${PHARNESS_E2E_ARGO_APP:-}" ]]; then
    echo "==> deployment evidence skipped because PHARNESS_E2E_ARGO_APP is not set"
    return
  fi

  run_json cluster-deployment-argo-app cargo run -q -p pharness-cli -- capabilities argo-get-app \
    --app "$PHARNESS_E2E_ARGO_APP" \
    --timeout-ms 30000
  assert_jq "$ARTIFACT_DIR/cluster-deployment-argo-app.json" '.status == "ok" and .executed == true and (.observation_id | type) == "string"' "cluster mode should read and persist configured Argo CD Application"
  local argo_observation_id
  argo_observation_id="$(jq -r '.observation_id' "$ARTIFACT_DIR/cluster-deployment-argo-app.json")"

  run_json cluster-deployment-attach-evidence cargo run -q -p pharness-cli -- deployment-intents attach-evidence \
    --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
    --observation-id "$argo_observation_id" \
    --actor smoke \
    --reason "e2e smoke live Argo evidence"
  assert_jq "$ARTIFACT_DIR/cluster-deployment-attach-evidence.json" '.deployment_intent.id == "'"$DEPLOYMENT_INTENT_ID"'" and .deployment_intent.status == "approved" and (.deployment_intent.intent_json.deployment_evidence.status | type) == "string" and .deployment_intent.intent_json.deployment_evidence.observation_id == "'"$argo_observation_id"'" and .observation.id == "'"$argo_observation_id"'"' "cluster mode should attach live Argo Application observation to the approved DeploymentIntent"
  DEPLOYMENT_EVIDENCE_STATUS="$(jq -r '.deployment_intent.intent_json.deployment_evidence.status' "$ARTIFACT_DIR/cluster-deployment-attach-evidence.json")"
}

release_observability_observation_id() {
  local candidate
  for candidate in cluster-prometheus-inventory cluster-loki-log-summary; do
    local file="$ARTIFACT_DIR/$candidate.json"
    if [[ -f "$file" ]]; then
      local observation_id
      observation_id="$(jq -r '.observation_id // empty' "$file")"
      if [[ -n "$observation_id" ]]; then
        echo "$observation_id"
        return 0
      fi
    fi
  done

  return 1
}

check_release_observability_evidence() {
  local observation_id
  if observation_id="$(release_observability_observation_id)"; then
    echo "==> release-observability using existing observation $observation_id"
  else
    run_json release-observability-observation cargo run -q -p pharness-cli -- observations create \
      --source prometheus \
      --kind inventory \
      --subject prometheus/inventory \
      --summary "synthetic Prometheus inventory for deterministic Release evidence smoke" \
      --resource-kind PrometheusInventory \
      --resource-name smoke \
      --resource-ref-json '{"source":"prometheus","kind":"inventory","name":"smoke"}' \
      --data-json '{"source":"prometheus","resource":"inventory","inventory":{"targets":{"active_count":1,"unhealthy_count":0},"rules":{"rule_count":1,"problem_rule_count":0},"alerts":{"alert_count":0}}}' \
      --actor smoke \
      --reason "e2e smoke release observability evidence"
    assert_jq "$ARTIFACT_DIR/release-observability-observation.json" '.source == "prometheus" and .kind == "inventory"' "deterministic Release observability observation should be persisted"
    observation_id="$(jq -r '.id' "$ARTIFACT_DIR/release-observability-observation.json")"
  fi

  run_json release-attach-observability cargo run -q -p pharness-cli -- releases attach-evidence \
    --release-id "$RELEASE_ID" \
    --observation-id "$observation_id" \
    --actor smoke \
    --reason "e2e smoke release observability reviewed"
  assert_jq "$ARTIFACT_DIR/release-attach-observability.json" '.release.id == "'"$RELEASE_ID"'" and .release.release_json.observability_evidence[0].observation_id == "'"$observation_id"'" and (.release.release_json.observability_evidence[0].status | type) == "string"' "Release should attach Prometheus or Loki observability evidence"
  RELEASE_OBSERVABILITY_STATUS="$(jq -r '.release.release_json.observability_evidence[0].status' "$ARTIFACT_DIR/release-attach-observability.json")"

  run_json release-observability-alert-observation cargo run -q -p pharness-cli -- observations create \
    --source prometheus \
    --kind inventory \
    --subject prometheus/inventory \
    --summary "synthetic Prometheus inventory with active alert for Release remediation smoke" \
    --resource-kind PrometheusInventory \
    --resource-name smoke-alert \
    --resource-ref-json '{"source":"prometheus","kind":"inventory","name":"smoke-alert"}' \
    --data-json '{"source":"prometheus","resource":"inventory","inventory":{"targets":{"active_count":3,"unhealthy_count":1},"rules":{"rule_count":2,"problem_rule_count":1},"alerts":{"alert_count":1}}}' \
    --actor smoke \
    --reason "e2e smoke release observability alert evidence"
  assert_jq "$ARTIFACT_DIR/release-observability-alert-observation.json" '.source == "prometheus" and .kind == "inventory"' "attention-required Release observability observation should be persisted"

  local alert_observation_id
  alert_observation_id="$(jq -r '.id' "$ARTIFACT_DIR/release-observability-alert-observation.json")"
  run_json release-attach-observability-alert cargo run -q -p pharness-cli -- releases attach-evidence \
    --release-id "$RELEASE_ID" \
    --observation-id "$alert_observation_id" \
    --actor smoke \
    --reason "e2e smoke release observability alert reviewed"
  assert_jq "$ARTIFACT_DIR/release-attach-observability-alert.json" '.release.id == "'"$RELEASE_ID"'" and .release.release_json.observability_evidence[-1].observation_id == "'"$alert_observation_id"'" and .release.release_json.observability_evidence[-1].status == "attention_required" and .incident.status == "candidate" and .incident.severity == "high" and .incident.observation_id == "'"$alert_observation_id"'" and .remediation_plan.status == "draft" and .remediation_plan.incident_id == .incident.id and .remediation_plan.requires_approval == true' "attention-required Release observability should create candidate Incident and draft RemediationPlan"

  local release_incident_id
  local release_remediation_plan_id
  release_incident_id="$(jq -r '.incident.id' "$ARTIFACT_DIR/release-attach-observability-alert.json")"
  release_remediation_plan_id="$(jq -r '.remediation_plan.id' "$ARTIFACT_DIR/release-attach-observability-alert.json")"
  RELEASE_OBSERVABILITY_INCIDENT_ID="$release_incident_id"
  RELEASE_OBSERVABILITY_REMEDIATION_PLAN_ID="$release_remediation_plan_id"

  run_json release-observability-remediation-plans cargo run -q -p pharness-cli -- remediation-plans list \
    --incident-id "$release_incident_id"
  assert_jq "$ARTIFACT_DIR/release-observability-remediation-plans.json" '.count == 1 and .remediation_plans[0].id == "'"$release_remediation_plan_id"'" and .remediation_plans[0].status == "draft" and .remediation_plans[0].requires_approval == true and .remediation_plans[0].plan_json.source == "release_observability_evidence"' "Release observability RemediationPlan should be queryable by incident"

  run_json release-observability-approval-gates cargo run -q -p pharness-cli -- approval-gates list \
    --remediation-plan-id "$release_remediation_plan_id" \
    --incident-id "$release_incident_id"
  assert_jq "$ARTIFACT_DIR/release-observability-approval-gates.json" '.count == 4 and ([.approval_gates[] | select(.status == "pending")] | length) == 4 and ([.approval_gates[] | select(.gate_kind == "cluster_mutation")] | length) == 1 and ([.approval_gates[] | select(.gate_kind == "production_impact")] | length) == 1' "Release observability RemediationPlan should create pending approval gates"

  run_json audit-release-observability-remediation cargo run -q -p pharness-cli -- audit-events \
    --resource-kind remediation_plan \
    --resource-id "$release_remediation_plan_id"
  assert_jq "$ARTIFACT_DIR/audit-release-observability-remediation.json" '[.events[] | select(.kind == "remediation_plan.created" and .actor == "smoke")] | length == 1' "Release observability RemediationPlan creation should be audited"

  RELEASE_OBSERVABILITY_STATUS="$(jq -r '.release.release_json.observability_evidence[-1].status' "$ARTIFACT_DIR/release-attach-observability-alert.json")"
  RELEASE_OBSERVABILITY_REMEDIATION_STATUS="created"
}

check_cluster_capabilities() {
  if [[ "$RUN_CLUSTER" != "1" ]]; then
    echo "==> cluster checks skipped; pass --cluster to run live read-only cluster checks"
    return
  fi

  need kubectl
  kubectl version --client=true >/dev/null
  CLUSTER_STATUS="completed"

  run_json cluster-tekton-pipelineruns cargo run -q -p pharness-cli -- capabilities tekton-get-pipeline-runs \
    --all-namespaces \
    --timeout-ms 30000
  assert_jq "$ARTIFACT_DIR/cluster-tekton-pipelineruns.json" '.status == "ok" and .executed == true and .result.content.output.item_count >= 0' "cluster mode should read Tekton PipelineRuns"

  run_json cluster-tekton-taskruns cargo run -q -p pharness-cli -- capabilities tekton-get-task-runs \
    --all-namespaces \
    --timeout-ms 30000
  assert_jq "$ARTIFACT_DIR/cluster-tekton-taskruns.json" '.status == "ok" and .executed == true and .result.content.output.item_count >= 0' "cluster mode should read Tekton TaskRuns"

  local pipeline_run_name
  local pipeline_run_namespace
  pipeline_run_name="$(jq -r '.result.content.output.items[0].metadata.name // empty' "$ARTIFACT_DIR/cluster-tekton-pipelineruns.json")"
  pipeline_run_namespace="$(jq -r '.result.content.output.items[0].metadata.namespace // empty' "$ARTIFACT_DIR/cluster-tekton-pipelineruns.json")"
  if [[ -n "$pipeline_run_name" ]] && [[ -n "$pipeline_run_namespace" ]]; then
    run_json cluster-tekton-analysis cargo run -q -p pharness-cli -- capabilities tekton-analyze-pipeline-run \
      --namespace "$pipeline_run_namespace" \
      --name "$pipeline_run_name" \
      --timeout-ms 30000
    assert_jq "$ARTIFACT_DIR/cluster-tekton-analysis.json" '.status == "ok" and .executed == true and (.artifact_id | type) == "string" and (.observation_id | type) == "string" and .result.content.analysis.kind == "PipelineRunAnalysis"' "cluster mode should analyze and persist one concrete PipelineRun"
    local analysis_artifact_id
    local analysis_observation_id
    analysis_artifact_id="$(jq -r '.artifact_id' "$ARTIFACT_DIR/cluster-tekton-analysis.json")"
    analysis_observation_id="$(jq -r '.observation_id' "$ARTIFACT_DIR/cluster-tekton-analysis.json")"

    run_json cluster-tekton-analysis-artifact cargo run -q -p pharness-cli -- artifacts get \
      --artifact-id "$analysis_artifact_id"
    assert_jq "$ARTIFACT_DIR/cluster-tekton-analysis-artifact.json" '.id == "'"$analysis_artifact_id"'" and .kind == "pipeline_run_analysis" and .content_json.analysis.kind == "PipelineRunAnalysis"' "cluster mode should persist PipelineRunAnalysis artifact"

    run_json cluster-tekton-analysis-observation cargo run -q -p pharness-cli -- observations get \
      --observation-id "$analysis_observation_id"
    assert_jq "$ARTIFACT_DIR/cluster-tekton-analysis-observation.json" '.id == "'"$analysis_observation_id"'" and .source == "tekton" and .kind == "pipeline_run_analysis" and .resource_namespace == "'"$pipeline_run_namespace"'" and .resource_name == "'"$pipeline_run_name"'" and .artifact_id == "'"$analysis_artifact_id"'"' "cluster mode should persist PipelineRunAnalysis observation"

    run_json cluster-tekton-analysis-observation-list cargo run -q -p pharness-cli -- observations list \
      --source tekton \
      --kind pipeline_run_analysis \
      --resource-namespace "$pipeline_run_namespace" \
      --resource-kind PipelineRun \
      --resource-name "$pipeline_run_name" \
      --limit 10
    assert_jq "$ARTIFACT_DIR/cluster-tekton-analysis-observation-list.json" '.count >= 1 and ([.observations[] | select(.id == "'"$analysis_observation_id"'")] | length) == 1' "cluster mode should index PipelineRunAnalysis observations by resource identity"

    run_json cluster-pipeline-intent-attach-evidence cargo run -q -p pharness-cli -- pipeline-intents attach-evidence \
      --pipeline-intent-id "$PIPELINE_INTENT_ID" \
      --observation-id "$analysis_observation_id" \
      --actor smoke \
      --reason "e2e smoke live Tekton evidence"
    assert_jq "$ARTIFACT_DIR/cluster-pipeline-intent-attach-evidence.json" '.pipeline_intent.id == "'"$PIPELINE_INTENT_ID"'" and .pipeline_intent.status == "approved" and (.pipeline_intent.intent_json.evidence.status | type) == "string" and (.pipeline_intent.intent_json.evidence.summary.image_alignment_status != "registry_mismatch" or .pipeline_intent.intent_json.evidence.status == "attention_required") and .pipeline_intent.intent_json.evidence.observation_id == "'"$analysis_observation_id"'" and .observation.id == "'"$analysis_observation_id"'"' "cluster mode should attach live PipelineRunAnalysis observation to the approved PipelineIntent without hiding image mismatch risk"
    PIPELINE_EVIDENCE_STATUS="$(jq -r '.pipeline_intent.intent_json.evidence.status' "$ARTIFACT_DIR/cluster-pipeline-intent-attach-evidence.json")"
  else
    echo "==> cluster-tekton-analysis skipped because no PipelineRuns were returned"
  fi

  if [[ -n "${PHARNESS_E2E_ARGO_APP:-}" ]]; then
    run_json cluster-argo-app cargo run -q -p pharness-cli -- capabilities argo-get-app \
      --app "$PHARNESS_E2E_ARGO_APP" \
      --timeout-ms 30000
    assert_jq "$ARTIFACT_DIR/cluster-argo-app.json" '.status == "ok" and .executed == true' "cluster mode should read configured Argo CD Application"
  else
    echo "==> cluster-argo-app skipped because PHARNESS_E2E_ARGO_APP is not set"
  fi

  if [[ -n "${PHARNESS_PROMETHEUS_URL:-}" ]]; then
    run_json cluster-prometheus-inventory cargo run -q -p pharness-cli -- capabilities prometheus-inventory \
      --timeout-ms 30000
    assert_jq "$ARTIFACT_DIR/cluster-prometheus-inventory.json" '.status == "ok" and .executed == true' "cluster mode should read Prometheus inventory when configured"
  else
    echo "==> cluster-prometheus-inventory skipped because PHARNESS_PROMETHEUS_URL is not set"
  fi

  if [[ -n "${PHARNESS_LOKI_URL:-}" ]] && [[ -n "${PHARNESS_E2E_LOKI_QUERY:-}" ]]; then
    run_json cluster-loki-log-summary cargo run -q -p pharness-cli -- capabilities loki-log-summary \
      --query "$PHARNESS_E2E_LOKI_QUERY" \
      --since-seconds "${PHARNESS_E2E_LOKI_SINCE_SECONDS:-3600}" \
      --limit "${PHARNESS_E2E_LOKI_LIMIT:-20}" \
      --timeout-ms 30000
    assert_jq "$ARTIFACT_DIR/cluster-loki-log-summary.json" '.status == "ok" and .executed == true' "cluster mode should read bounded Loki logs when configured"
  else
    echo "==> cluster-loki-log-summary skipped because PHARNESS_LOKI_URL or PHARNESS_E2E_LOKI_QUERY is not set"
  fi
}

ensure_deploy_ready_pipeline_evidence() {
  if [[ "$PIPELINE_EVIDENCE_STATUS" == "satisfied" ]]; then
    return
  fi

  if [[ "$RUN_CLUSTER" == "1" ]]; then
    echo "cluster PipelineRunAnalysis evidence is not satisfied; refusing to approve a downstream DeploymentIntent" >&2
    return 1
  fi

  run_json pipeline-evidence-fixture cargo run -q -p pharness-cli -- observations create \
    --id "obs_e2e_pipeline_analysis" \
    --source tekton \
    --kind pipeline_run_analysis \
    --subject clone-build-push \
    --summary "Deterministic successful PipelineRunAnalysis fixture" \
    --resource-namespace tekton-pipelines \
    --resource-kind PipelineRun \
    --resource-name pharness-e2e-pipeline \
    --resource-ref-json '{"apiVersion":"tekton.dev/v1","kind":"PipelineRun","namespace":"tekton-pipelines","name":"pharness-e2e-pipeline"}' \
    --data-json '{"analysis":{"summary":{"status":"succeeded","failed_task_run_count":0,"running_task_run_count":0,"succeeded_task_run_count":1,"image_alignment":{"status":"exact_match"}}}}' \
    --actor smoke \
    --reason "deterministic PipelineRunAnalysis fixture"
  assert_jq "$ARTIFACT_DIR/pipeline-evidence-fixture.json" '.id == "obs_e2e_pipeline_analysis" and .source == "tekton" and .kind == "pipeline_run_analysis"' "deterministic smoke should create a successful PipelineRunAnalysis fixture"

  run_json pipeline-intent-attach-fixture-evidence cargo run -q -p pharness-cli -- pipeline-intents attach-evidence \
    --pipeline-intent-id "$PIPELINE_INTENT_ID" \
    --observation-id obs_e2e_pipeline_analysis \
    --actor smoke \
    --reason "deterministic pipeline evidence attached"
  assert_jq "$ARTIFACT_DIR/pipeline-intent-attach-fixture-evidence.json" '.pipeline_intent.intent_json.evidence.status == "satisfied" and .observation.id == "obs_e2e_pipeline_analysis"' "successful PipelineRunAnalysis fixture should make downstream deployment eligible"
  PIPELINE_EVIDENCE_STATUS="satisfied"
}

check_model_run() {
  if [[ "$RUN_MODEL" == "never" ]]; then
    echo "==> model-run skipped by --no-model"
    return
  fi

  if [[ -z "${FIREWORKS_API_KEY:-}" ]]; then
    if [[ "$RUN_MODEL" == "always" ]]; then
      echo "FIREWORKS_API_KEY is required when --model is passed" >&2
      exit 1
    fi
    echo "==> model-run skipped because FIREWORKS_API_KEY is not set"
    return
  fi

  run_json model-run cargo run -q -p pharness-cli -- run \
    --follow-events \
    --task "List the top-level files, then finish with one sentence." \
    --cwd "$ROOT" \
    --timeout-ms "${PHARNESS_E2E_MODEL_TIMEOUT_MS:-300000}"
  assert_jq "$ARTIFACT_DIR/model-run.json" '.wait_status == "completed" and .run.status == "completed"' "model-backed run should complete"
  MODEL_RUN_ID="$(jq -r '.run.id' "$ARTIFACT_DIR/model-run.json")"
  run_json model-run-events cargo run -q -p pharness-cli -- runs get \
    --run-id "$MODEL_RUN_ID" \
    --with-events
  assert_jq "$ARTIFACT_DIR/model-run-events.json" '.events | length >= 4' "model run should persist events"
  MODEL_STATUS="completed"
}

write_manifest() {
  jq -n \
    --arg api_url "$API_URL" \
    --arg artifact_dir "$ARTIFACT_ROOT/$RUN_NAME" \
    --arg model_run "$MODEL_STATUS" \
    --arg cluster_run "$CLUSTER_STATUS" \
    --arg release_observability "$RELEASE_OBSERVABILITY_STATUS" \
    --arg release_observability_remediation "$RELEASE_OBSERVABILITY_REMEDIATION_STATUS" \
    '{
      status: "passed",
      api_url: $api_url,
      artifact_dir: $artifact_dir,
      model_run: $model_run,
      cluster_run: $cluster_run,
      release_observability: $release_observability,
      release_observability_remediation: $release_observability_remediation,
      checks: ([
        "health",
        "config",
        "runs_summary",
        "secret_denial",
        "registry_inspect",
        "audit_secret_denial",
        "audit_registry",
        "event_stream_cursor",
        "sdlc_root_chain",
        "sdlc_downstream_chain",
        "pipeline_evidence_gate",
        "work_plan_flow",
        "release_observability_evidence",
        "release_observability_remediation",
        "change_set_flow"
      ] + (if $cluster_run == "completed" then ["cluster_read_only_capabilities"] else [] end))
    }' >"$ARTIFACT_DIR/manifest.json"

  jq . "$ARTIFACT_DIR/manifest.json"
  echo "pharness e2e smoke passed; artifacts are in $ARTIFACT_ROOT/$RUN_NAME"
}

main() {
  parse_args "$@"
  need cargo
  need curl
  need jq
  need python3

  prepare_artifacts
  trap cleanup EXIT

  start_api
  check_local_contract
  check_sdlc_roots
  check_sdlc_downstream
  check_model_run
  write_manifest
}

main "$@"
