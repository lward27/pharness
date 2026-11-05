#!/usr/bin/env bash
# In-cluster qualification of Campus session Jobs in admission-controlled lab
# slots (LUC-125).
#
# It creates temporary probe slots (campus-lab-probe-N) carrying the same
# guardrails as the Argo-managed slots, rendered from charts/campus-lab in the
# GitOps repository, plus a test-only image allowlist that admits only the
# research profile digest. The Argo-managed slots carry the real LUC-121
# allowlist, which is empty, so they refuse every session image; the probe
# checks that too. Everything the probe creates is removed on exit.
#
# Phases:
#   1. preconditions and versions (Argo, RuntimeClass, guard, node, CNI)
#   2. controller RBAC
#   3. provider provision (crates/pharness-core/tests/campus_session_jobs_live.rs)
#   4. stress: fork, processes, memory, disk, logs, updates, concurrency
#   5. provider delete
#   6. independent admission matrix
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTEXT="lucas_engineering"
CHART="${CAMPUS_LAB_CHART:-}"
SLOTS=5
REAL_SLOT="campus-lab-slot-1"
NODE="ubuntu-lucas-engineering-2"
CONTROLLER_SA="system:serviceaccount:pharness:pharness-campus-controller"
PROBE_PREFIX="campus-lab-probe-"
PROBE_LABEL="pharness.dev/campus-lab-probe"
GUARD="pharness-campus-lab-session-guard"
TEST_POLICY="pharness-campus-session-probe-allowlist"
RESEARCH_IMAGE="$(jq -r .image "${ROOT}/crates/pharness-core/src/campus_profile/fixtures/research.json")"
ALT_IMAGE="docker.io/library/busybox@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662"
RUN="$(date -u +%H%M%S)"
OUT=""

usage() {
  cat <<USAGE
Usage: $0 --chart <lucas_engineering>/charts/campus-lab [--context <ctx>] [--slots N] [--out <dir>]

Needs cluster-admin on the context, helm, jq and cargo. The controller
identity is impersonated as ${CONTROLLER_SA}.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --context) CONTEXT="$2"; shift 2 ;;
    --chart) CHART="$2"; shift 2 ;;
    --slots) SLOTS="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ -n "$CHART" && -f "${CHART}/Chart.yaml" ]] || { usage >&2; exit 2; }
[[ "$SLOTS" =~ ^[1-5]$ ]] || { echo "--slots must be 1-5" >&2; exit 2; }
OUT="${OUT:-$(mktemp -d /tmp/campus-session-jobs.XXXXXX)}"
mkdir -p "$OUT"

PASSED=0
FAILED=0
pass() { PASSED=$((PASSED + 1)); echo "PASS $*"; }
fail() { FAILED=$((FAILED + 1)); echo "FAIL $*"; }
k() { kubectl --context "$CONTEXT" "$@"; }
kc() { kubectl --context "$CONTEXT" --as "$CONTROLLER_SA" "$@"; }

PROBES=()
for index in $(seq 1 "$SLOTS"); do PROBES+=("${PROBE_PREFIX}${index}"); done

cleanup() {
  local status=$?
  set +e
  if [[ "$FAILED" != 0 || "$status" != 0 ]]; then
    for ns in "${PROBES[@]}"; do
      k -n "$ns" get jobs.batch,pods,configmaps -o json >"${OUT}/diagnostics-${ns}.json" 2>&1
      k -n "$ns" get events --sort-by=.lastTimestamp >"${OUT}/events-${ns}.txt" 2>&1
    done
  fi
  echo "== Cleanup"
  for ns in "${PROBES[@]}"; do k delete namespace "$ns" --ignore-not-found --wait=false >/dev/null 2>&1; done
  k delete validatingadmissionpolicybinding "$TEST_POLICY" "${GUARD}-probe" --ignore-not-found >/dev/null 2>&1
  k delete validatingadmissionpolicy "$TEST_POLICY" --ignore-not-found >/dev/null 2>&1
  k -n "$REAL_SLOT" delete configmaps,jobs.batch -l pharness.dev/campus-tenant=tenant-probe --ignore-not-found >/dev/null 2>&1
  for _ in $(seq 1 60); do
    remaining="$(k get namespaces -l "${PROBE_LABEL}=true" -o name 2>/dev/null)"
    [[ -z "$remaining" ]] && break
    sleep 3
  done
  echo "remaining probe namespaces: ${remaining:-none}"
  echo "remaining probe policies: $(k get validatingadmissionpolicies,validatingadmissionpolicybindings -o name 2>/dev/null | grep -E "${TEST_POLICY}|${GUARD}-probe" || echo none)"
  echo "evidence: ${OUT}"
}
trap cleanup EXIT

node_sample() {
  k get --raw "/api/v1/nodes/${NODE}/proxy/stats/summary" |
    jq -c '{curproc: .node.rlimit.curproc, maxpid: .node.rlimit.maxpid,
            memAvailMiB: (.node.memory.availableBytes / 1048576 | floor),
            fsAvailMiB: (.node.fs.availableBytes / 1048576 | floor),
            swapUsedMiB: ((.node.swap.swapUsageBytes // 0) / 1048576 | floor)}'
}

node_pressure() {
  k get node "$NODE" -o json |
    jq -r '[.status.conditions[] | select((.type == "Ready" and .status != "True") or (.type != "Ready" and .status != "False")) | .type] | join(",")'
}

node_restarts() {
  k get pods -A --field-selector "spec.nodeName=${NODE}" -o json |
    jq --arg prefix "$PROBE_PREFIX" '[.items[] | select(.metadata.namespace | startswith($prefix) | not)
      | [.status.containerStatuses[]?.restartCount] | add // 0] | add // 0'
}

echo "== Phase 1: preconditions and versions (run ${RUN}, evidence ${OUT})"
app="$(k -n argocd get applications.argoproj.io campus-lab -o jsonpath='{.status.sync.status}/{.status.health.status}/{.status.sync.revision}')"
[[ "$app" == Synced/Healthy/* ]] && pass "Argo campus-lab ${app}" || fail "Argo campus-lab ${app}"
rc="$(k get runtimeclass gvisor -o json | jq -c '{handler, overhead: .overhead.podFixed, pool: .scheduling.nodeSelector}')"
[[ "$rc" == '{"handler":"runsc","overhead":{"cpu":"50m","memory":"128Mi"},"pool":{"pharness.dev/campus-lab-runtime":"gvisor"}}' ]] &&
  pass "RuntimeClass gvisor ${rc}" || fail "RuntimeClass gvisor ${rc}"
warnings="$(k get validatingadmissionpolicy "$GUARD" -o jsonpath='{.status.typeChecking.expressionWarnings}')"
[[ -z "$warnings" ]] && pass "${GUARD} type-checks without warnings" || fail "${GUARD} warnings: ${warnings}"
k get node "$NODE" -o json | jq -c '{kubelet: .status.nodeInfo.kubeletVersion, kernel: .status.nodeInfo.kernelVersion,
  os: .status.nodeInfo.osImage, containerRuntime: .status.nodeInfo.containerRuntimeVersion,
  cni: .metadata.annotations["flannel.alpha.coreos.com/backend-type"],
  pool: .metadata.labels["pharness.dev/campus-lab-runtime"],
  allocatable: .status.allocatable | {cpu, memory, pods}}' | tee "${OUT}/node.json"
echo "api server: $(k version -o json | jq -r .serverVersion.gitVersion)"
labelled="$(k get namespaces -l pharness.dev/campus-lab=true -o jsonpath='{.items[*].metadata.name}')"
echo "namespaces with pharness.dev/campus-lab=true: ${labelled}"

echo "== Creating ${SLOTS} probe slots and the test-only allowlist"
for ns in "${PROBES[@]}"; do
  k create namespace "$ns" >/dev/null
  k label namespace "$ns" "${PROBE_LABEL}=true" "pharness.dev/campus-slot=${ns}" \
    pharness.dev/purpose=luc-125-probe pod-security.kubernetes.io/enforce=restricted \
    pod-security.kubernetes.io/enforce-version=latest pod-security.kubernetes.io/audit=restricted \
    pod-security.kubernetes.io/warn=restricted >/dev/null
  helm template campus-lab "$CHART" --set "probe.namespace=${ns}" | k apply -f - >/dev/null
done
cat >"${OUT}/test-policy.yaml" <<POLICY
apiVersion: admissionregistration.k8s.io/v1
kind: ValidatingAdmissionPolicy
metadata:
  name: ${TEST_POLICY}
spec:
  failurePolicy: Fail
  matchConstraints:
    resourceRules:
      - apiGroups: [""]
        apiVersions: ["v1"]
        operations: ["CREATE", "UPDATE"]
        resources: ["pods", "pods/ephemeralcontainers"]
      - apiGroups: ["batch"]
        apiVersions: ["v1"]
        operations: ["CREATE", "UPDATE"]
        resources: ["jobs"]
  variables:
    - name: podSpec
      expression: "object.kind == 'Pod' ? object.spec : object.spec.template.spec"
    - name: images
      expression: >-
        variables.podSpec.containers.map(c, c.image) +
        (has(variables.podSpec.initContainers) ? variables.podSpec.initContainers.map(c, c.image) : []) +
        (has(variables.podSpec.ephemeralContainers) ? variables.podSpec.ephemeralContainers.map(c, c.image) : [])
  validations:
    - expression: "variables.images.all(i, i in ['${RESEARCH_IMAGE}'])"
      message: "probe allowlist: image is not the approved research profile digest"
---
apiVersion: admissionregistration.k8s.io/v1
kind: ValidatingAdmissionPolicyBinding
metadata:
  name: ${TEST_POLICY}
spec:
  policyName: ${TEST_POLICY}
  validationActions: [Deny]
  matchResources:
    namespaceSelector:
      matchLabels:
        ${PROBE_LABEL}: "true"
POLICY
k apply -f "${OUT}/test-policy.yaml" >/dev/null
# A Pod that passes Pod Security and the LimitRange, so only the two
# policies can refuse it: one variant per policy.
probe_pod() {
  jq -n --arg ns "$1" --arg image "$2" '{apiVersion: "v1", kind: "Pod",
    metadata: {name: "campus-binding-check", namespace: $ns, labels: {
      "pharness.dev/campus-controller": "pharness-campus-controller", "pharness.dev/campus-tenant": "tenant-probe",
      "pharness.dev/campus-session": "binding-check", "pharness.dev/campus-slot": $ns,
      "pharness.dev/campus-generation": "1", "pharness.dev/campus-record": "workload"}},
    spec: {runtimeClassName: "gvisor", nodeSelector: {"pharness.dev/campus-lab-runtime": "gvisor"},
      restartPolicy: "Never", activeDeadlineSeconds: 60, automountServiceAccountToken: false,
      securityContext: {runAsNonRoot: true, runAsUser: 65532, seccompProfile: {type: "RuntimeDefault"}},
      containers: [{name: "c", image: $image,
        securityContext: {allowPrivilegeEscalation: false, capabilities: {drop: ["ALL"]}},
        resources: {limits: {cpu: "100m", memory: "64Mi", "ephemeral-storage": "16Mi"}}}]}}'
}
probe_pod "${PROBES[0]}" "$ALT_IMAGE" >"${OUT}/allowlist-check.json"
probe_pod "${PROBES[0]}" "$RESEARCH_IMAGE" | jq 'del(.metadata.labels["pharness.dev/campus-tenant"])' >"${OUT}/binding-check.json"
bound=false
for _ in $(seq 1 40); do
  allowlist="$(k create --dry-run=server -f "${OUT}/allowlist-check.json" 2>&1 || true)"
  guard="$(k create --dry-run=server -f "${OUT}/binding-check.json" 2>&1 || true)"
  if grep -qF "$TEST_POLICY" <<<"$allowlist" && grep -qF "$GUARD" <<<"$guard"; then bound=true; break; fi
  sleep 3
done
[[ "$bound" == true ]] && pass "probe bindings active" || { fail "probe bindings never took effect: ${allowlist} | ${guard}"; exit 1; }

echo "== Phase 2: controller RBAC (${CONTROLLER_SA})"
can() {
  local expected="$1"; shift
  local got
  got="$(kc auth can-i "$@" 2>/dev/null || true)"
  [[ "$got" == "$expected" ]] && pass "can-i ${*} = ${got}" || fail "can-i ${*} = ${got}, expected ${expected}"
}
P="${PROBES[0]}"
can yes create jobs.batch -n "$REAL_SLOT"
can yes create jobs.batch -n "$P"
can yes delete pods -n "$P"
can yes create configmaps -n "$P"
can yes get runtimeclasses.node.k8s.io/gvisor
can no create pods -n "$REAL_SLOT"
can no create pods/exec -n "$REAL_SLOT"
can no create pods/ephemeralcontainers -n "$REAL_SLOT"
can no update pods -n "$REAL_SLOT"
can no patch jobs.batch -n "$REAL_SLOT"
can no update configmaps -n "$REAL_SLOT"
can no get secrets -n "$REAL_SLOT"
can no create serviceaccounts/token -n "$REAL_SLOT"
can no create jobs.batch -n default
can no create jobs.batch -n pharness
can no patch namespaces/"$REAL_SLOT"
can no patch resourcequotas -n "$REAL_SLOT"
can no delete networkpolicies -n "$REAL_SLOT"
can no create rolebindings -n "$REAL_SLOT"
can no update validatingadmissionpolicies/"$GUARD"
can no delete validatingadmissionpolicybindings/"$GUARD"
can no get runtimeclasses.node.k8s.io/runc
can no list runtimeclasses.node.k8s.io
can no create runtimeclasses.node.k8s.io
can no patch nodes

echo "== Phase 3: provider provision through the controller identity"
baseline_restarts="$(node_restarts)"
echo "node before sessions: $(node_sample) restarts=${baseline_restarts}"
live() {
  PHARNESS_CAMPUS_LIVE_PHASE="$1" PHARNESS_CAMPUS_LIVE_RUN="$RUN" \
    PHARNESS_CAMPUS_LIVE_SLOTS="$(IFS=,; echo "${PROBES[*]}")" PHARNESS_CAMPUS_LIVE_REAL_SLOT="$REAL_SLOT" \
    PHARNESS_CAMPUS_LIVE_CONTROLLER_ARGS="--context ${CONTEXT} --as ${CONTROLLER_SA}" \
    PHARNESS_CAMPUS_LIVE_ADMIN_ARGS="--context ${CONTEXT}" PHARNESS_CAMPUS_LIVE_OUT="$OUT" \
    cargo test --quiet --manifest-path "${ROOT}/Cargo.toml" -p pharness-core --test campus_session_jobs_live \
    -- --ignored --nocapture >"${OUT}/live-$1.log" 2>&1
}
if live provision; then pass "provider provision phase"; else fail "provider provision phase"; fi
grep '^@@CAMPUS' "${OUT}/live-provision.log" || tail -40 "${OUT}/live-provision.log"
[[ -f "${OUT}/pod-${PROBES[0]}.json" ]] || exit 1

echo "== Phase 4: placement, limits and stress"
pods=()
for ns in "${PROBES[@]}"; do
  pod="$(jq -r .metadata.name "${OUT}/pod-${ns}.json")"
  pods+=("$pod")
  k -n "$ns" get pod "$pod" -o json | jq -c '{ns: .metadata.namespace, node: .spec.nodeName,
    runtimeClass: .spec.runtimeClassName, overhead: .spec.overhead, imageID: .status.containerStatuses[0].imageID,
    limits: .spec.containers[0].resources.limits, token: (.spec.volumes | map(select(.projected)) | length),
    ownership: (.metadata.labels | with_entries(select(.key | startswith("pharness.dev/campus-"))))}' |
    tee -a "${OUT}/placement.jsonl"
done
placed="$(jq -s --arg node "$NODE" '[.[] | select(.node == $node and .runtimeClass == "gvisor" and .token == 0)] | length' "${OUT}/placement.jsonl")"
[[ "$placed" == "$SLOTS" ]] && pass "all ${SLOTS} sessions run concurrently with gVisor on ${NODE} without a token volume" ||
  fail "placement: ${placed}/${SLOTS}"
sx() { local index="$1"; shift; k -n "${PROBES[$index]}" exec "${pods[$index]}" -c workspace -- "$@"; }
sandbox="$(sx 0 /bin/sh -c 'uname -a; dmesg 2>/dev/null | head -2; cat /proc/1/limits | grep -E "processes|core"')"
echo "$sandbox" | tee "${OUT}/sandbox.txt"
grep -qE 'Max processes +128 +128' <<<"$sandbox" && pass "PID 1 runs with RLIMIT_NPROC 128/128" || fail "PID 1 limits: ${sandbox}"
logs="$(k -n "${PROBES[0]}" logs "${pods[0]}" -c workspace | wc -c)"
[[ "$logs" == 0 ]] && pass "workspace output does not reach the container log" || fail "container log has ${logs} bytes"

FORK='
import os, resource, time
kids = []
try:
    for _ in range(1000):
        pid = os.fork()
        if pid == 0:
            time.sleep(120)
            os._exit(0)
        kids.append(pid)
    print("forked", len(kids))
except OSError as error:
    print("refused_after", len(kids), error.strerror)
try:
    resource.setrlimit(resource.RLIMIT_NPROC, (100000, 100000))
    print("raised")
except (ValueError, OSError) as error:
    print("raise_refused", error)
time.sleep(int(os.environ.get("HOLD", "0")))
for pid in kids:
    os.kill(pid, 9)
for pid in kids:
    os.waitpid(pid, 0)
'
# The exact session command wrapper from the Job: what PID 1 runs and what any
# other entry must use. It discards output, so the probe writes to a file.
WRAPPER="$(jq -r '.spec.template.spec.containers[0].command[2]' "${OUT}/job-${PROBES[0]}.json")"
sx 0 /bin/sh -c "$WRAPPER" campus-workspace python3 -c \
  "import sys; sys.stdout = open('/workspace/fork.out', 'w'); exec(sys.argv[1])" "$FORK" || true
fork="$(sx 0 cat /workspace/fork.out)"
echo "wrapped fork: ${fork}"
if grep -q '^refused_after' <<<"$fork" && grep -q '^raise_refused' <<<"$fork" &&
   (( $(awk '/^refused_after/ {print $2}' <<<"$fork") < 128 )); then
  pass "fork stops below the 128-process limit and the limit cannot be raised"
else
  fail "wrapped fork: ${fork}"
fi

# gVisor runs sandbox processes as sentry goroutines, but blocked tasks still
# cost host threads. Measure that cost for a held fork bomb through the
# wrapper, and for 600 processes through kubectl exec, which skips the wrapper
# (the controller has no pods/exec; operators do). The bound that matters is
# the node: its process count must stay far below pid_max and it must not
# report PIDPressure.
pod_state() {
  k -n "${PROBES[$1]}" get pod "${pods[$1]}" -o json | jq -c '{phase: .status.phase, reason: .status.reason,
    state: .status.containerStatuses[0].state, restarts: .status.containerStatuses[0].restartCount}' ||
    echo '{"pod":"not found"}'
}
host_threads() {
  local label="$1" index="$2"; shift 2
  local before during
  before="$(node_sample)"
  sx "$index" "$@" >"${OUT}/${label}.txt" 2>&1 &
  local holder=$!
  sleep 12
  during="$(node_sample)"
  wait "$holder" || true
  sleep 3
  local delta=$(( $(jq .curproc <<<"$during") - $(jq .curproc <<<"$before") ))
  echo "${label}: $(tr '\n' ' ' <"${OUT}/${label}.txt") | pod $(pod_state "$index") | host before=${before} during=${during} delta=${delta}"
  if (( $(jq .curproc <<<"$during") * 10 < $(jq .maxpid <<<"$during") )) && [[ -z "$(node_pressure)" ]]; then
    pass "${label}: host processes +${delta}, node stays below 10% of pid_max without PIDPressure"
  else
    fail "${label}: host ${during}, pressure $(node_pressure)"
  fi
}
host_threads wrapped-fork-held 1 /bin/sh -c "$WRAPPER" campus-workspace env HOLD=20 python3 -c \
  "import sys; sys.stdout = open('/workspace/fork-held.out', 'w'); exec(sys.argv[1])" "$FORK"
echo "wrapped held fork: $(sx 1 cat /workspace/fork-held.out | tr '\n' ' ')"
# Without the wrapper nothing caps the process count inside the sandbox, so
# this can exhaust the session's own memory; it must not reach the node.
host_threads unwrapped-fork-600 1 env HOLD=20 python3 -c "${FORK/range(1000)/range(600)}"

MEMORY='
import time
chunks = []
try:
    for index in range(64):
        chunk = bytearray(64 * 1024 * 1024)
        for offset in range(0, len(chunk), 4096):
            chunk[offset] = 1
        chunks.append(chunk)
        print("allocated_mib", (index + 1) * 64, flush=True)
except MemoryError:
    print("memory_error", len(chunks) * 64, flush=True)
'
before="$(node_sample)"
memory="$(sx 2 python3 -c "$MEMORY" 2>&1 | tail -3 || true)"
sleep 5
after="$(node_sample)"
state="$(pod_state 2)"
echo "memory: ${memory} | pod ${state} | host before=${before} after=${after}"
# gVisor charges the whole sandbox to the Pod cgroup, whose memory.max is the
# container limit plus the RuntimeClass overhead (2048 + 128 MiB). Swap is
# closed by the kubepods.slice drop-in from ops/campus-lab-gvisor.
last="$(grep -oE '[0-9]+' <<<"$(grep allocated_mib <<<"$memory" | tail -1)" | tail -1)"
if [[ -n "$last" ]] && (( last <= 2176 )); then
  pass "memory stops at ${last} MiB, within the 2176 MiB Pod cgroup (limit + overhead)"
else
  fail "memory: ${memory}"
fi
swap_delta=$(( $(jq .swapUsedMiB <<<"$after") - $(jq .swapUsedMiB <<<"$before") ))
(( swap_delta < 64 )) && pass "node swap changed by ${swap_delta} MiB during the memory probe" ||
  fail "node swap grew by ${swap_delta} MiB"
[[ -z "$(node_pressure)" ]] && pass "node has no pressure condition after the memory probe" || fail "node pressure: $(node_pressure)"

before="$(node_sample)"
disk="$(sx 3 /bin/sh -c '
  dd if=/dev/zero of=/workspace/fill bs=1M count=2048 2>&1 | grep -E "No space|copied"
  dd if=/dev/zero of=/tmp/fill bs=1M count=1024 2>&1 | grep -E "No space|copied"
  for target in /fill /etc/fill /usr/fill; do (: >"$target") 2>/dev/null && echo "writable $target" || echo "refused $target"; done
  while read -r _ mnt _ opts _; do [ "$mnt" = / ] && echo "rootfs $opts"; done </proc/mounts
  df -m /workspace /tmp | tail -2
  rm -f /workspace/fill /tmp/fill' 2>&1 || true)"
after="$(node_sample)"
echo "$disk" | tee "${OUT}/disk.txt"
echo "disk host before=${before} after=${after}"
grep -c 'No space left' <<<"$disk" | grep -q '^2$' && pass "/workspace and /tmp stop at their sizeLimit" || fail "disk: ${disk}"
if grep -c '^refused /' <<<"$disk" | grep -q '^3$' && grep -qE '^rootfs (ro|ro,.*)$' <<<"$disk"; then
  pass "root filesystem is mounted read-only and refuses writes"
else
  fail "root filesystem: ${disk}"
fi
fs_delta=$(( $(jq .fsAvailMiB <<<"$before") - $(jq .fsAvailMiB <<<"$after") ))
(( fs_delta < 64 )) && pass "node filesystem changed by ${fs_delta} MiB during 1 GiB of writes" ||
  fail "node filesystem changed by ${fs_delta} MiB"

output="$(k -n "${PROBES[4]}" debug "${pods[4]}" --profile=restricted --image="$RESEARCH_IMAGE" --target=workspace -- true 2>&1 || true)"
grep -qF "do not admit ephemeral containers" <<<"$output" && pass "ephemeral debug container rejected" ||
  fail "ephemeral container: ${output}"
output="$(k -n "${PROBES[4]}" label pod "${pods[4]}" "pharness.dev/campus-slot=${PROBES[0]}" --overwrite --dry-run=server 2>&1 || true)"
grep -qF "ownership labels" <<<"$output" && pass "relabelling a session Pod into another slot rejected" ||
  fail "relabel: ${output}"

restarts="$(node_restarts)"
[[ "$restarts" == "$baseline_restarts" ]] && pass "no container on ${NODE} restarted during the stress (${restarts} total)" ||
  fail "restarts on ${NODE}: ${baseline_restarts} -> ${restarts}"
[[ -z "$(node_pressure)" ]] && pass "${NODE} Ready without pressure: $(node_sample)" || fail "node pressure: $(node_pressure)"

echo "== Phase 5: provider delete"
if live delete; then pass "provider delete phase"; else fail "provider delete phase"; fi
grep '^@@CAMPUS' "${OUT}/live-delete.log" || tail -40 "${OUT}/live-delete.log"
for ns in "${PROBES[@]}"; do
  left="$(k -n "$ns" get jobs.batch,pods -o name | wc -l)"
  [[ "$left" == 0 ]] || fail "${ns} still has ${left} Jobs or Pods"
done

echo "== Phase 6: independent admission matrix"
BASE="${OUT}/job-${PROBES[0]}.json"
jq 'del(.status, .metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp, .metadata.managedFields,
      .metadata.generation, .metadata.ownerReferences, .spec.selector)
    | .metadata.labels |= with_entries(select(.key | test("controller-uid|job-name") | not))
    | .spec.template.metadata.labels |= with_entries(select(.key | test("controller-uid|job-name") | not))
    | .metadata.name = "campus-matrix"' "$BASE" >"${OUT}/base.json"
C='.spec.template.spec.containers[0]'
S='.spec.template.spec'
mutate() { jq "$1" "${OUT}/base.json" >"${OUT}/case.json"; }
expect_admit() {
  local label="$1" output
  if output="$("${@:2}" create --dry-run=server -f "${OUT}/case.json" -o name 2>&1)"; then
    pass "admitted: ${label}"
  else
    fail "expected admission: ${label}: ${output}"
  fi
}
expect_reject() {
  local label="$1" needle="$2" output
  if output="$(k create --dry-run=server -f "${OUT}/case.json" -o name 2>&1)"; then
    fail "expected rejection: ${label}"
  elif grep -qF "$needle" <<<"$output" && grep -qF "ValidatingAdmissionPolicy" <<<"$output"; then
    pass "rejected: ${label}"
  else
    fail "rejected for another reason: ${label}: ${output}"
  fi
}
mutate '.'; expect_admit "session Job as the controller" kc
mutate '.'; expect_admit "session Job as an operator" k
# The API server itself refuses privileged with allowPrivilegeEscalation=false.
mutate "${C}.securityContext.privileged = true | ${C}.securityContext.allowPrivilegeEscalation = true"; expect_reject "privileged" "must not be privileged"
mutate "${C}.securityContext.allowPrivilegeEscalation = true"; expect_reject "privilege escalation" "must not be privileged"
mutate "${C}.securityContext.capabilities.add = [\"NET_ADMIN\"]"; expect_reject "added capability" "must not be privileged"
mutate "${S}.volumes += [{name: \"host\", hostPath: {path: \"/\"}}] | ${C}.volumeMounts += [{name: \"host\", mountPath: \"/host\"}]"
expect_reject "hostPath volume" "only emptyDir"
mutate "${S}.volumes += [{name: \"token\", projected: {sources: [{serviceAccountToken: {path: \"token\"}}]}}]"
expect_reject "projected token volume" "only emptyDir"
mutate "${S}.volumes += [{name: \"secret\", secret: {secretName: \"x\"}}]"; expect_reject "secret volume" "only emptyDir"
mutate "${S}.automountServiceAccountToken = true"; expect_reject "token automount" "automountServiceAccountToken"
mutate "del(${S}.automountServiceAccountToken)"; expect_reject "token automount unset" "automountServiceAccountToken"
mutate "${S}.hostNetwork = true"; expect_reject "hostNetwork" "host network"
mutate "${S}.hostPID = true"; expect_reject "hostPID" "host network"
mutate "${S}.hostIPC = true"; expect_reject "hostIPC" "host network"
mutate "del(${S}.runtimeClassName)"; expect_reject "no runtimeClassName" "runtimeClassName gvisor"
mutate "${S}.runtimeClassName = \"runc\""; expect_reject "alternate runtime" "runtimeClassName gvisor"
mutate "${S}.nodeSelector = {\"kubernetes.io/hostname\": \"ubuntu-lucas-engineering\"}"; expect_reject "other node pool" "pool"
mutate "${S}.nodeName = \"ubuntu-lucas-engineering\""; expect_reject "nodeName" "must not set nodeName"
mutate "${S}.tolerations = [{key: \"node-role.kubernetes.io/control-plane\", operator: \"Exists\", effect: \"NoSchedule\"}]"
expect_reject "control-plane toleration" "must not tolerate"
mutate "${C}.image = \"registry.lucas.engineering/pharness-python-runner:latest\""; expect_reject "tag-only image" "digest-pinned"
mutate "${C}.image = \"${ALT_IMAGE}\""; expect_reject "alternate digest image" "probe allowlist"
mutate "${C}.envFrom = [{secretRef: {name: \"x\"}}]"; expect_reject "envFrom secret" "must not read environment"
mutate "${C}.env += [{name: \"X\", valueFrom: {secretKeyRef: {name: \"x\", key: \"y\"}}}]"; expect_reject "env from secret" "must not read environment"
mutate "${C}.ports = [{containerPort: 8080, hostPort: 8080}]"; expect_reject "hostPort" "host ports"
mutate "del(${C}.resources.limits[\"ephemeral-storage\"]) | del(${C}.resources.requests[\"ephemeral-storage\"])"
expect_reject "missing ephemeral-storage limit" "limits"
mutate ".spec.activeDeadlineSeconds = 20000 | ${S}.activeDeadlineSeconds = 20000"; expect_reject "deadline above 14400" "activeDeadlineSeconds"
mutate "del(${S}.activeDeadlineSeconds)"; expect_reject "no Pod deadline" "activeDeadlineSeconds"
mutate "del(.metadata.labels[\"pharness.dev/campus-tenant\"])"; expect_reject "no tenant label" "ownership labels"
mutate "del(.spec.template.metadata.labels[\"pharness.dev/campus-generation\"])"; expect_reject "no generation on Pod template" "ownership labels"
mutate ".metadata.labels[\"pharness.dev/campus-slot\"] = \"${PROBES[1]:-other}\" | .spec.template.metadata.labels[\"pharness.dev/campus-slot\"] = \"${PROBES[1]:-other}\""
expect_reject "slot label of another namespace" "ownership labels"
mutate ".metadata.labels[\"pharness.dev/campus-controller\"] = \"other\" | .spec.template.metadata.labels[\"pharness.dev/campus-controller\"] = \"other\""
expect_reject "other controller" "ownership labels"
mutate ".spec.template.metadata.labels[\"pharness.dev/campus-session\"] = \"other-session\""; expect_reject "Pod template of another session" "same ownership labels"
mutate "${S}.restartPolicy = \"OnFailure\""; expect_reject "restartPolicy OnFailure" "restartPolicy Never"
mutate '{apiVersion: "apps/v1", kind: "Deployment", metadata: .metadata, spec: {replicas: 0,
  selector: {matchLabels: {"pharness.dev/campus-session": .metadata.labels["pharness.dev/campus-session"]}},
  template: (.spec.template | .spec.restartPolicy = "Always" | del(.spec.activeDeadlineSeconds))}}'
expect_reject "Deployment" "admit only session Jobs"
mutate '{apiVersion: "batch/v1", kind: "CronJob", metadata: .metadata, spec: {schedule: "* * * * *", jobTemplate: {spec: .spec}}}'
expect_reject "CronJob" "admit only session Jobs"
mutate '{apiVersion: "v1", kind: "Pod", metadata: (.spec.template.metadata + {name: "campus-matrix", namespace: .metadata.namespace}), spec: .spec.template.spec}'
output="$(kc create --dry-run=server -f "${OUT}/case.json" -o name 2>&1 || true)"
grep -qF "forbidden" <<<"$output" && pass "controller cannot create bare Pods (RBAC)" || fail "bare Pod as controller: ${output}"
jq --arg slot "${REAL_SLOT}" '.metadata.namespace = $slot | .metadata.labels["pharness.dev/campus-slot"] = $slot
  | .spec.template.metadata.labels["pharness.dev/campus-slot"] = $slot' "${OUT}/base.json" >"${OUT}/case.json"
output="$(kc create --dry-run=server -f "${OUT}/case.json" -o name 2>&1 || true)"
grep -qF "pharness-campus-lab-image-allowlist" <<<"$output" &&
  pass "Argo-managed slot ${REAL_SLOT} refuses the session image (empty LUC-121 allowlist)" ||
  fail "real slot: ${output}"
jq --arg ns "$REAL_SLOT" '.metadata.namespace = $ns' "${OUT}/binding-check.json" >"${OUT}/case.json"
output="$(k create --dry-run=server -f "${OUT}/case.json" -o name 2>&1 || true)"
grep -qF "ValidatingAdmissionPolicy" <<<"$output" && pass "Argo-managed slot rejects a Pod without a tenant label" || fail "real slot Pod: ${output}"

echo "== Result: ${PASSED} passed, ${FAILED} failed"
[[ "$FAILED" == 0 ]]
