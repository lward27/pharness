#!/usr/bin/env bash
set -euo pipefail

# Build immutable PHarness OCI images through the lucas_engineering Tekton
# pipeline and its in-cluster rootless BuildKit service. The local Buildx path
# remains available as pharness-build-local.sh.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel)"
TARGET="${1:-}"
REVISION=""
CLUSTER_CONTEXT="${PHARNESS_CLUSTER_CONTEXT:-lucas_engineering}"
KUBECTL_BIN="${PHARNESS_KUBECTL:-kubectl}"
PIPELINE_NAMESPACE="tekton-pipelines"
PIPELINE_NAME="clone-build-push"
BUILD_SERVICE_ACCOUNT="tekton-ci-build"
BUILD_TIMEOUT_SECONDS="3600"
PREFLIGHT_ONLY=false

usage() {
  cat >&2 <<EOF
Usage: $0 <runtime|ui|python-runner|node-runner|model-gateway|eval-runner|codex-host|all> \\
  --revision <40-char-sha> [--context <kubectl-context>] [--timeout <seconds>] \\
  [--preflight-only]

OCI image builds run in lucas_engineering via Tekton Pipeline ${PIPELINE_NAME}.
codex-host and all also build the native bundle target in the cluster and fetch
that immutable artifact for local archive/checksum handling. No local Buildx
compilation is used; pharness-build-local.sh remains the explicit local path.
EOF
  exit 2
}

[[ "$TARGET" =~ ^(runtime|ui|python-runner|node-runner|model-gateway|eval-runner|codex-host|all)$ ]] || usage
shift
while [[ $# -gt 0 ]]; do
  case "$1" in
    --revision) REVISION="${2:-}"; shift 2 ;;
    --context) CLUSTER_CONTEXT="${2:-}"; shift 2 ;;
    --timeout) BUILD_TIMEOUT_SECONDS="${2:-}"; shift 2 ;;
    --preflight-only) PREFLIGHT_ONLY=true; shift ;;
    *) usage ;;
  esac
done

[[ "$REVISION" =~ ^[0-9a-f]{40}$ ]] || {
  echo "--revision must be a full lowercase 40-character Git SHA" >&2
  exit 1
}
[[ "$CLUSTER_CONTEXT" =~ ^[A-Za-z0-9][A-Za-z0-9._/-]*$ ]] || {
  echo "--context must name one explicit kubectl context" >&2
  exit 1
}
[[ "$BUILD_TIMEOUT_SECONDS" =~ ^[1-9][0-9]{0,4}$ ]] || {
  echo "--timeout must be a positive number of seconds (maximum 99999)" >&2
  exit 1
}

needs_bundle=false
if [[ "$TARGET" == "all" || "$TARGET" == "codex-host" ]]; then
  needs_bundle=true
fi

command -v "$KUBECTL_BIN" >/dev/null || {
  echo "kubectl was not found at the configured executable: ${KUBECTL_BIN}" >&2
  exit 1
}
if [[ "$needs_bundle" == true ]]; then
  command -v python3 >/dev/null || {
    echo "python3 is required to fetch and verify the in-cluster native bundle artifact" >&2
    exit 1
  }
  command -v tar >/dev/null || {
    echo "tar is required to package the verified native bundle" >&2
    exit 1
  }
fi

components=()
case "$TARGET" in
  all) components=(runtime ui python-runner node-runner model-gateway eval-runner codex-host) ;;
  *) components=("$TARGET") ;;
esac

verify_source_revision() {
  local output verified
  output="$("${SCRIPT_DIR}/pharness-verify-build-revision.sh" \
    --repo "$REPOSITORY_ROOT" \
    --remote "${PHARNESS_BUILD_REMOTE:-origin}" \
    --branch "${PHARNESS_BUILD_BRANCH:-main}" \
    --revision "$REVISION")"
  verified="$(awk -F= '$1 == "verified_revision" { print $2 }' <<<"$output")"
  [[ "$verified" == "$REVISION" ]] || {
    echo "source verification did not return the requested immutable revision" >&2
    return 1
  }
}

kubectl() {
  command "$KUBECTL_BIN" --context "$CLUSTER_CONTEXT" -n "$PIPELINE_NAMESPACE" "$@"
}

check_cluster_builder() {
  local service_account endpoint_slice builder_pod pod_json node_name node_json

  kubectl get "pipeline/${PIPELINE_NAME}" -o name >/dev/null
  service_account="$(kubectl get "serviceaccount/${BUILD_SERVICE_ACCOUNT}" -o json)"
  jq -e '.automountServiceAccountToken == false' <<<"$service_account" >/dev/null || {
    echo "${BUILD_SERVICE_ACCOUNT} must have automountServiceAccountToken=false" >&2
    return 1
  }

  endpoint_slice="$(kubectl get endpointslices \
    -l kubernetes.io/service-name=k3s-buildkit -o json)"
  builder_pod="$(jq -er '
    [.items[].endpoints[]? | select(.conditions.ready == true)
      | select(.targetRef.kind == "Pod") | .targetRef.name] | unique
      | if length == 1 then .[0] else error("expected one ready BuildKit endpoint") end
  ' <<<"$endpoint_slice")" || {
    echo "k3s-buildkit must have exactly one ready in-cluster endpoint" >&2
    return 1
  }
  pod_json="$(kubectl get "pod/${builder_pod}" -o json)"
  jq -e '[.status.conditions[]? | select(.type == "Ready" and .status == "True")] | length == 1' \
    <<<"$pod_json" >/dev/null || {
    echo "the in-cluster BuildKit Pod is not Ready" >&2
    return 1
  }
  node_name="$(jq -er '.spec.nodeName | select(length > 0)' <<<"$pod_json")" || {
    echo "the in-cluster BuildKit Pod is not scheduled to a node" >&2
    return 1
  }
  node_json="$(kubectl get "node/${node_name}" -o json)"
  jq -e '.metadata.labels.workload == "build" and .metadata.labels["kubernetes.io/arch"] == "amd64"' \
    <<<"$node_json" >/dev/null || {
    echo "the BuildKit endpoint is not running on an AMD64 workload=build node" >&2
    return 1
  }
}

component_dockerfile() {
  case "$1" in
    runtime) echo "./deploy/docker/Dockerfile.runtime" ;;
    ui) echo "./deploy/docker/Dockerfile.ui" ;;
    python-runner) echo "./deploy/docker/Dockerfile.python-runner" ;;
    node-runner) echo "./deploy/docker/Dockerfile.node-runner" ;;
    model-gateway) echo "./deploy/docker/Dockerfile.model-gateway" ;;
    eval-runner) echo "./deploy/docker/Dockerfile.eval-runner" ;;
    codex-host) echo "./deploy/docker/Dockerfile.codex-host" ;;
    *) echo "unsupported PHarness component ${1}" >&2; return 1 ;;
  esac
}

TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/pharness-incluster-build.XXXXXX")"
cleanup() {
  if [[ -n "${TEMP_ROOT:-}" && -d "$TEMP_ROOT" && "$TEMP_ROOT" == */pharness-incluster-build.* ]]; then
    rm -rf -- "$TEMP_ROOT"
  fi
}
trap cleanup EXIT

build_receipts=()
NATIVE_BUNDLE_DIGEST=""
NATIVE_BUNDLE_IMAGE=""
NATIVE_BUNDLE_ARCHIVE=""
NATIVE_BUNDLE_ARCHIVE_SHA256=""
make_run_manifest() {
  local component="$1" build_target="$2" image_component="$3" run_name image_ref dockerfile
  run_name="pharness-${image_component}-${REVISION:0:12}"
  image_ref="registry.lucas.engineering/pharness-${image_component}:git-${REVISION}"
  dockerfile="$(component_dockerfile "$component")"
  jq -n \
    --arg name "$run_name" \
    --arg namespace "$PIPELINE_NAMESPACE" \
    --arg component "$component" \
    --arg image_component "$image_component" \
    --arg build_target "$build_target" \
    --arg revision "$REVISION" \
    --arg image "$image_ref" \
    --arg dockerfile "$dockerfile" \
    --arg context "$CLUSTER_CONTEXT" \
    --arg pipeline "$PIPELINE_NAME" \
    --arg service_account "$BUILD_SERVICE_ACCOUNT" \
    --arg storage "8Gi" \
    --arg timeout "${BUILD_TIMEOUT_SECONDS}s" \
    '{apiVersion:"tekton.dev/v1",kind:"PipelineRun",
      metadata:{name:$name,namespace:$namespace,
        labels:{"app.kubernetes.io/part-of":"pharness","app.kubernetes.io/component":$image_component},
        annotations:{"pharness.lucas.engineering/source-commit":$revision,"pharness.lucas.engineering/cluster-context":$context,"pharness.lucas.engineering/build-target":$build_target}},
      spec:{pipelineRef:{name:$pipeline},
        params:[
          {name:"repo-url",value:"https://github.com/lward27/pharness.git"},
          {name:"revision",value:$revision},
          {name:"image-reference",value:$image},
          {name:"dockerfile",value:$dockerfile},
          {name:"context",value:"./"},
          {name:"build-args",value:["PHARNESS_BUILD_REVISION="+$revision,"TARGETARCH=amd64"]},
          {name:"build-target",value:$build_target},
          {name:"deployment",value:""},
          {name:"deployment-namespace",value:"pharness"}],
        taskRunTemplate:{serviceAccountName:$service_account},
        timeouts:{pipeline:$timeout},
        workspaces:[{name:"shared-data",volumeClaimTemplate:{spec:{storageClassName:"local-path",
          accessModes:["ReadWriteOnce"],resources:{requests:{storage:$storage}}}}}]}}'
}

validate_existing_run() {
  local run_json="$1" component="$2" build_target="$3" image_component="$4" image_ref="$5" dockerfile="$6"
  jq -e \
    --arg run "pharness-${image_component}-${REVISION:0:12}" \
    --arg revision "$REVISION" \
    --arg image "$image_ref" \
    --arg dockerfile "$dockerfile" \
    --arg repo "https://github.com/lward27/pharness.git" \
    --arg pipeline "$PIPELINE_NAME" \
    --arg service_account "$BUILD_SERVICE_ACCOUNT" \
    --arg context "$CLUSTER_CONTEXT" \
    '.metadata.name == $run and .spec.pipelineRef.name == $pipeline
     and .metadata.annotations["pharness.lucas.engineering/source-commit"] == $revision
     and .metadata.annotations["pharness.lucas.engineering/cluster-context"] == $context
     and .spec.taskRunTemplate.serviceAccountName == $service_account
     and ([.spec.params[] | select(.name == "repo-url" and .value == $repo)] | length) == 1
     and ([.spec.params[] | select(.name == "revision" and .value == $revision)] | length) == 1
     and ([.spec.params[] | select(.name == "image-reference" and .value == $image)] | length) == 1
     and ([.spec.params[] | select(.name == "dockerfile" and .value == $dockerfile)] | length) == 1
     and ([.spec.params[] | select(.name == "context" and .value == "./")] | length) == 1
     and ([.spec.params[] | select(.name == "build-args" and .value == ["PHARNESS_BUILD_REVISION="+$revision,"TARGETARCH=amd64"])] | length) == 1
     and ([.spec.params[] | select(.name == "build-target" and .value == $build_target)] | length) == 1
     and ([.spec.params[] | select(.name == "deployment" and .value == "")] | length) == 1
     and ([.spec.params[] | select(.name == "deployment-namespace" and .value == "pharness")] | length) == 1' \
    <<<"$run_json" >/dev/null || {
    echo "existing PipelineRun name has different build inputs; refusing to reuse it" >&2
    return 1
  }
}

run_component() {
  local component="$1" build_target="$2" image_component="$3" run_name image_ref dockerfile manifest run_json wait_output
  local fetch_taskrun build_taskrun fetch_taskrun_json build_taskrun_json source_commit image_url digest condition

  run_name="pharness-${image_component}-${REVISION:0:12}"
  image_ref="registry.lucas.engineering/pharness-${image_component}:git-${REVISION}"
  dockerfile="$(component_dockerfile "$component")"
  manifest="${TEMP_ROOT}/${component}.pipelinerun.json"
  make_run_manifest "$component" "$build_target" "$image_component" >"$manifest"

  if [[ "$PREFLIGHT_ONLY" == true ]]; then
    verify_source_revision
    kubectl create --dry-run=server -f "$manifest" -o name >/dev/null
    build_receipts+=("$(jq -n --arg component "$component" --arg target "$build_target" --arg run "$run_name" --arg image "$image_ref" \
      '{component:$component,build_target:$target,pipeline_run:$run,image_url:$image,preflight:"server_dry_run_passed",image_push:false}')")
    return
  fi

  # Re-check the public branch immediately before each new remote build request.
  verify_source_revision
  if ! run_json="$(kubectl get "pipelinerun/${run_name}" -o json --ignore-not-found=true 2>/dev/null)"; then
    echo "could not safely reconcile PipelineRun ${PIPELINE_NAMESPACE}/${run_name}" >&2
    return 1
  fi
  if [[ -n "$run_json" && "$(jq -r '.kind // empty' <<<"$run_json")" == "PipelineRun" ]]; then
    validate_existing_run "$run_json" "$component" "$build_target" "$image_component" "$image_ref" "$dockerfile"
  else
    # Server-side dry-run validates the exact admitted object without creating it.
    kubectl create --dry-run=server -f "$manifest" -o name >/dev/null
    if ! kubectl create -f "$manifest" -o name >/dev/null; then
      # A lost create response is reconciled by deterministic name; never replay it.
      if ! run_json="$(kubectl get "pipelinerun/${run_name}" -o json --ignore-not-found=true 2>/dev/null)" || \
        [[ -z "$run_json" || "$(jq -r '.kind // empty' <<<"$run_json")" != "PipelineRun" ]]; then
        echo "PipelineRun create outcome is uncertain and the deterministic name is not visible; not retrying" >&2
        return 1
      fi
      validate_existing_run "$run_json" "$component" "$build_target" "$image_component" "$image_ref" "$dockerfile"
    fi
    run_json="$(kubectl get "pipelinerun/${run_name}" -o json)"
  fi

  condition="$(jq -r '[.status.conditions[]? | select(.type == "Succeeded")][0].status // empty' <<<"$run_json")"
  if [[ "$condition" != "True" && "$condition" != "False" ]]; then
    if ! wait_output="$(kubectl wait --for=condition=Succeeded=True "pipelinerun/${run_name}" \
      --timeout="${BUILD_TIMEOUT_SECONDS}s" 2>&1)"; then
      run_json="$(kubectl get "pipelinerun/${run_name}" -o json)"
      condition="$(jq -r '[.status.conditions[]? | select(.type == "Succeeded")][0].status // empty' <<<"$run_json")"
      if [[ "$condition" != "True" ]]; then
        echo "PipelineRun ${run_name} did not complete successfully; retained for inspection, no retry or rollout was triggered" >&2
        return 1
      fi
    fi
  fi
  run_json="$(kubectl get "pipelinerun/${run_name}" -o json)"
  condition="$(jq -r '[.status.conditions[]? | select(.type == "Succeeded")][0].status // empty' <<<"$run_json")"
  if [[ "$condition" != "True" ]]; then
    echo "PipelineRun ${run_name} is not successful; retained for inspection, no retry or rollout was triggered" >&2
    return 1
  fi

  fetch_taskrun="$(jq -er '[.status.childReferences[]? | select(.kind == "TaskRun" and .pipelineTaskName == "fetch-source") | .name] | if length == 1 then .[0] else error end' <<<"$run_json")"
  build_taskrun="$(jq -er '[.status.childReferences[]? | select(.kind == "TaskRun" and .pipelineTaskName == "build-push") | .name] | if length == 1 then .[0] else error end' <<<"$run_json")"
  fetch_taskrun_json="$(kubectl get "taskrun/${fetch_taskrun}" -o json)"
  build_taskrun_json="$(kubectl get "taskrun/${build_taskrun}" -o json)"
  jq -e --arg service_account "$BUILD_SERVICE_ACCOUNT" '.spec.serviceAccountName == $service_account' \
    <<<"$fetch_taskrun_json" >/dev/null || {
    echo "fetch-source TaskRun did not use the no-token build identity" >&2
    return 1
  }
  jq -e --arg service_account "$BUILD_SERVICE_ACCOUNT" '.spec.serviceAccountName == $service_account' \
    <<<"$build_taskrun_json" >/dev/null || {
    echo "build-push TaskRun did not use the no-token build identity" >&2
    return 1
  }
  source_commit="$(jq -er '.status.results[] | select(.name == "commit") | .value' <<<"$fetch_taskrun_json")"
  image_url="$(jq -er '.status.results[] | select(.name == "IMAGE_URL") | .value' <<<"$build_taskrun_json")"
  digest="$(jq -er '.status.results[] | select(.name == "IMAGE_DIGEST") | .value' <<<"$build_taskrun_json")"
  [[ "$source_commit" == "$REVISION" ]] || {
    echo "Tekton cloned ${source_commit}, not the requested source revision ${REVISION}" >&2
    return 1
  }
  [[ "$image_url" == "$image_ref" ]] || {
    echo "Tekton reported an unexpected image URL for ${component}" >&2
    return 1
  }
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "Tekton did not return a valid immutable image digest for ${component}" >&2
    return 1
  }

  build_receipts+=("$(jq -n \
    --arg component "$component" \
    --arg target "$build_target" \
    --arg context "$CLUSTER_CONTEXT" \
    --arg namespace "$PIPELINE_NAMESPACE" \
    --arg pipeline "$PIPELINE_NAME" \
    --arg run "$run_name" \
    --arg source "$source_commit" \
    --arg url "$image_url" \
    --arg digest "$digest" \
    '{component:$component,build_target:$target,cluster_context:$context,namespace:$namespace,pipeline:$pipeline,
      pipeline_run:$run,source_revision:$source,image_url:$url,digest:$digest,
      immutable_ref:($url|split(":git-")[0])+"@"+$digest,build_executor:"in-cluster BuildKit",
      rollout:"not_requested",registry_manifest_independently_verified:false}')")
  if [[ "$image_component" == "codex-host-bundle" ]]; then
    NATIVE_BUNDLE_DIGEST="$digest"
    NATIVE_BUNDLE_IMAGE="$image_url"
  fi
}

package_native_bundle() {
  local immutable_image fetch_output bundle_dir package_output
  immutable_image="${NATIVE_BUNDLE_IMAGE%:git-*}@${NATIVE_BUNDLE_DIGEST}"
  [[ "$NATIVE_BUNDLE_DIGEST" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "cluster bundle build did not return an immutable digest" >&2
    return 1
  }
  [[ "$immutable_image" == "registry.lucas.engineering/pharness-codex-host-bundle@${NATIVE_BUNDLE_DIGEST}" ]] || {
    echo "cluster bundle build returned an unexpected immutable image reference" >&2
    return 1
  }
  bundle_dir="${TEMP_ROOT}/bundle-oci-extract"
  mkdir -p "$bundle_dir"
  fetch_output="$(python3 "${SCRIPT_DIR}/pharness-fetch-oci-bundle.py" \
    --image "$immutable_image" \
    --revision "$REVISION" \
    --output-dir "$bundle_dir")"
  bundle_dir="$(jq -er '.bundle_dir' <<<"$fetch_output")"
  package_output="$("${SCRIPT_DIR}/pharness-package-codex-host.sh" \
    --revision "$REVISION" \
    --bundle-dir "$bundle_dir")"
  NATIVE_BUNDLE_ARCHIVE="$(jq -er '.archive_path' <<<"$package_output")"
  NATIVE_BUNDLE_ARCHIVE_SHA256="$(jq -er '.sha256' <<<"$package_output")"
}

verify_source_revision
check_cluster_builder

for component in "${components[@]}"; do
  if [[ "$component" == "codex-host" ]]; then
    run_component "$component" "runtime" "codex-host"
    run_component "$component" "bundle" "codex-host-bundle"
  else
    run_component "$component" "" "$component"
  fi
done

if [[ "$PREFLIGHT_ONLY" != true && "$needs_bundle" == true ]]; then
  package_native_bundle ""
fi

jq -n \
  --arg source "$REVISION" \
  --arg context "$CLUSTER_CONTEXT" \
  --arg pipeline "$PIPELINE_NAME" \
  --argjson receipts "$(printf '%s\n' "${build_receipts[@]}" | jq -s '.')" \
  --argjson bundle_built "$(if [[ "$needs_bundle" == true && "$PREFLIGHT_ONLY" != true ]]; then echo true; else echo false; fi)" \
  --arg bundle_digest "$NATIVE_BUNDLE_DIGEST" \
  --arg bundle_archive "$NATIVE_BUNDLE_ARCHIVE" \
  --arg bundle_archive_sha256 "$NATIVE_BUNDLE_ARCHIVE_SHA256" \
  '{status:(if all($receipts[]; .preflight == "server_dry_run_passed") then "preflight_passed" else "builds_completed" end),
    source_revision:$source,cluster_context:$context,pipeline:$pipeline,components:$receipts,
    native_bundle_built:$bundle_built,native_bundle:{image_digest:(if $bundle_built then $bundle_digest else null end),
      archive:(if $bundle_built then $bundle_archive else null end),archive_sha256:(if $bundle_built then $bundle_archive_sha256 else null end)},
    production_rollout:false,registry_manifest_independently_verified:false}'
