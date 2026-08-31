#!/usr/bin/env bash
set -euo pipefail

# Build and push one immutable PHarness revision from a clean local worktree.
# The registry targets and linux/amd64 platform are fixed intentionally. The
# resulting digests must still be reviewed and committed through GitOps.
#
# Usage:
#   scripts/pharness-build-local.sh <runtime|ui|python-runner|node-runner|model-gateway|eval-runner|codex-host|all> \
#     --revision <40-char-sha> [--builder <buildx-builder>] [--preflight-only]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel)"
TARGET="${1:-}"
REVISION=""
BUILDER="${PHARNESS_BUILDX_BUILDER:-lucas-desktop}"
PREFLIGHT_ONLY=false
PLATFORM="linux/amd64"
REGISTRY="registry.lucas.engineering"

usage() {
  echo "Usage: $0 <runtime|ui|python-runner|node-runner|model-gateway|eval-runner|codex-host|all> --revision <40-char-sha> [--builder lucas-desktop] [--preflight-only]" >&2
  exit 2
}

[[ "$TARGET" =~ ^(runtime|ui|python-runner|node-runner|model-gateway|eval-runner|codex-host|all)$ ]] || usage
shift
while [[ $# -gt 0 ]]; do
  case "$1" in
    --revision) REVISION="${2:-}"; shift 2 ;;
    --builder) BUILDER="${2:-}"; shift 2 ;;
    --preflight-only) PREFLIGHT_ONLY=true; shift ;;
    *) usage ;;
  esac
done

[[ "$REVISION" =~ ^[0-9a-f]{40}$ ]] || {
  echo "--revision must be a full lowercase 40-character Git SHA" >&2
  exit 1
}
[[ "$BUILDER" =~ ^[A-Za-z0-9._-]+$ ]] || {
  echo "--builder must be a normalized Docker buildx builder name" >&2
  exit 1
}
[[ "$BUILDER" == "lucas-desktop" ]] || {
  echo "PHarness release builds are pinned to the dedicated lucas-desktop builder; no automatic fallback is permitted" >&2
  exit 1
}

VERIFICATION_OUTPUT="$(
  "${SCRIPT_DIR}/pharness-verify-build-revision.sh" \
    --repo "$REPOSITORY_ROOT" \
    --remote "${PHARNESS_BUILD_REMOTE:-origin}" \
    --branch "${PHARNESS_BUILD_BRANCH:-main}" \
    --revision "$REVISION"
)"
printf '%s\n' "$VERIFICATION_OUTPUT"
VERIFIED_REVISION="$(awk -F= '$1 == "verified_revision" { print $2 }' <<<"$VERIFICATION_OUTPUT")"
[[ "$VERIFIED_REVISION" == "$REVISION" ]] || {
  echo "build preflight did not return the requested immutable revision" >&2
  exit 1
}
REVISION="$VERIFIED_REVISION"

docker version >/dev/null
BUILDER_INSPECTION="$(docker buildx inspect "$BUILDER" --bootstrap)"
grep -Eq 'Platforms:.*(^|, | )linux/amd64([, ]|$)' <<<"$BUILDER_INSPECTION" || {
  echo "buildx builder ${BUILDER} does not advertise ${PLATFORM}" >&2
  exit 1
}

components=()
case "$TARGET" in
  all) components=(runtime ui python-runner node-runner model-gateway eval-runner codex-host) ;;
  *) components=("$TARGET") ;;
esac

if [[ "$PREFLIGHT_ONLY" == true ]]; then
  jq -n \
    --arg revision "$REVISION" \
    --arg builder "$BUILDER" \
    --arg platform "$PLATFORM" \
    --argjson components "$(printf '%s\n' "${components[@]}" | jq -R . | jq -s .)" \
    '{preflight:"passed",revision:$revision,builder:$builder,platform:$platform,components:$components,external_mutations:[]}'
  exit 0
fi

TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/pharness-local-build.XXXXXX")"
cleanup() {
  if [[ -n "${TEMP_ROOT:-}" && -d "$TEMP_ROOT" && "$TEMP_ROOT" == */pharness-local-build.* ]]; then
    rm -rf -- "$TEMP_ROOT"
  fi
}
trap cleanup EXIT

build_component() {
  local component="$1"
  local dockerfile=""
  local image_repository="${REGISTRY}/pharness-${component}"
  local image_tag="${image_repository}:git-${REVISION}"
  local metadata_file="${TEMP_ROOT}/${component}.json"
  local digest=""
  local immutable_reference=""
  local manifest_json=""
  local image_json=""
  local -a target_args=()

  case "$component" in
    runtime) dockerfile="deploy/docker/Dockerfile.runtime" ;;
    ui) dockerfile="deploy/docker/Dockerfile.ui" ;;
    python-runner) dockerfile="deploy/docker/Dockerfile.python-runner" ;;
    node-runner) dockerfile="deploy/docker/Dockerfile.node-runner" ;;
    model-gateway) dockerfile="deploy/docker/Dockerfile.model-gateway" ;;
    eval-runner) dockerfile="deploy/docker/Dockerfile.eval-runner" ;;
    codex-host)
      dockerfile="deploy/docker/Dockerfile.codex-host"
      target_args=(--target runtime)
      ;;
    *) echo "unsupported PHarness component ${component}" >&2; return 1 ;;
  esac

  docker buildx build \
    --builder "$BUILDER" \
    --platform "$PLATFORM" \
    --pull \
    --push \
    --provenance=false \
    --sbom=false \
    --file "${REPOSITORY_ROOT}/${dockerfile}" \
    "${target_args[@]}" \
    --build-arg "PHARNESS_BUILD_REVISION=${REVISION}" \
    --build-arg TARGETARCH=amd64 \
    --tag "$image_tag" \
    --metadata-file "$metadata_file" \
    "$REPOSITORY_ROOT"

  digest="$(jq -r '."containerimage.digest" // empty' "$metadata_file")"
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "local build did not return an immutable digest for ${component}" >&2
    return 1
  }
  immutable_reference="${image_repository}@${digest}"
  manifest_json="$(docker buildx imagetools inspect "$immutable_reference" --format '{{json .Manifest}}')"
  image_json="$(docker buildx imagetools inspect "$immutable_reference" --format '{{json .Image}}')"

  [[ "$(jq -r '.digest // empty' <<<"$manifest_json")" == "$digest" ]] || {
    echo "registry manifest digest does not match the ${component} build result" >&2
    return 1
  }
  [[ "$(jq -r '.os // empty' <<<"$image_json")" == "linux" ]] || {
    echo "${component} image OS is not linux" >&2
    return 1
  }
  [[ "$(jq -r '.architecture // empty' <<<"$image_json")" == "amd64" ]] || {
    echo "${component} image architecture is not amd64" >&2
    return 1
  }
  [[ "$(jq -r '.config.Labels["org.opencontainers.image.revision"] // empty' <<<"$image_json")" == "$REVISION" ]] || {
    echo "${component} OCI revision label does not match the verified source SHA" >&2
    return 1
  }
  [[ "$(jq -r '.config.Labels["org.opencontainers.image.source"] // empty' <<<"$image_json")" == "https://github.com/lward27/pharness" ]] || {
    echo "${component} OCI source label is missing or incorrect" >&2
    return 1
  }

  jq -n \
    --arg component "$component" \
    --arg revision "$REVISION" \
    --arg builder "$BUILDER" \
    --arg platform "$PLATFORM" \
    --arg image_url "$image_repository" \
    --arg digest "$digest" \
    '{component:$component,revision:$revision,builder:$builder,platform:$platform,image_url:$image_url,digest:$digest,immutable_ref:($image_url+"@"+$digest),oci_source:"https://github.com/lward27/pharness",sbom_verified:false,signature_verified:false,provenance_verified:false}'
}

for component in "${components[@]}"; do
  build_component "$component"
done

if [[ "$TARGET" == "all" || "$TARGET" == "codex-host" ]]; then
  "${SCRIPT_DIR}/pharness-package-codex-host.sh" \
    --revision "$REVISION" \
    --builder "$BUILDER"
fi
