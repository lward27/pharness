#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REVISION=0000000000000000000000000000000000000000

# Invalid selections must fail before Docker, Git mutation, or a build starts.
for script in pharness-build-local.sh pharness-build.sh pharness-package-codex-host.sh; do
  args=(--revision "$REVISION")
  [[ "$script" == pharness-package-codex-host.sh ]] || args=(ui --revision "$REVISION")
  for builder in '' '--other-option' 'host/with/path' 'host with spaces'; do
    if output="$(PHARNESS_BUILDX_BUILDER= bash "${SCRIPT_DIR}/../${script}" "${args[@]}" --builder "$builder" 2>&1)"; then
      echo "$script accepted an invalid builder" >&2; exit 1
    fi
    grep -Eq 'normalized|explicit|Explicit' <<<"$output"
  done
done

# All release paths propagate the selected worker and keep the fixed platform.
for script in pharness-build-local.sh pharness-package-codex-host.sh; do
  grep -Fq -- '--builder "$BUILDER"' "${SCRIPT_DIR}/../${script}"
  grep -Fq 'linux/amd64' "${SCRIPT_DIR}/../${script}"
  if grep -Eq 'BUILDER.*!=.*lucas-desktop|--builder lucas-desktop' "${SCRIPT_DIR}/../${script}"; then
    echo "$script still hardcodes a physical builder" >&2; exit 1
  fi
done
grep -Fq 'Dockerfile.platform-check' "${SCRIPT_DIR}/../pharness-build-local.sh"
echo "Explicit builder selection and fixed platform checks passed"
