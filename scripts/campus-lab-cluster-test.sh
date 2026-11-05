#!/usr/bin/env bash
set -euo pipefail

# In-cluster qualification of the Campus image admission policy and the
# per-session package cache controls. Everything runs in one existing
# namespace; the script labels it only for the duration of the run, deletes
# the Pods it creates and removes the test-only policy on exit.
#
# Phase 1 binds a test-only copy of the policy, rendered by the same code from
# scripts/tests/fixtures/campus/admission-test-evidence.json, so allowlisted
# admission and cache isolation can be shown while no runner digest is
# approved. Phase 2 binds the committed policy rendered from
# deploy/campus/approved-images.json.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
GATE=(python3 "${SCRIPT_DIR}/campus_image_gate.py")
CONTEXT="${PHARNESS_CLUSTER_CONTEXT:-lucas_engineering}"
NAMESPACE="campus-lab-spike"
SCOPE_NAMESPACE="tekton-pipelines"
KEEP_POLICY=true
FIXTURE="${SCRIPT_DIR}/tests/fixtures/campus/admission-test-evidence.json"
TEST_POLICY="pharness-campus-lab-image-allowlist-test"
TEST_LABEL="pharness.dev/campus-lab-policy-test"
REAL_LABEL="pharness.dev/campus-lab"
REAL_MANIFEST="${ROOT}/deploy/campus/admission/campus-lab-image-allowlist.yaml"
SOURCES="${ROOT}/deploy/campus/package-sources.json"

PYTHON_IMAGE="registry.lucas.engineering/pharness-python-runner@sha256:0dcdb9e74355d55c634e6adaf9643b4466557aa728130e752408d62baf2f9756"
NODE_IMAGE="registry.lucas.engineering/pharness-node-runner@sha256:c478a31d341af1d1c7a13a99236120c870d0b88d1a9b80ac59159dd1a145a7bd"
UNKNOWN_IMAGE="registry.lucas.engineering/pharness-runtime@sha256:95a4e5ab8f944b484067dd4681ce88d3e6ea85bc84b5cefc6adb3f640c2f94c7"
TAMPERED_IMAGE="${PYTHON_IMAGE%?}7"
TAG_IMAGE="registry.lucas.engineering/pharness-python-runner:git-8f7338c792fc88d4b2b700b45b2161108b39e0bf"
LATEST_IMAGE="registry.lucas.engineering/pharness-python-runner:latest"
TAG_AND_DIGEST_IMAGE="registry.lucas.engineering/pharness-python-runner:latest@${PYTHON_IMAGE#*@}"

usage() {
  cat >&2 <<EOF
Usage: $0 [--context <kubectl-context>] [--namespace <namespace>] [--remove-policy]

Runs the admission matrix and the contaminated-cache fixture in ${NAMESPACE}.
The committed ValidatingAdmissionPolicy is applied and left in place unless
--remove-policy is given; it only binds namespaces labelled ${REAL_LABEL}=true.
EOF
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --context) CONTEXT="${2:-}"; shift 2 ;;
    --namespace) NAMESPACE="${2:-}"; shift 2 ;;
    --remove-policy) KEEP_POLICY=false; shift ;;
    *) usage ;;
  esac
done
[[ "$CONTEXT" =~ ^[A-Za-z0-9][A-Za-z0-9._/-]*$ ]] || usage
[[ "$NAMESPACE" =~ ^[a-z0-9]([-a-z0-9]*[a-z0-9])?$ ]] || usage

k() { kubectl --context "$CONTEXT" "$@"; }

TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/campus-lab-cluster-test.XXXXXX")"
RUN_ID="$(date -u +%H%M%S)"
CREATED_PODS=()
cleanup() {
  local pod
  for pod in "${CREATED_PODS[@]}"; do
    k -n "$NAMESPACE" delete pod "$pod" --ignore-not-found --wait=false >/dev/null 2>&1 || true
  done
  k label namespace "$NAMESPACE" "${TEST_LABEL}-" "${REAL_LABEL}-" >/dev/null 2>&1 || true
  k delete validatingadmissionpolicybinding "$TEST_POLICY" --ignore-not-found >/dev/null 2>&1 || true
  k delete validatingadmissionpolicy "$TEST_POLICY" --ignore-not-found >/dev/null 2>&1 || true
  if [[ "$KEEP_POLICY" != true ]]; then
    k delete -f "$REAL_MANIFEST" --ignore-not-found >/dev/null 2>&1 || true
  fi
  if [[ -n "${TEMP_ROOT:-}" && "$TEMP_ROOT" == */campus-lab-cluster-test.* ]]; then
    rm -rf -- "$TEMP_ROOT"
  fi
}
trap cleanup EXIT

FAILURES=0
pass() { printf 'PASS  %s\n' "$*"; }
fail() { printf 'FAIL  %s\n' "$*"; FAILURES=$((FAILURES + 1)); }

# Pod manifest with the lab dependency controls and the given images.
pod_manifest() {
  local name="$1" image="$2" init_image="${3:-}"
  "${GATE[@]}" render-lab-pod --name "$name" --namespace "$NAMESPACE" --image "$PYTHON_IMAGE" \
    --session "$name" --sources "$SOURCES" --container-command sleep 600 |
    jq --arg image "$image" --arg init "$init_image" '
      .spec.containers[0].image = $image
      | if $init == "" then . else .spec.initContainers = [(.spec.containers[0]
          | .name = "setup" | .image = $init | .command = ["true"])] end'
}

workload_manifest() {
  local kind="$1" name="$2" image="$3"
  pod_manifest "$name" "$image" | jq --arg kind "$kind" --arg name "$name" '
    (.spec | .restartPolicy = (if $kind == "Deployment" then "Always" else "Never" end)) as $spec
    | {metadata: {labels: .metadata.labels}, spec: $spec} as $template
    | if $kind == "Job" then
        {apiVersion: "batch/v1", kind: "Job", metadata: {name: $name, namespace: .metadata.namespace},
         spec: {backoffLimit: 0, template: $template}}
      elif $kind == "CronJob" then
        {apiVersion: "batch/v1", kind: "CronJob", metadata: {name: $name, namespace: .metadata.namespace},
         spec: {schedule: "0 0 1 1 *", suspend: true, jobTemplate: {spec: {backoffLimit: 0, template: $template}}}}
      else
        {apiVersion: "apps/v1", kind: "Deployment", metadata: {name: $name, namespace: .metadata.namespace},
         spec: {replicas: 0, selector: {matchLabels: {"pharness.dev/campus-session": $name}},
           template: ($template | .metadata.labels["pharness.dev/campus-session"] = $name)}}
      end'
}

expect_admit() {
  local label="$1" file="$2" output
  if output="$(k create --dry-run=server -f "$file" -o name 2>&1)"; then
    pass "admitted: ${label}"
  else
    fail "expected admission: ${label}: ${output}"
  fi
}

expect_reject() {
  local label="$1" file="$2" needle="$3" output
  if output="$(k create --dry-run=server -f "$file" -o name 2>&1)"; then
    fail "expected rejection: ${label}"
  elif grep -qF "$needle" <<<"$output" && grep -qF "ValidatingAdmissionPolicy" <<<"$output"; then
    pass "rejected: ${label}"
    printf '      %s\n' "$output"
  else
    fail "rejected for an unexpected reason: ${label}: ${output}"
  fi
}

PIN_MESSAGE="Campus lab images must be fully qualified immutable references"
ALLOW_MESSAGE="Campus lab image digest is not approved"

k get namespace "$NAMESPACE" -o name >/dev/null
k get runtimeclass gvisor -o name >/dev/null
[[ "$(k get namespace "$NAMESPACE" -o jsonpath='{.metadata.labels.pod-security\.kubernetes\.io/enforce}')" == restricted ]] || {
  echo "${NAMESPACE} must enforce Pod Security restricted" >&2
  exit 1
}

echo "== Applying the committed policy and the test-only fixture policy"
"${GATE[@]}" check --root "$ROOT" >/dev/null
k apply -f "$REAL_MANIFEST"
"${GATE[@]}" render-policy --evidence "$FIXTURE" --name "$TEST_POLICY" --namespace-label "$TEST_LABEL" \
  >"${TEMP_ROOT}/test-policy.yaml"
k apply -f "${TEMP_ROOT}/test-policy.yaml"
for policy in pharness-campus-lab-image-allowlist "$TEST_POLICY"; do
  warnings="$(k get validatingadmissionpolicy "$policy" -o jsonpath='{.status.typeChecking.expressionWarnings}')"
  [[ -z "$warnings" ]] || fail "type-check warnings on ${policy}: ${warnings}"
done
k label namespace "$NAMESPACE" "${TEST_LABEL}=true" --overwrite >/dev/null
# Admission policy bindings are cached by the API server; wait until the
# test binding is observed before judging any request.
for _ in $(seq 1 30); do
  pod_manifest probe "$TAG_IMAGE" >"${TEMP_ROOT}/probe.json"
  if ! k create --dry-run=server -f "${TEMP_ROOT}/probe.json" -o name >/dev/null 2>&1; then break; fi
  sleep 2
done

echo "== Phase 1: admission matrix against the fixture allowlist"
pod_manifest "allowed-${RUN_ID}" "$PYTHON_IMAGE" >"${TEMP_ROOT}/allowed.json"
expect_admit "allowlisted digest Pod" "${TEMP_ROOT}/allowed.json"
pod_manifest "tag-${RUN_ID}" "$TAG_IMAGE" >"${TEMP_ROOT}/tag.json"
expect_reject "tag-only image" "${TEMP_ROOT}/tag.json" "$PIN_MESSAGE"
pod_manifest "latest-${RUN_ID}" "$LATEST_IMAGE" >"${TEMP_ROOT}/latest.json"
expect_reject "mutable :latest image" "${TEMP_ROOT}/latest.json" "$PIN_MESSAGE"
pod_manifest "tagdigest-${RUN_ID}" "$TAG_AND_DIGEST_IMAGE" >"${TEMP_ROOT}/tagdigest.json"
expect_reject "tag next to a digest" "${TEMP_ROOT}/tagdigest.json" "$PIN_MESSAGE"
pod_manifest "unknown-${RUN_ID}" "$UNKNOWN_IMAGE" >"${TEMP_ROOT}/unknown.json"
expect_reject "unknown digest" "${TEMP_ROOT}/unknown.json" "$ALLOW_MESSAGE"
pod_manifest "tampered-${RUN_ID}" "$TAMPERED_IMAGE" >"${TEMP_ROOT}/tampered.json"
expect_reject "tampered digest (last hex character changed)" "${TEMP_ROOT}/tampered.json" "$ALLOW_MESSAGE"
pod_manifest "init-${RUN_ID}" "$PYTHON_IMAGE" "$UNKNOWN_IMAGE" >"${TEMP_ROOT}/init.json"
expect_reject "initContainer bypass" "${TEMP_ROOT}/init.json" "$ALLOW_MESSAGE"
workload_manifest Job "job-${RUN_ID}" "$TAG_IMAGE" >"${TEMP_ROOT}/job-bad.json"
expect_reject "Job with a tag-only image" "${TEMP_ROOT}/job-bad.json" "$PIN_MESSAGE"
workload_manifest Job "job-ok-${RUN_ID}" "$PYTHON_IMAGE" >"${TEMP_ROOT}/job-good.json"
expect_admit "Job with an allowlisted digest" "${TEMP_ROOT}/job-good.json"
workload_manifest CronJob "cron-${RUN_ID}" "$UNKNOWN_IMAGE" >"${TEMP_ROOT}/cron-bad.json"
expect_reject "CronJob with an unknown digest" "${TEMP_ROOT}/cron-bad.json" "$ALLOW_MESSAGE"
workload_manifest Deployment "deploy-${RUN_ID}" "$TAMPERED_IMAGE" >"${TEMP_ROOT}/deploy-bad.json"
expect_reject "Deployment with a tampered digest" "${TEMP_ROOT}/deploy-bad.json" "$ALLOW_MESSAGE"

echo "== Phase 1: running Pods, ephemeral containers and updates"
python_a="cache-py-a-${RUN_ID}"
python_b="cache-py-b-${RUN_ID}"
node_a="cache-node-a-${RUN_ID}"
node_b="cache-node-b-${RUN_ID}"
for pod in "$python_a" "$python_b"; do
  pod_manifest "$pod" "$PYTHON_IMAGE" >"${TEMP_ROOT}/${pod}.json"
  "${GATE[@]}" check-lab-pod --pod "${TEMP_ROOT}/${pod}.json" --sources "$SOURCES" >/dev/null
  k create -f "${TEMP_ROOT}/${pod}.json" -o name
  CREATED_PODS+=("$pod")
done
for pod in "$node_a" "$node_b"; do
  pod_manifest "$pod" "$NODE_IMAGE" >"${TEMP_ROOT}/${pod}.json"
  "${GATE[@]}" check-lab-pod --pod "${TEMP_ROOT}/${pod}.json" --sources "$SOURCES" >/dev/null
  k create -f "${TEMP_ROOT}/${pod}.json" -o name
  CREATED_PODS+=("$pod")
done
pass "created four allowlisted gVisor session Pods (real admission, not dry-run)"
for pod in "${CREATED_PODS[@]}"; do
  k -n "$NAMESPACE" wait --for=condition=Ready "pod/${pod}" --timeout=180s >/dev/null
done

debug_patch() {
  jq -cn --arg image "$1" '{spec: {ephemeralContainers: [{name: "debug", image: $image, command: ["true"],
    securityContext: {allowPrivilegeEscalation: false, runAsNonRoot: true, capabilities: {drop: ["ALL"]},
      seccompProfile: {type: "RuntimeDefault"}}}]}}'
}
if output="$(k -n "$NAMESPACE" patch pod "$python_a" --subresource=ephemeralcontainers --type=strategic \
  -p "$(debug_patch "$UNKNOWN_IMAGE")" --dry-run=server 2>&1)"; then
  fail "expected rejection: ephemeral container with an unknown digest"
elif grep -qF "$ALLOW_MESSAGE" <<<"$output"; then
  pass "rejected: ephemeral container bypass"
  printf '      %s\n' "$output"
else
  fail "ephemeral container rejected for an unexpected reason: ${output}"
fi
if output="$(k -n "$NAMESPACE" patch pod "$python_a" --subresource=ephemeralcontainers --type=strategic \
  -p "$(debug_patch "$PYTHON_IMAGE")" --dry-run=server 2>&1)"; then
  pass "admitted: ephemeral container with an allowlisted digest"
else
  fail "expected admission: allowlisted ephemeral container: ${output}"
fi
if output="$(k -n "$NAMESPACE" set image "pod/${python_a}" "session=${TAMPERED_IMAGE}" --dry-run=server 2>&1)"; then
  fail "expected rejection: Pod image update to a tampered digest"
elif grep -qF "$ALLOW_MESSAGE" <<<"$output"; then
  pass "rejected: Pod image update to a tampered digest"
  printf '      %s\n' "$output"
else
  fail "image update rejected for an unexpected reason: ${output}"
fi
if output="$(k -n "$NAMESPACE" label pod "$python_a" pharness.dev/probe=update --dry-run=server 2>&1)"; then
  pass "admitted: metadata-only update of a running session Pod"
else
  fail "expected admission: metadata-only update: ${output}"
fi

echo "== Contaminated-cache fixture"
in_pod() { k -n "$NAMESPACE" exec "$1" -- sh -ec "$2"; }
in_pod "$python_a" '
  test "$PIP_INDEX_URL" = "https://pypi.org/simple"
  test "$(python3 -m pip cache dir)" = /var/cache/campus/pip
  mkdir -p "$PIP_CACHE_DIR/wheels/poison" "$PIP_CACHE_DIR/http-v2"
  printf "poisoned wheel from session A\n" >"$PIP_CACHE_DIR/wheels/poison/requests-99.0.0-py3-none-any.whl"
  printf "poisoned index response\n" >"$PIP_CACHE_DIR/http-v2/poison"
  find "$PIP_CACHE_DIR" -type f | sort'
pass "session A planted poisoned pip cache entries"
if in_pod "$python_b" '
  test "$(python3 -m pip cache dir)" = /var/cache/campus/pip
  test -z "$(find /var/cache/campus -type f -print -quit)"
  ! grep -rqs "session A" /var/cache/campus /workspace /tmp'; then
  pass "session B cannot see session A pip cache (its /var/cache/campus is empty)"
else
  fail "session B observed session A pip cache content"
fi
in_pod "$node_a" '
  test "$(npm config get registry)" = "https://registry.npmjs.org/"
  test "$(npm config get cache)" = /var/cache/campus/npm
  mkdir -p "$NPM_CONFIG_CACHE/_cacache/content-v2/sha512/00"
  printf "poisoned tarball from session A\n" >"$NPM_CONFIG_CACHE/_cacache/content-v2/sha512/00/poison"
  find "$NPM_CONFIG_CACHE" -type f | sort'
pass "session A planted a poisoned npm cache entry"
# npm writes its own debug logs under the cache, so only session B's logs may
# be present; nothing planted by session A may be.
if in_pod "$node_b" '
  test "$(npm config get cache)" = /var/cache/campus/npm
  test ! -e "$NPM_CONFIG_CACHE/_cacache/content-v2/sha512/00/poison"
  test -z "$(find /var/cache/campus -type f ! -path "$NPM_CONFIG_CACHE/_logs/*" -print -quit)"
  ! grep -rqs "session A" /var/cache/campus /workspace /tmp'; then
  pass "session B cannot see session A npm cache (only its own npm debug logs exist)"
else
  fail "session B observed session A npm cache content"
fi
for pod in "${CREATED_PODS[@]}"; do
  volumes="$(k -n "$NAMESPACE" get pod "$pod" -o json | jq -c '[.spec.volumes[] | keys - ["name"] | .[0]] | unique')"
  [[ "$volumes" == '["emptyDir"]' ]] || fail "${pod} uses non-emptyDir volumes: ${volumes}"
done
pass "all session Pods use only per-Pod emptyDir volumes"

echo "== Phase 2: committed allowlist from deploy/campus/approved-images.json"
k label namespace "$NAMESPACE" "${TEST_LABEL}-" "${REAL_LABEL}=true" --overwrite >/dev/null
for _ in $(seq 1 30); do
  if ! k create --dry-run=server -f "${TEMP_ROOT}/allowed.json" -o name >/dev/null 2>&1; then break; fi
  sleep 2
done
allowlist_size="$(jq '[.images[] | select(.status == "approved")] | length' "${ROOT}/deploy/campus/approved-images.json")"
if [[ "$allowlist_size" == 0 ]]; then
  expect_reject "python-runner digest while not approved" "${TEMP_ROOT}/allowed.json" "$ALLOW_MESSAGE"
else
  echo "committed allowlist has ${allowlist_size} digests"
fi
expect_reject "tag-only image under the committed policy" "${TEMP_ROOT}/tag.json" "$PIN_MESSAGE"
expect_reject "Job with a tag-only image under the committed policy" "${TEMP_ROOT}/job-bad.json" "$PIN_MESSAGE"

echo "== Scope: unlabelled namespaces are unaffected"
labelled="$(k get namespaces -l "${REAL_LABEL}=true" -o jsonpath='{.items[*].metadata.name}')"
[[ "$labelled" == "$NAMESPACE" ]] && pass "only ${NAMESPACE} carries ${REAL_LABEL}=true" || fail "unexpected labelled namespaces: ${labelled}"
jq --arg ns "$SCOPE_NAMESPACE" '.metadata.namespace = $ns' "${TEMP_ROOT}/tag.json" >"${TEMP_ROOT}/scope.json"
if output="$(k create --dry-run=server -f "${TEMP_ROOT}/scope.json" -o name 2>&1)"; then
  pass "admitted (dry-run): tag-only Pod in unlabelled namespace ${SCOPE_NAMESPACE}"
else
  fail "policy affected unlabelled namespace ${SCOPE_NAMESPACE}: ${output}"
fi

echo
if [[ "$FAILURES" -gt 0 ]]; then
  echo "${FAILURES} campus lab cluster checks failed" >&2
  exit 1
fi
echo "all campus lab cluster checks passed"
