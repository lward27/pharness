#!/usr/bin/env bash
# In-cluster qualification of Campus teardown (LUC-133).
#
# Runs the two ignored pharness-api tests in campus/tests/teardown_live.rs:
#   1. campus_teardown_live_stuck: in a probe namespace labelled as slot
#      campus-lab-slot-1, the janitor quarantines and then removes another
#      journal's session, and a Pod or Job held by an operator finalizer
#      leaves the cleanup blocked (slot quarantined, finalizer untouched)
#      until the operator removes the finalizer.
#   2. campus_teardown_live_cycles: create, expire and stop cycles (default
#      100) in the Argo-managed slots campus-lab-slot-1..5 with the approved
#      runner images, measuring broker revocation and resource removal.
#
# The journal and reconciler run in process; Kubernetes calls use the
# controller identity (impersonated). Preconditions require the slots to be
# empty of controller objects, so cleanup may remove any it finds there.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTEXT="lucas_engineering"
CHART="${CAMPUS_LAB_CHART:-}"
SLOTS=(campus-lab-slot-1 campus-lab-slot-2 campus-lab-slot-3 campus-lab-slot-4 campus-lab-slot-5)
PROBE_NS="campus-teardown-probe-1"
PROBE_SLOT_ID="campus-lab-slot-1"
PROBE_LABEL="pharness.dev/campus-lab-probe"
PROBE_FINALIZER="pharness.dev/luc-133-probe"
CONTROLLER_LABEL="pharness.dev/campus-controller"
GUARD="pharness-campus-lab-session-guard"
SECRET_GUARD="pharness-campus-lab-secret-guard"
# The controller identity lives with the Campus API in campus-poc (LUC-137).
CONTROLLER_SA="${CAMPUS_CONTROLLER_SA:-system:serviceaccount:campus-poc:pharness-campus-controller}"
CYCLES=100
OUT=""
SKIP_STUCK=false
SKIP_CYCLES=false

usage() {
  cat <<USAGE
Usage: $0 --chart <lucas_engineering>/charts/campus-lab [--context <ctx>] [--out <dir>]
          [--cycles <multiple of 5>] [--skip-stuck] [--skip-cycles]

Needs cluster-admin on the context, helm, jq and cargo. The controller
identity is impersonated as ${CONTROLLER_SA}.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --context) CONTEXT="$2"; shift 2 ;;
    --chart) CHART="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --cycles) CYCLES="$2"; shift 2 ;;
    --skip-stuck) SKIP_STUCK=true; shift ;;
    --skip-cycles) SKIP_CYCLES=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ -n "$CHART" && -f "${CHART}/Chart.yaml" ]] || { usage >&2; exit 2; }
OUT="${OUT:-$(mktemp -d /tmp/campus-teardown.XXXXXX)}"
mkdir -p "$OUT"

PASSED=0
FAILED=0
pass() { PASSED=$((PASSED + 1)); echo "PASS $*"; }
fail() { FAILED=$((FAILED + 1)); echo "FAIL $*"; }
k() { kubectl --context "$CONTEXT" "$@"; }

live() {
  local name="$1"; shift
  env PHARNESS_CAMPUS_TEARDOWN_LIVE_CONTROLLER_ARGS="--context ${CONTEXT} --as ${CONTROLLER_SA}" \
    PHARNESS_CAMPUS_TEARDOWN_LIVE_ADMIN_ARGS="--context ${CONTEXT}" \
    PHARNESS_CAMPUS_TEARDOWN_LIVE_OUT="$OUT" PHARNESS_CAMPUS_TEARDOWN_LIVE_CYCLES="$CYCLES" \
    PHARNESS_CAMPUS_TEARDOWN_LIVE_PROBE_NAMESPACE="$PROBE_NS" "$@" \
    cargo test --quiet --manifest-path "${ROOT}/Cargo.toml" -p pharness-api "$name" \
    -- --ignored --nocapture >"${OUT}/${name}.log" 2>&1 &&
    grep -q 'test result: ok. 1 passed' "${OUT}/${name}.log"
}

# Only the operator finalizer this script's test adds; the controller's and
# the Job controller's finalizers are left alone.
drop_probe_finalizer() {
  local ns="$1" kind="$2"
  k -n "$ns" get "$kind" -o json 2>/dev/null |
    jq -r --arg f "$PROBE_FINALIZER" '.items[] | select((.metadata.finalizers // []) | index($f)) | .metadata.name' |
    while read -r name; do
      current="$(k -n "$ns" get "$kind" "$name" -o json | jq -c '.metadata.finalizers')"
      next="$(jq -c --arg f "$PROBE_FINALIZER" 'map(select(. != $f))' <<<"$current")"
      k -n "$ns" patch "$kind" "$name" --type=json \
        -p "[{\"op\":\"test\",\"path\":\"/metadata/finalizers\",\"value\":${current}},{\"op\":\"replace\",\"path\":\"/metadata/finalizers\",\"value\":${next}}]" >/dev/null &&
        echo "removed the probe finalizer from ${kind}/${name}"
    done
}

cleanup() {
  local status=$?
  set +e
  echo "== Cleanup"
  if k get namespace "$PROBE_NS" >/dev/null 2>&1; then
    drop_probe_finalizer "$PROBE_NS" pods
    drop_probe_finalizer "$PROBE_NS" jobs.batch
    k delete namespace "$PROBE_NS" --ignore-not-found --wait=false >/dev/null 2>&1
  fi
  k delete validatingadmissionpolicybinding "${GUARD}-probe" "${SECRET_GUARD}-probe" --ignore-not-found >/dev/null 2>&1
  for ns in "${SLOTS[@]}"; do
    k -n "$ns" delete jobs.batch,secrets,configmaps -l "$CONTROLLER_LABEL" --ignore-not-found --wait=false >/dev/null 2>&1
  done
  local remaining="" left=""
  for _ in $(seq 1 100); do
    remaining="$(k get namespace "$PROBE_NS" -o name 2>/dev/null)"
    left=""
    for ns in "${SLOTS[@]}"; do
      left+="$(k -n "$ns" get jobs.batch,pods,secrets,configmaps -l "$CONTROLLER_LABEL" -o name 2>/dev/null | sed "s|^|${ns}/|")"
    done
    [[ -z "$remaining" && -z "$left" ]] && break
    sleep 3
  done
  echo "remaining probe namespace: ${remaining:-none}"
  echo "remaining controller objects in the slots: ${left:-none}"
  echo "evidence: ${OUT}"
  exit "$status"
}
trap cleanup EXIT

echo "== Preconditions (evidence ${OUT})"
app="$(k -n argocd get applications.argoproj.io campus-lab -o jsonpath='{.status.sync.status}/{.status.health.status}/{.status.sync.revision}')"
[[ "$app" == Synced/Healthy/* ]] && pass "Argo campus-lab ${app}" || fail "Argo campus-lab ${app}"
for ns in "${SLOTS[@]}"; do
  got="$(k -n "$ns" get jobs.batch,pods,secrets,configmaps -l "$CONTROLLER_LABEL" -o name)"
  slot="$(k get namespace "$ns" -o jsonpath='{.metadata.labels.pharness\.dev/campus-slot}')"
  [[ -z "$got" && "$slot" == "$ns" ]] && pass "${ns} is empty and labelled as slot ${slot}" ||
    { fail "${ns}: slot label ${slot}, objects ${got}"; exit 1; }
done
if k get namespace "$PROBE_NS" >/dev/null 2>&1; then fail "${PROBE_NS} already exists"; exit 1; fi
# A deployed Campus API reconciles the same slots; its janitor and this
# run's would each see the other's sessions as orphans.
api="$(k -n campus-poc get pods -l app=pharness-campus-api -o name 2>/dev/null)"
[[ -z "$api" ]] && pass "no Campus API runs against the slots" ||
  { fail "a Campus API runs against the slots (scale it to zero first): ${api}"; exit 1; }
echo "building the live tests"
cargo test --quiet --manifest-path "${ROOT}/Cargo.toml" -p pharness-api --no-run >"${OUT}/build.log" 2>&1 ||
  { fail "build, see ${OUT}/build.log"; exit 1; }

if [[ "$SKIP_STUCK" != true ]]; then
  echo "== Stuck finalizers and orphans (${PROBE_NS} as slot ${PROBE_SLOT_ID})"
  k create namespace "$PROBE_NS" >/dev/null
  k label namespace "$PROBE_NS" "${PROBE_LABEL}=true" "pharness.dev/campus-slot=${PROBE_SLOT_ID}" \
    pharness.dev/purpose=luc-133-probe pod-security.kubernetes.io/enforce=restricted >/dev/null
  helm template campus-lab "$CHART" --set "probe.namespace=${PROBE_NS}" | k apply -f - >/dev/null
  started=$SECONDS
  if live campus_teardown_live_stuck; then
    pass "stuck finalizer and orphan phase ($((SECONDS - started))s)"
  else
    fail "stuck finalizer and orphan phase"
  fi
  grep -E '^(@@CAMPUS|PASS|FAIL)|panicked' "${OUT}/campus_teardown_live_stuck.log" || tail -40 "${OUT}/campus_teardown_live_stuck.log"
  left="$(k -n "$PROBE_NS" get jobs.batch,pods,secrets,configmaps -l "$CONTROLLER_LABEL" -o name)"
  [[ -z "$left" ]] && pass "${PROBE_NS} has no controller objects left" || fail "${PROBE_NS} leftovers: ${left}"
  k delete namespace "$PROBE_NS" --wait=false >/dev/null
  k delete validatingadmissionpolicybinding "${GUARD}-probe" "${SECRET_GUARD}-probe" --ignore-not-found >/dev/null
fi

if [[ "$SKIP_CYCLES" != true ]]; then
  echo "== ${CYCLES} create, expire and stop cycles (${SLOTS[*]})"
  started=$SECONDS
  if live campus_teardown_live_cycles; then
    pass "${CYCLES} cycles ($((SECONDS - started))s)"
  else
    fail "${CYCLES} cycles"
  fi
  grep -E '^(PASS|FAIL)|panicked' "${OUT}/campus_teardown_live_cycles.log" || tail -40 "${OUT}/campus_teardown_live_cycles.log"
  [[ -f "${OUT}/teardown-cycles.json" ]] && { cat "${OUT}/teardown-cycles.json"; echo; }
  for ns in "${SLOTS[@]}"; do
    left="$(k -n "$ns" get jobs.batch,pods,secrets,configmaps -l "$CONTROLLER_LABEL" -o name)"
    [[ -z "$left" ]] && pass "${ns} has no controller objects left" || fail "${ns} leftovers: ${left}"
  done
fi

echo "== Result: ${PASSED} passed, ${FAILED} failed"
[[ "$FAILED" == 0 ]]
