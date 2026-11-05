#!/usr/bin/env bash
set -euo pipefail

# Assess one released PHarness image for Campus labs and record the evidence.
#
# The digest must belong to the immutable release record. A Tekton TaskRun in
# tekton-pipelines then generates an SPDX SBOM with syft, scans it with grype,
# applies the Critical/High-with-fix policy and the reviewed exceptions,
# attests the SBOM and the vulnerability disposition, and signs the digest only
# when nothing blocks it. The private key is mounted from the
# campus-cosign-signing Secret inside that Pod and never reaches this machine.
# Back here, every attestation and signature is verified against the committed
# public key before deploy/campus/approved-images.json and the generated
# admission allowlist are rewritten for review.
#
# --key-qualification signs a no-approval canary payload instead, to prove that
# the Secret still matches deploy/campus/cosign.pub after creation or rotation.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel)"
GATE=(python3 "${SCRIPT_DIR}/campus_image_gate.py")
CAMPUS="${ROOT}/deploy/campus"
CONTEXT="${PHARNESS_CLUSTER_CONTEXT:-lucas_engineering}"
NAMESPACE="tekton-pipelines"
COSIGN="${PHARNESS_COSIGN:-cosign}"
COSIGN_VERSION="$("${SCRIPT_DIR}/campus-install-supply-chain-tools.sh" --print-versions | awk -F= '$1 == "cosign" { print $2 }')"
IMAGE=""
REVISION=""
RELEASE_DIR=""
KEEP_RUN=false
KEY_QUALIFICATION=false
FROM_TASKRUN=""
TIMEOUT_SECONDS=2700

usage() {
  cat >&2 <<EOF
Usage:
  $0 --image <registry.lucas.engineering/repo@sha256:...> --revision <40-char-sha> \\
     [--release-dir <dir containing BUILD.json and ARTIFACTS.json>] [--context <ctx>] \\
     [--timeout <seconds>] [--keep-run] [--from-taskrun <name>]
  $0 --key-qualification [--context <ctx>] [--keep-run]

--from-taskrun re-collects evidence from an existing finished approval TaskRun
(for example after a workstation-side verification failure) instead of
starting a new assessment.

The release directory defaults to dist/releases/<revision> in this checkout.
Install the pinned CLIs with scripts/campus-install-supply-chain-tools.sh.
EOF
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="${2:-}"; shift 2 ;;
    --revision) REVISION="${2:-}"; shift 2 ;;
    --release-dir) RELEASE_DIR="${2:-}"; shift 2 ;;
    --context) CONTEXT="${2:-}"; shift 2 ;;
    --timeout) TIMEOUT_SECONDS="${2:-}"; shift 2 ;;
    --keep-run) KEEP_RUN=true; shift ;;
    --from-taskrun) FROM_TASKRUN="${2:-}"; shift 2 ;;
    --key-qualification) KEY_QUALIFICATION=true; shift ;;
    *) usage ;;
  esac
done

[[ "$CONTEXT" =~ ^[A-Za-z0-9][A-Za-z0-9._/-]*$ ]] || usage
[[ "$TIMEOUT_SECONDS" =~ ^[1-9][0-9]{1,4}$ ]] || usage
for tool in kubectl jq python3 curl; do
  command -v "$tool" >/dev/null || { echo "${tool} is required" >&2; exit 1; }
done
command -v "$COSIGN" >/dev/null || {
  echo "cosign was not found; run scripts/campus-install-supply-chain-tools.sh" >&2
  exit 1
}
"$COSIGN" version --json 2>/dev/null | jq -e --arg v "v${COSIGN_VERSION}" '.gitVersion == $v' >/dev/null || {
  echo "cosign must be the pinned v${COSIGN_VERSION}" >&2
  exit 1
}

k() { kubectl --context "$CONTEXT" -n "$NAMESPACE" "$@"; }

TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/campus-image-approve.XXXXXX")"
RUN_NAME=""
cleanup() {
  if [[ -n "$RUN_NAME" && "$KEEP_RUN" != true ]]; then
    k delete taskrun "$RUN_NAME" --ignore-not-found --wait=false >/dev/null 2>&1 || true
  fi
  if [[ -n "${TEMP_ROOT:-}" && "$TEMP_ROOT" == */campus-image-approve.* ]]; then
    rm -rf -- "$TEMP_ROOT"
  fi
}
trap cleanup EXIT

preflight_cluster() {
  # Only object names are read; Secret data never leaves the cluster.
  k get secret campus-cosign-signing -o name >/dev/null
  k get secret lucas-registry-push -o name >/dev/null
  k get secret registry-internal-ca -o name >/dev/null
  k get serviceaccount tekton-ci-build -o json | jq -e '.automountServiceAccountToken == false' >/dev/null || {
    echo "tekton-ci-build must keep automountServiceAccountToken=false" >&2
    return 1
  }
}

run_taskrun() {
  local manifest="$1" deadline condition
  k create --dry-run=server -f "$manifest" -o name >/dev/null
  k create -f "$manifest" -o name >&2
  deadline=$((SECONDS + TIMEOUT_SECONDS))
  while :; do
    condition="$(k get taskrun "$RUN_NAME" -o jsonpath='{.status.conditions[?(@.type=="Succeeded")].status}')"
    [[ "$condition" == True || "$condition" == False ]] && break
    if (( SECONDS > deadline )); then
      echo "TaskRun ${RUN_NAME} did not finish within ${TIMEOUT_SECONDS}s" >&2
      return 1
    fi
    sleep 10
  done
  k get taskrun "$RUN_NAME" -o json >"${TEMP_ROOT}/taskrun.json"
}

if [[ "$KEY_QUALIFICATION" == true ]]; then
  [[ -z "$IMAGE" && -z "$REVISION" ]] || usage
  preflight_cluster
  created="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  RUN_NAME="campus-key-qualification-$(date -u +%Y%m%d%H%M%S)"
  "${GATE[@]}" render-canary-taskrun --name "$RUN_NAME" --created "$created" >"${TEMP_ROOT}/canary.json"
  run_taskrun "${TEMP_ROOT}/canary.json"
  jq -e '[.status.conditions[] | select(.type == "Succeeded")][0].status == "True"' "${TEMP_ROOT}/taskrun.json" >/dev/null || {
    echo "key qualification TaskRun ${RUN_NAME} failed" >&2
    exit 1
  }
  out_dir="${CAMPUS}/key-qualification"
  mkdir -p "$out_dir"
  base="${out_dir}/${created%%T*}-${RUN_NAME}"
  jq -j '.status.results[] | select(.name == "PAYLOAD") | .value' "${TEMP_ROOT}/taskrun.json" >"${base}.json"
  jq -j '.status.results[] | select(.name == "SIGNATURE") | .value' "${TEMP_ROOT}/taskrun.json" >"${base}.sig"
  "$COSIGN" verify-blob --key "${CAMPUS}/cosign.pub" --insecure-ignore-tlog=true \
    --signature "${base}.sig" "${base}.json"
  python3 - "$CAMPUS/approved-images.json" "${base#"${ROOT}/"}" "$RUN_NAME" "$created" <<'PY'
import hashlib, json, sys
from pathlib import Path
evidence_path, base, run, created = sys.argv[1:]
root = Path(evidence_path).parents[2]
evidence = json.loads(Path(evidence_path).read_text())
payload = (root / f"{base}.json").read_bytes()
evidence.setdefault("key_qualification", []).append({
    "payload": f"{base}.json",
    "signature": f"{base}.sig",
    "payload_sha256": "sha256:" + hashlib.sha256(payload).hexdigest(),
    "public_key_sha256": evidence["signing"]["public_key_sha256"],
    "taskrun": f"tekton-pipelines/{run}",
    "created": created,
})
Path(evidence_path).write_text(json.dumps(evidence, indent=2) + "\n")
PY
  "${GATE[@]}" check --root "$ROOT"
  echo "key qualification recorded at ${base}.json"
  exit 0
fi

[[ -n "$IMAGE" && "$REVISION" =~ ^[0-9a-f]{40}$ ]] || usage
RELEASE_DIR="${RELEASE_DIR:-${ROOT}/dist/releases/${REVISION}}"
BUILD_JSON="${RELEASE_DIR}/BUILD.json"
ARTIFACTS_JSON="${RELEASE_DIR}/ARTIFACTS.json"

echo "== Release record" >&2
"${GATE[@]}" release-check --build "$BUILD_JSON" --artifacts "$ARTIFACTS_JSON" \
  --image "$IMAGE" --revision "$REVISION" >"${TEMP_ROOT}/release.json"
component="$(jq -er '.component' "${TEMP_ROOT}/release.json")"
jq -c '{component, pipeline_run, config_digest, platform}' "${TEMP_ROOT}/release.json" >&2

echo "== Registry digest" >&2
repository="${IMAGE%@*}"
digest="${IMAGE#*@}"
path="${repository#registry.lucas.engineering/}"
served="$(curl --fail --silent --show-error --head \
  -H 'Accept: application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json' \
  "https://registry.lucas.engineering/v2/${path}/manifests/${digest}" | tr -d '\r' |
  awk 'tolower($1) == "docker-content-digest:" { print $2 }')"
[[ "$served" == "$digest" ]] || {
  echo "registry.lucas.engineering does not serve ${IMAGE}" >&2
  exit 1
}

echo "== Repository evidence and exceptions" >&2
"${GATE[@]}" check --root "$ROOT" >&2
preflight_cluster

if [[ -n "$FROM_TASKRUN" ]]; then
  [[ "$FROM_TASKRUN" =~ ^campus-approve-[a-z0-9-]+$ ]] || usage
  RUN_NAME="$FROM_TASKRUN"
  echo "== Re-collecting TaskRun ${NAMESPACE}/${RUN_NAME}" >&2
  k get taskrun "$RUN_NAME" -o json >"${TEMP_ROOT}/taskrun.json"
else
  RUN_NAME="campus-approve-${component}-${digest:7:12}-$(date -u +%H%M%S)"
  "${GATE[@]}" render-taskrun --name "$RUN_NAME" --image "$IMAGE" --revision "$REVISION" \
    --component "$component" --exceptions "${CAMPUS}/vulnerability-exceptions.json" >"${TEMP_ROOT}/approve.json"
  echo "== TaskRun ${NAMESPACE}/${RUN_NAME}" >&2
  run_taskrun "${TEMP_ROOT}/approve.json"
fi
pod="$(jq -r '.status.podName // empty' "${TEMP_ROOT}/taskrun.json")"
if [[ -n "$pod" ]]; then
  k logs "$pod" -c step-evaluate 2>/dev/null | sed -n '1,200p' >&2 || true
fi
"${GATE[@]}" taskrun-outcome --taskrun "${TEMP_ROOT}/taskrun.json" | jq -c '{outcome, results}' >&2

echo "== Verifying attestations and signature against deploy/campus/cosign.pub" >&2
"${GATE[@]}" collect \
  --taskrun "${TEMP_ROOT}/taskrun.json" \
  --release-check "${TEMP_ROOT}/release.json" \
  --image "$IMAGE" \
  --exceptions "${CAMPUS}/vulnerability-exceptions.json" \
  --public-key "${CAMPUS}/cosign.pub" \
  --cosign "$COSIGN" \
  --build-sha256 "sha256:$(sha256sum "$BUILD_JSON" | awk '{ print $1 }')" \
  --artifacts-sha256 "sha256:$(sha256sum "$ARTIFACTS_JSON" | awk '{ print $1 }')" \
  >"${TEMP_ROOT}/entry.json"
"${GATE[@]}" record --evidence "${CAMPUS}/approved-images.json" --entry "${TEMP_ROOT}/entry.json"
"${GATE[@]}" render-policy --evidence "${CAMPUS}/approved-images.json" \
  >"${CAMPUS}/admission/campus-lab-image-allowlist.yaml"
"${GATE[@]}" check --root "$ROOT" >&2
jq '{image, status, sbom: .sbom.canonical_sha256, summary: .vulnerability_scan.summary,
  blocking: (.vulnerability_scan.blocking_findings | length), signature: .signature.reference,
  attestations: .attestations}' "${TEMP_ROOT}/entry.json"
echo "recorded $(jq -r .status "${TEMP_ROOT}/entry.json") evidence for ${IMAGE}; review and commit deploy/campus" >&2
