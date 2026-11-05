#!/usr/bin/env bash
# In-cluster qualification of Campus session network isolation and approved
# egress (LUC-130), plus the live broker-token checks of LUC-131.
#
# Sessions are real: the Kubernetes provider launches them through the
# controller identity in Argo-managed lab slots, under gVisor, with the
# approved runner images. Probes run inside them with kubectl exec (operator
# identity; the controller has no pods/exec). Everything the test creates is
# removed on exit.
#
# Phases:
#   1. preconditions (Argo, proxy, certificate, slot policies, guards)
#   2. RBAC: the controller cannot change isolation; the proxy only lists Pods
#   3. Secret admission matrix
#   4. provider: missing enforcement refused, egress sessions launched
#   5. session admission matrix (egress label, hosts, gate, token volume)
#   6. functional egress: pip, npm, git clone, codeload, raw files
#   7. isolation and proxy matrix from inside the sessions
#   8. proxy decision log
#   9. startup race and the network gate
#  10. missing or weakened slot policy refuses a launch
#  11. broker token scrape, rotation and revocation (pharness-api live test)
#  12. provider delete
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTEXT="lucas_engineering"
CHART="${CAMPUS_LAB_CHART:-}"
EGRESS_SLOTS=(campus-lab-slot-1 campus-lab-slot-2 campus-lab-slot-3)
MATRIX_SLOT="campus-lab-slot-4"
BROKER_SLOT="campus-lab-slot-5"
# The broker phase runs the API's journal in process; its one lab slot is
# tenant-a-lab-slot-01, so it gets a probe namespace carrying that slot label.
BROKER_NS="campus-egress-broker-1"
BROKER_SLOT_ID="tenant-a-lab-slot-01"
CONTROLLER_SA="system:serviceaccount:pharness:pharness-campus-controller"
PROXY_SA="system:serviceaccount:campus-egress:campus-egress-proxy"
PROXY_NS="campus-egress"
PROBE_SLOT="campus-egress-probe-1"
RACE_NS="campus-egress-race"
OPEN_NS="campus-egress-open"
PROBE_LABEL="pharness.dev/campus-lab-probe"
GUARD="pharness-campus-lab-session-guard"
SECRET_GUARD="pharness-campus-lab-secret-guard"
PYTHON_IMAGE="$(jq -r .image "${ROOT}/crates/pharness-core/src/campus_profile/fixtures/research.json")"
RUN="$(date -u +%H%M%S)"
OUT=""
SKIP_BROKER=false

usage() {
  cat <<USAGE
Usage: $0 --chart <lucas_engineering>/charts/campus-lab [--context <ctx>] [--out <dir>] [--skip-broker]

Needs cluster-admin on the context, helm, jq, openssl and cargo. The
controller identity is impersonated as ${CONTROLLER_SA}.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --context) CONTEXT="$2"; shift 2 ;;
    --chart) CHART="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --skip-broker) SKIP_BROKER=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ -n "$CHART" && -f "${CHART}/Chart.yaml" ]] || { usage >&2; exit 2; }
OUT="${OUT:-$(mktemp -d /tmp/campus-egress.XXXXXX)}"
mkdir -p "$OUT"

PASSED=0
FAILED=0
pass() { PASSED=$((PASSED + 1)); echo "PASS $*"; }
fail() { FAILED=$((FAILED + 1)); echo "FAIL $*"; }
k() { kubectl --context "$CONTEXT" "$@"; }
kc() { kubectl --context "$CONTEXT" --as "$CONTROLLER_SA" "$@"; }
kp() { kubectl --context "$CONTEXT" --as "$PROXY_SA" "$@"; }

live() {
  local phase="$1"; shift
  env PHARNESS_CAMPUS_LIVE_PHASE="$phase" PHARNESS_CAMPUS_LIVE_RUN="$RUN" \
    PHARNESS_CAMPUS_LIVE_SLOTS="$PROBE_SLOT" PHARNESS_CAMPUS_LIVE_REAL_SLOT="$MATRIX_SLOT" \
    PHARNESS_CAMPUS_LIVE_EGRESS_SLOTS="$(IFS=,; echo "${EGRESS_SLOTS[*]}")" \
    PHARNESS_CAMPUS_LIVE_EGRESS_CA_FILE="${OUT}/egress-ca.pem" \
    PHARNESS_CAMPUS_LIVE_CONTROLLER_ARGS="--context ${CONTEXT} --as ${CONTROLLER_SA}" \
    PHARNESS_CAMPUS_LIVE_ADMIN_ARGS="--context ${CONTEXT}" PHARNESS_CAMPUS_LIVE_OUT="$OUT" "$@" \
    cargo test --quiet --manifest-path "${ROOT}/Cargo.toml" -p pharness-core --test campus_session_jobs_live \
    -- --ignored --nocapture >"${OUT}/live-${phase}.log" 2>&1
}

SESSIONS_UP=false
cleanup() {
  local status=$?
  set +e
  echo "== Cleanup"
  if [[ "$SESSIONS_UP" == true ]]; then
    live egress-delete && echo "egress sessions deleted" || echo "egress delete failed, see ${OUT}/live-egress-delete.log"
  fi
  for ns in "${EGRESS_SLOTS[@]}" "$MATRIX_SLOT" "$BROKER_SLOT"; do
    k -n "$ns" delete jobs.batch,configmaps,secrets -l pharness.dev/campus-tenant=tenant-probe --ignore-not-found >/dev/null 2>&1
    # A delete of a session that was never provisioned leaves a tombstone
    # without a tenant.
    k -n "$ns" get configmaps -l pharness.dev/campus-tenant=unknown -o json 2>/dev/null |
      jq -r --arg run "probe-${RUN}-" '.items[] | select(.metadata.labels["pharness.dev/campus-session"] | startswith($run)) | .metadata.name' |
      xargs -r kubectl --context "$CONTEXT" -n "$ns" delete configmap --ignore-not-found >/dev/null 2>&1
  done
  for ns in "$PROBE_SLOT" "$RACE_NS" "$OPEN_NS" "$BROKER_NS"; do k delete namespace "$ns" --ignore-not-found --wait=false >/dev/null 2>&1; done
  k delete validatingadmissionpolicybinding "${GUARD}-probe" "${SECRET_GUARD}-probe" --ignore-not-found >/dev/null 2>&1
  for _ in $(seq 1 60); do
    remaining="$(k get namespaces "$PROBE_SLOT" "$RACE_NS" "$OPEN_NS" "$BROKER_NS" -o name 2>/dev/null)"
    [[ -z "$remaining" ]] && break
    sleep 3
  done
  left=""
  for ns in "${EGRESS_SLOTS[@]}" "$MATRIX_SLOT" "$BROKER_SLOT"; do
    left+="$(k -n "$ns" get jobs.batch,pods,secrets,configmaps -o name 2>/dev/null | grep -v kube-root-ca | sed "s|^|${ns}/|")"
  done
  echo "remaining probe namespaces: ${remaining:-none}"
  echo "remaining slot workloads: ${left:-none}"
  echo "evidence: ${OUT}"
  exit "$status"
}
trap cleanup EXIT

can() {
  local who="$1" expected="$2"; shift 2
  local got
  got="$("$who" auth can-i "$@" 2>/dev/null || true)"
  [[ "$got" == "$expected" ]] && pass "${who} can-i ${*} = ${got}" || fail "${who} can-i ${*} = ${got}, expected ${expected}"
}

echo "== Phase 1: preconditions (run ${RUN}, evidence ${OUT})"
app="$(k -n argocd get applications.argoproj.io campus-lab -o jsonpath='{.status.sync.status}/{.status.health.status}/{.status.sync.revision}')"
[[ "$app" == Synced/Healthy/* ]] && pass "Argo campus-lab ${app}" || fail "Argo campus-lab ${app}"
for policy in "$GUARD" "$SECRET_GUARD"; do
  warnings="$(k get validatingadmissionpolicy "$policy" -o jsonpath='{.status.typeChecking.expressionWarnings}')"
  [[ -z "$warnings" ]] && pass "${policy} type-checks without warnings" || fail "${policy} warnings: ${warnings}"
done
k -n "$PROXY_NS" rollout status deployment/campus-egress-proxy --timeout=180s >/dev/null &&
  pass "egress proxy rolled out" || fail "egress proxy not ready"
k -n "$PROXY_NS" get pods -l app=campus-egress-proxy -o json |
  jq -c '.items[] | {pod: .metadata.name, node: .spec.nodeName, image: .status.containerStatuses[0].imageID,
    ready: .status.containerStatuses[0].ready}' | tee "${OUT}/proxy-pods.jsonl"
ip="$(k -n "$PROXY_NS" get service campus-egress-proxy -o jsonpath='{.spec.clusterIP}:{.spec.ports[0].port}')"
[[ "$ip" == "10.43.130.30:3128" ]] && pass "proxy Service at ${ip}" || fail "proxy Service at ${ip}"
cert="$(k -n "$PROXY_NS" get certificate campus-egress-proxy-tls -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}')"
[[ "$cert" == True ]] && pass "proxy certificate Ready" || fail "proxy certificate ${cert}"
# The public CA certificate only; the key never leaves the cluster.
k -n "$PROXY_NS" get secret campus-egress-proxy-tls -o jsonpath='{.data.ca\.crt}' | base64 -d >"${OUT}/egress-ca.pem"
openssl x509 -in "${OUT}/egress-ca.pem" -noout -subject -enddate | tee "${OUT}/egress-ca.txt"
k -n "$PROXY_NS" get secret campus-egress-proxy-tls -o jsonpath='{.data.tls\.crt}' | base64 -d |
  openssl x509 -noout -ext subjectAltName | tr -d ' ' | tail -1 | tee "${OUT}/egress-sans.txt"
for ns in "${EGRESS_SLOTS[@]}" "$MATRIX_SLOT" "$BROKER_SLOT"; do
  got="$(k -n "$ns" get networkpolicies -o jsonpath='{.items[*].metadata.name}')"
  [[ "$got" == "campus-lab-default-deny campus-lab-session-broker campus-lab-session-egress" ]] &&
    pass "${ns} policies: ${got}" || fail "${ns} policies: ${got}"
done
api_pod="$(k -n pharness get pods -l app=pharness-api -o jsonpath='{.items[0].status.podIP}')"
echo "pharness-api Pod ${api_pod}"

echo "== Phase 2: RBAC"
S="${EGRESS_SLOTS[0]}"
can kc yes create secrets -n "$S"
can kc yes update secrets -n "$S"
can kc no patch secrets -n "$S"
can kc no watch secrets -n "$S"
can kc yes get networkpolicies -n "$S"
for verb in create update patch delete deletecollection; do can kc no "$verb" networkpolicies -n "$S"; done
can kc no patch namespaces/"$S"
can kc no get secrets -n "$PROXY_NS"
can kc no list pods -n "$PROXY_NS"
can kc no patch deployments.apps -n "$PROXY_NS"
can kc no update validatingadmissionpolicies/"$SECRET_GUARD"
can kc no delete validatingadmissionpolicybindings/"$SECRET_GUARD"
can kp yes list pods -n "$S"
can kp no get pods -n "$S"
can kp no watch pods -n "$S"
can kp no list secrets -n "$S"
can kp no list pods -n pharness
can kp no create pods -n "$S"
can kp no list pods -n "$PROXY_NS"
output="$(kc -n "$S" delete networkpolicy campus-lab-default-deny --dry-run=server 2>&1 || true)"
grep -qF forbidden <<<"$output" && pass "controller cannot delete the default-deny policy" || fail "delete default-deny: ${output}"
output="$(jq -n --arg ns "$S" '{apiVersion: "networking.k8s.io/v1", kind: "NetworkPolicy",
  metadata: {name: "allow-all", namespace: $ns}, spec: {podSelector: {}, egress: [{}], policyTypes: ["Egress"]}}' |
  kc create --dry-run=server -f - 2>&1 || true)"
grep -qF forbidden <<<"$output" && pass "controller cannot add an allow-all policy" || fail "allow-all: ${output}"
output="$(kc -n "$S" patch networkpolicy campus-lab-session-egress --type=merge -p '{"spec":{"podSelector":{}}}' --dry-run=server 2>&1 || true)"
grep -qF forbidden <<<"$output" && pass "controller cannot widen the egress policy" || fail "widen egress: ${output}"
output="$(kc label namespace "$S" pharness.dev/campus-lab- --dry-run=server 2>&1 || true)"
grep -qF forbidden <<<"$output" && pass "controller cannot unlabel a slot out of the guards" || fail "unlabel slot: ${output}"

echo "== Phase 3: Secret admission (${MATRIX_SLOT}, as the controller)"
secret() {
  jq -n --arg ns "$MATRIX_SLOT" '{apiVersion: "v1", kind: "Secret", type: "Opaque",
    metadata: {name: "campus-broker-0123456789abcdef01234567", namespace: $ns, labels: {
      "pharness.dev/campus-controller": "pharness-campus-controller", "pharness.dev/campus-tenant": "tenant-probe",
      "pharness.dev/campus-session": "secret-check", "pharness.dev/campus-slot": $ns,
      "pharness.dev/campus-generation": "1", "pharness.dev/campus-record": "broker-token"}},
    stringData: {token: "pcbt1_not-a-real-token"}}' | jq "$1"
}
expect_secret() {
  local label="$1" filter="$2" needle="$3" output
  output="$(secret "$filter" | kc create --dry-run=server -f - -o name 2>&1 || true)"
  if [[ "$needle" == admit ]]; then
    grep -q '^secret/' <<<"$output" && pass "admitted: ${label}" || fail "expected admission: ${label}: ${output}"
  elif grep -qF "$needle" <<<"$output" && grep -qF "$SECRET_GUARD" <<<"$output"; then
    pass "rejected: ${label}"
  else
    fail "rejected for another reason or admitted: ${label}: ${output}"
  fi
}
expect_secret "broker token Secret" '.' admit
expect_secret "broker token Secret with the egress CA" '.stringData["egress-ca.pem"] = "x"' admit
expect_secret "service-account token Secret" \
  '.type = "kubernetes.io/service-account-token" | .metadata.annotations = {"kubernetes.io/service-account.name": "default"} | del(.stringData)' \
  "only Opaque"
expect_secret "dockerconfigjson Secret" '.type = "kubernetes.io/dockerconfigjson" | .stringData = {".dockerconfigjson": "{}"}' "only Opaque"
expect_secret "other Secret name" '.metadata.name = "student-credentials"' "named as broker token"
expect_secret "workload record label" '.metadata.labels["pharness.dev/campus-record"] = "workload"' "broker-token record"
expect_secret "slot label of another namespace" '.metadata.labels["pharness.dev/campus-slot"] = "campus-lab-slot-1"' "broker-token record"
expect_secret "other controller" '.metadata.labels["pharness.dev/campus-controller"] = "other"' "broker-token record"
expect_secret "extra key" '.stringData.kubeconfig = "x"' "may hold only"
expect_secret "no token" '.stringData = {"egress-ca.pem": "x"}' "may hold only"

echo "== Phase 4: provider enforcement and egress sessions (${EGRESS_SLOTS[*]})"
SESSIONS_UP=true
if live egress-provision; then pass "egress provision phase"; else fail "egress provision phase"; fi
grep '^@@CAMPUS' "${OUT}/live-egress-provision.log" || tail -40 "${OUT}/live-egress-provision.log"
[[ -f "${OUT}/egress-pod-python.json" ]] || exit 1
declare -A POD IP NS
for purpose in node python denied; do
  POD[$purpose]="$(jq -r .metadata.name "${OUT}/egress-pod-${purpose}.json")"
  NS[$purpose]="$(jq -r .metadata.namespace "${OUT}/egress-pod-${purpose}.json")"
  IP[$purpose]="$(k -n "${NS[$purpose]}" get pod "${POD[$purpose]}" -o jsonpath='{.status.podIP}')"
  k -n "${NS[$purpose]}" get pod "${POD[$purpose]}" -o json | jq -c --arg p "$purpose" '{purpose: $p,
    ns: .metadata.namespace, ip: .status.podIP, node: .spec.nodeName, runtimeClass: .spec.runtimeClassName,
    image: .status.containerStatuses[0].imageID, egress: .metadata.labels["pharness.dev/campus-egress"],
    hosts: .metadata.annotations["pharness.dev/campus-egress-hosts"],
    gate: .status.initContainerStatuses[0].state.terminated | {exitCode, startedAt, finishedAt},
    started: .status.containerStatuses[0].state.running.startedAt,
    volumes: [.spec.volumes[] | {name, kind: (keys - ["name"])[0]}]}' | tee -a "${OUT}/egress-sessions.jsonl"
done
gated="$(jq -s '[.[] | select(.runtimeClass == "gvisor" and .gate.exitCode == 0)] | length' "${OUT}/egress-sessions.jsonl")"
[[ "$gated" == 3 ]] && pass "all three sessions run under gVisor after the network gate passed" || fail "gated sessions: ${gated}"
[[ "$(jq -r .egress "${OUT}/egress-pod-denied.json" 2>/dev/null)" == null ]] || true
labels="$(jq -s -c 'map({(.purpose): [.egress, .hosts]}) | add' "${OUT}/egress-sessions.jsonl")"
[[ "$labels" == '{"node":["proxy","codeload.github.com,files.pythonhosted.org,github.com,pypi.org,registry.npmjs.org"],"python":["proxy","codeload.github.com,files.pythonhosted.org,github.com,pypi.org,raw.githubusercontent.com,registry.npmjs.org"],"denied":[null,null]}' ]] &&
  pass "egress label and hosts follow each session's policy" || fail "egress labels: ${labels}"

echo "== Phase 5: session admission (${MATRIX_SLOT})"
jq --arg slot "$MATRIX_SLOT" 'del(.status, .metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp,
      .metadata.managedFields, .metadata.generation, .metadata.ownerReferences, .spec.selector)
    | .metadata.labels |= with_entries(select(.key | test("controller-uid|job-name") | not))
    | .spec.template.metadata.labels |= with_entries(select(.key | test("controller-uid|job-name") | not))
    | .metadata.name = "campus-egress-matrix" | .metadata.namespace = $slot
    | .metadata.labels["pharness.dev/campus-slot"] = $slot
    | .spec.template.metadata.labels["pharness.dev/campus-slot"] = $slot' \
  "${OUT}/egress-job-python.json" >"${OUT}/base.json"
T='.spec.template'
S='.spec.template.spec'
C='.spec.template.spec.containers[0]'
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
  if output="$(kc create --dry-run=server -f "${OUT}/case.json" -o name 2>&1)"; then
    fail "expected rejection: ${label}"
  elif grep -qF "$needle" <<<"$output" && grep -qF "ValidatingAdmissionPolicy" <<<"$output"; then
    pass "rejected: ${label}"
  else
    fail "rejected for another reason: ${label}: ${output}"
  fi
}
mutate '.'; expect_admit "egress session Job as the controller" kc
mutate '.'; expect_admit "egress session Job as an operator" k
mutate "del(${T}.metadata.annotations[\"pharness.dev/campus-egress-hosts\"])"; expect_reject "egress label without hosts" "Pods with egress"
mutate "del(${T}.metadata.labels[\"pharness.dev/campus-egress\"]) | del(.metadata.labels[\"pharness.dev/campus-egress\"])"
expect_reject "hosts without the egress label" "Pods with egress"
mutate "${T}.metadata.annotations[\"pharness.dev/campus-egress-hosts\"] = \"pypi.org,evil.example\""; expect_reject "unapproved host" "Pods with egress"
mutate "${T}.metadata.annotations[\"pharness.dev/campus-egress-hosts\"] = \"*\""; expect_reject "wildcard host" "Pods with egress"
mutate "${T}.metadata.labels[\"pharness.dev/campus-egress\"] = \"direct\" | .metadata.labels[\"pharness.dev/campus-egress\"] = \"direct\""
expect_reject "other egress label value" "Pods with egress"
mutate "del(${S}.initContainers)"; expect_reject "no network gate" "network gate"
mutate "${S}.initContainers[0].name = \"setup\""; expect_reject "gate renamed" "network gate"
mutate "${S}.initContainers += [${S}.initContainers[0] | .name = \"second\"]"; expect_reject "a second init container" "network gate"
mutate "${S}.initContainers[0].restartPolicy = \"Always\""; expect_reject "gate as a sidecar" "network gate"
mutate "${C}.volumeMounts |= map(if .name == \"campus-broker-token\" then .readOnly = false else . end)"
expect_reject "writable token mount" "read-only and without subPath"
mutate "${C}.volumeMounts |= map(if .name == \"campus-broker-token\" then .subPath = \"token\" else . end)"
expect_reject "token mount with subPath" "read-only and without subPath"
mutate "${S}.volumes |= map(if .name == \"campus-broker-token\" then .secret.optional = true else . end)"
expect_reject "optional token Secret" "broker token Secret"
mutate "${S}.volumes |= map(if .name == \"campus-broker-token\" then .secret.secretName = \"student-credentials\" else . end)"
expect_reject "token volume of another Secret" "broker token Secret"
mutate "${S}.volumes += [{name: \"other\", secret: {secretName: \"campus-broker-0123456789abcdef01234567\"}}]"
expect_reject "a second Secret volume" "broker token Secret"

echo "== Phase 6: functional egress"
x() { local purpose="$1"; shift; k -n "${NS[$purpose]}" exec -i "${POD[$purpose]}" -c workspace -- "$@"; }
timed() {
  local label="$1"; shift
  local started=$SECONDS output
  if output="$("$@" 2>&1)"; then
    pass "${label} ($((SECONDS - started))s)"
  else
    fail "${label}: $(tail -5 <<<"$output")"
  fi
  printf '%s\n' "$output" >"${OUT}/functional-${label// /-}.txt"
}
timed "pip install requests" x python /bin/sh -c \
  'pip install --no-cache-dir --disable-pip-version-check --target /workspace/pip-probe requests==2.32.3 &&
   PYTHONPATH=/workspace/pip-probe python3 -c "import requests; print(requests.__version__)"'
timed "git clone on Python" x python git clone --depth 1 https://github.com/octocat/Hello-World.git /workspace/hello
timed "npm install" x node /bin/sh -c \
  'npm install --prefix /workspace/npm-probe --no-audit --no-fund --no-update-notifier is-number@7.0.0 &&
   node -e "console.log(require(\"/workspace/npm-probe/node_modules/is-number\")(5))"'
timed "git clone on Node" x node git clone --depth 1 https://github.com/octocat/Hello-World.git /workspace/hello
x python python3 - functional \
  urls=https://codeload.github.com/octocat/Hello-World/tar.gz/refs/heads/master,https://raw.githubusercontent.com/octocat/Hello-World/master/README,https://pypi.org/simple/requests/ \
  <"${ROOT}/scripts/campus-egress-probe.py" | tee "${OUT}/functional-retrieval.txt" || true
PASSED=$((PASSED + $(grep -c '^PASS' "${OUT}/functional-retrieval.txt" || true)))
FAILED=$((FAILED + $(grep -c '^FAIL' "${OUT}/functional-retrieval.txt" || true)))
output="$(x python /bin/sh -c 'cd /workspace/hello && git -c user.email=s@example -c user.name=s commit --allow-empty -qm probe &&
  git push https://github.com/octocat/Hello-World.git HEAD:refs/heads/campus-probe' 2>&1 || true)"
echo "$output" >"${OUT}/git-push.txt"
grep -qE '403|refused' <<<"$output" && pass "git push refused: $(grep -E '403|refused' <<<"$output" | head -1)" ||
  fail "git push: ${output}"

echo "== Phase 7: isolation matrix from inside the sessions"
# A listener in the Node session, to prove a peer session cannot reach it.
x node /bin/sh -c 'nohup node -e "require(\"net\").createServer(s => s.end(\"peer\\n\")).listen(8080)" >/dev/null 2>&1 &' || true
sleep 2
local_peer="$(x node node -e 'const s = require("net").connect(8080, "127.0.0.1"); s.on("data", d => { console.log(String(d).trim()); process.exit(0) }); s.on("error", e => { console.log(e.code); process.exit(0) })' 2>&1 || true)"
[[ "$local_peer" == peer ]] && pass "the Node session's own listener answers locally" || fail "local listener: ${local_peer}"
x python python3 - matrix "api_pod=${api_pod}" "peer=${IP[node]}" <"${ROOT}/scripts/campus-egress-probe.py" |
  tee "${OUT}/matrix-python.txt" || true
x denied python3 - isolated "api_pod=${api_pod}" "peer=${IP[python]}" <"${ROOT}/scripts/campus-egress-probe.py" |
  tee "${OUT}/matrix-denied.txt" || true
for file in matrix-python matrix-denied; do
  PASSED=$((PASSED + $(grep -c '^PASS' "${OUT}/${file}.txt" || true)))
  FAILED=$((FAILED + $(grep -c '^FAIL' "${OUT}/${file}.txt" || true)))
  grep -q '^RESULT failed=0' "${OUT}/${file}.txt" || fail "${file} did not finish cleanly"
done
NODE_PROBE='
const net = require("net");
const [mode, host, port, authority] = process.argv.slice(1);
const s = net.connect({ host, port: Number(port), timeout: 3000 });
s.on("connect", () => {
  if (mode === "tcp") { console.log("open"); process.exit(0); }
  s.write(`CONNECT ${authority} HTTP/1.1\r\nHost: ${authority}\r\n\r\n`);
});
s.on("data", d => { console.log(String(d).split("\r\n")[0]); process.exit(0); });
s.on("timeout", () => { console.log("timeout"); process.exit(0); });
s.on("error", e => { console.log(e.code); process.exit(0); });'
for target in 10.43.0.1:443 192.168.20.192:6443 169.254.169.254:80 1.1.1.1:443 "${IP[python]}:8080" "${api_pod}:4777"; do
  got="$(x node node -e "$NODE_PROBE" tcp "${target%:*}" "${target##*:}" 2>&1 || true)"
  [[ "$got" != open ]] && pass "Node session cannot reach ${target} (${got})" || fail "Node session reached ${target}"
done
got="$(x node node -e "$NODE_PROBE" connect 10.43.130.30 3128 raw.githubusercontent.com:443 2>&1 || true)"
[[ "$got" == "HTTP/1.1 403"* ]] && pass "Node session: a host approved only for another session is refused (${got})" ||
  fail "Node session raw.githubusercontent.com: ${got}"
got="$(x node node -e "$NODE_PROBE" connect 10.43.130.30 3128 registry.npmjs.org:443 2>&1 || true)"
[[ "$got" == "HTTP/1.1 200"* ]] && pass "Node session: its approved host opens a tunnel (${got})" || fail "Node session npm tunnel: ${got}"

echo "== Phase 8: proxy decision log"
k -n "$PROXY_NS" logs deployment/campus-egress-proxy --since=1h | sed 's/\x1b\[[0-9;]*m//g' >"${OUT}/proxy.log"
for code in git_push method upgrade host_mismatch target; do
  grep -qF "decision=\"${code}\"" "${OUT}/proxy.log" &&
    pass "proxy logged a ${code} refusal" || fail "proxy log has no ${code} refusal"
done
for refusal in "the host is not approved for this session" "only CONNECT to an approved host on port 443"; do
  grep -qF "$refusal" "${OUT}/proxy.log" && pass "proxy logged CONNECT refusals: ${refusal}" ||
    fail "proxy log has no CONNECT refusal: ${refusal}"
done
grep -q "campus egress tunnel opened" "${OUT}/proxy.log" && pass "proxy logged opened tunnels with their session" || fail "no tunnel log"
if grep -qE 'pcbt1_|authorization:|Bearer ' "${OUT}/proxy.log"; then fail "proxy log carries a credential"; else pass "proxy log carries no credential"; fi
grep -c 'campus egress' "${OUT}/proxy.log" | xargs echo "proxy log lines:"

echo "== Phase 9: startup race and the network gate (${RACE_NS}, ${OPEN_NS})"
for ns in "$RACE_NS" "$OPEN_NS"; do
  k create namespace "$ns" >/dev/null
  k label namespace "$ns" pharness.dev/purpose=luc-130-probe >/dev/null
done
jq -n --arg ns "$RACE_NS" '{apiVersion: "networking.k8s.io/v1", kind: "NetworkPolicy",
  metadata: {name: "campus-lab-default-deny", namespace: $ns}, spec: {podSelector: {}, policyTypes: ["Ingress", "Egress"]}}' |
  k apply -f - >/dev/null
GATE="$(jq -c '.spec.initContainers[0].command' "${OUT}/egress-pod-python.json")"
race_pod() {
  local ns="$1" name="$2" gate="$3"
  jq -n --arg ns "$ns" --arg name "$name" --arg image "$PYTHON_IMAGE" --argjson gate "$gate" '
    {apiVersion: "v1", kind: "Pod", metadata: {name: $name, namespace: $ns},
     spec: {runtimeClassName: "gvisor", restartPolicy: "Never", automountServiceAccountToken: false,
       activeDeadlineSeconds: 120,
       securityContext: {runAsNonRoot: true, runAsUser: 65532, seccompProfile: {type: "RuntimeDefault"}},
       initContainers: (if $gate == null then [] else [{name: "network-gate", image: $image, command: $gate,
         securityContext: {allowPrivilegeEscalation: false, capabilities: {drop: ["ALL"]}}}] end),
       containers: [{name: "workspace", image: $image,
         command: ["python3", "-c", "import socket, time\nstart = time.monotonic(); first = last = None\nwhile time.monotonic() - start < 12:\n    s = socket.socket(); s.settimeout(0.2)\n    try:\n        s.connect((\"10.43.0.1\", 443)); now = time.monotonic() - start\n        first = now if first is None else first; last = now\n    except OSError:\n        pass\n    s.close(); time.sleep(0.05)\nprint(\"api reachable from\", first, \"to\", last, \"seconds after start\")"],
         securityContext: {allowPrivilegeEscalation: false, capabilities: {drop: ["ALL"]}}}]}}'
}
wait_done() {
  for _ in $(seq 1 60); do
    phase="$(k -n "$1" get pod "$2" -o jsonpath='{.status.phase}' 2>/dev/null)"
    [[ "$phase" == Succeeded || "$phase" == Failed ]] && return 0
    sleep 2
  done
  return 1
}
race_pod "$RACE_NS" race-ungated null | k apply -f - >/dev/null
wait_done "$RACE_NS" race-ungated || true
ungated="$(k -n "$RACE_NS" logs race-ungated 2>&1 || true)"
echo "ungated Pod under default-deny: ${ungated}" | tee "${OUT}/race-ungated.txt"
race_pod "$RACE_NS" race-gated "$GATE" | k apply -f - >/dev/null
wait_done "$RACE_NS" race-gated || true
gated="$(k -n "$RACE_NS" logs race-gated -c workspace 2>&1 || true)"
k -n "$RACE_NS" get pod race-gated -o json | jq -c '{phase: .status.phase,
  gate: .status.initContainerStatuses[0].state.terminated | {exitCode, startedAt, finishedAt},
  workspace: .status.containerStatuses[0].state.terminated | {exitCode, startedAt}}' | tee "${OUT}/race-gated.json"
[[ "$gated" == "api reachable from None to None seconds after start" ]] &&
  pass "gated Pod never reaches the API from its first instant" || fail "gated Pod: ${gated}"
# No policy at all: the gate never sees isolation and the workspace never runs.
short_gate="$(jq -c '.[4] = "10"' <<<"$GATE")"
race_pod "$OPEN_NS" gate-open "$short_gate" | k apply -f - >/dev/null
wait_done "$OPEN_NS" gate-open || true
k -n "$OPEN_NS" get pod gate-open -o json | jq -c '{phase: .status.phase,
  gate: .status.initContainerStatuses[0].state.terminated | {exitCode, reason},
  workspace: .status.containerStatuses[0].state}' | tee "${OUT}/gate-open.json"
k -n "$OPEN_NS" logs gate-open -c network-gate 2>&1 | tail -1 | tee "${OUT}/gate-open.log"
[[ "$(jq -r '.phase + "/" + (.gate.exitCode | tostring) + "/" + (.workspace | keys[0])' <"${OUT}/gate-open.json" | head -1)" == Failed/1/waiting ]] &&
  pass "without enforcement the gate fails the Pod and the workspace never starts" || fail "open gate: $(head -1 "${OUT}/gate-open.json")"

echo "== Phase 10: missing or weakened slot policy (${PROBE_SLOT})"
k create namespace "$PROBE_SLOT" >/dev/null
k label namespace "$PROBE_SLOT" "${PROBE_LABEL}=true" "pharness.dev/campus-slot=${PROBE_SLOT}" \
  pharness.dev/purpose=luc-130-probe pod-security.kubernetes.io/enforce=restricted >/dev/null
helm template campus-lab "$CHART" --set "probe.namespace=${PROBE_SLOT}" | k apply -f - >/dev/null
k -n "$PROBE_SLOT" delete networkpolicy campus-lab-session-broker >/dev/null
if live enforcement PHARNESS_CAMPUS_LIVE_EXPECT="campus-lab-session-broker does not exist"; then
  pass "missing broker policy refuses the launch"
else
  fail "missing broker policy: $(grep -E 'panicked|reason' "${OUT}/live-enforcement.log" | head -3)"
fi
grep '^@@CAMPUS' "${OUT}/live-enforcement.log" || true
helm template campus-lab "$CHART" --set "probe.namespace=${PROBE_SLOT}" | k apply -f - >/dev/null
k -n "$PROBE_SLOT" patch networkpolicy campus-lab-default-deny --type=merge -p '{"spec":{"policyTypes":["Ingress"]}}' >/dev/null
if live enforcement PHARNESS_CAMPUS_LIVE_EXPECT="no longer denies all ingress and egress"; then
  pass "weakened default-deny refuses the launch"
else
  fail "weakened default-deny: $(grep -E 'panicked|reason' "${OUT}/live-enforcement.log" | head -3)"
fi
grep '^@@CAMPUS' "${OUT}/live-enforcement.log" || true

echo "== Phase 11: broker token scrape, rotation and revocation (${BROKER_NS}, slot ${BROKER_SLOT_ID})"
if [[ "$SKIP_BROKER" != true ]]; then
  k create namespace "$BROKER_NS" >/dev/null
  k label namespace "$BROKER_NS" "${PROBE_LABEL}=true" "pharness.dev/campus-slot=${BROKER_SLOT_ID}" \
    pharness.dev/purpose=luc-130-probe pod-security.kubernetes.io/enforce=restricted >/dev/null
  helm template campus-lab "$CHART" --set "probe.namespace=${BROKER_NS}" | k apply -f - >/dev/null
fi
if [[ "$SKIP_BROKER" == true ]]; then
  echo "skipped"
elif PHARNESS_CAMPUS_BROKER_LIVE_SLOT="$BROKER_NS" \
    PHARNESS_CAMPUS_BROKER_LIVE_CONTROLLER_ARGS="--context ${CONTEXT} --as ${CONTROLLER_SA}" \
    PHARNESS_CAMPUS_BROKER_LIVE_ADMIN_ARGS="--context ${CONTEXT}" PHARNESS_CAMPUS_BROKER_LIVE_OUT="$OUT" \
    cargo test --quiet --manifest-path "${ROOT}/Cargo.toml" -p pharness-api campus_broker_live \
    -- --ignored --nocapture >"${OUT}/live-broker.log" 2>&1; then
  pass "broker token live phase"
else
  fail "broker token live phase"
fi
grep -E '^(@@CAMPUS|PASS|FAIL)' "${OUT}/live-broker.log" 2>/dev/null || tail -40 "${OUT}/live-broker.log" 2>/dev/null || true

echo "== Phase 12: provider delete"
if live egress-delete; then pass "egress delete phase"; SESSIONS_UP=false; else fail "egress delete phase"; fi
grep '^@@CAMPUS' "${OUT}/live-egress-delete.log" || tail -20 "${OUT}/live-egress-delete.log"

echo "== Result: ${PASSED} passed, ${FAILED} failed"
[[ "$FAILED" == 0 ]]
