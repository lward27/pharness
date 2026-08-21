#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
fixture_path="${repo_root}/crates/pharness-api/tests/fixtures/v3-characterization.json"
base_url="${PHARNESS_BASE_URL:-http://127.0.0.1:18081}"
operator_token="${PHARNESS_OPERATOR_TOKEN:-}"

usage() {
  cat <<'USAGE'
Usage: scripts/pharness-v3-characterization.sh

Run sanitized, read-only characterization checks against a deployed PHarness API.

Environment:
  PHARNESS_BASE_URL       API/UI origin (default: http://127.0.0.1:18081)
  PHARNESS_OPERATOR_TOKEN Optional bearer token when operator auth is enabled

The script sends GET requests only. It never decides a gate, executes an action,
creates a writer authorization, or mutates the completed V3 WorkItem.
USAGE
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi
if [[ $# -ne 0 ]]; then
  usage >&2
  exit 2
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/pharness-v3-characterization.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

fixture_value() {
  python3 - "${fixture_path}" "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    fixture = json.load(handle)
print(fixture[sys.argv[2]])
PY
}

fetch_json() {
  local label="$1"
  local path="$2"
  if [[ -n "${operator_token}" ]]; then
    curl --silent --show-error --fail-with-body \
      -H "Authorization: Bearer ${operator_token}" \
      -H 'Accept: application/json' \
      -o "${tmp_dir}/${label}.json" \
      "${base_url%/}${path}"
  else
    curl --silent --show-error --fail-with-body \
      -H 'Accept: application/json' \
      -o "${tmp_dir}/${label}.json" \
      "${base_url%/}${path}"
  fi
}

work_item_id="$(fixture_value work_item_id)"
run_id="$(fixture_value run_id)"
release_id="$(fixture_value release_id)"

fetch_json work_item_flow "/api/work-items/${work_item_id}/flow"
fetch_json operator_summary "/api/runs/${run_id}/operator-summary"
fetch_json release "/api/releases/${release_id}"
fetch_json rollback "/api/work-items/${work_item_id}/rollback-intents"
fetch_json readiness "/api/system/readiness"

python3 - "${fixture_path}" "${tmp_dir}" <<'PY'
import json
import pathlib
import sys

fixture_path = pathlib.Path(sys.argv[1])
result_dir = pathlib.Path(sys.argv[2])

def load(name):
    with (result_dir / f"{name}.json").open(encoding="utf-8") as handle:
        return json.load(handle)

with fixture_path.open(encoding="utf-8") as handle:
    expected = json.load(handle)

flow = load("work_item_flow")
summary = load("operator_summary")
release = load("release")
rollback = load("rollback")
readiness = load("readiness")

assert flow["work_item"]["id"] == expected["work_item_id"]
assert flow["work_item"]["status"] == expected["work_item_status"]
rollback_writer = [
    action
    for action in flow["action_rail"]
    if action["id"] == "execute_rollback_gitops_pr"
]
assert len(rollback_writer) == 1
assert rollback_writer[0]["status"] == "ready"

assert summary["run_id"] == expected["run_id"]
assert summary["turns"] == expected["run_turns"]
assert release["id"] == expected["release_id"]
assert release["status"] == expected["release_status"]

rollback_content = rollback["content"]
assert rollback_content["rollback_intent_id"] == expected["rollback_intent_id"]
assert rollback_content["status"] == expected["rollback_status"]
assert rollback_content["baseline"]["image_digest"] == expected["rollback_baseline_digest"]
assert not rollback_content.get("writer_execution_id")
assert not rollback_content.get("pull_request_url")

assert readiness["api_revision"] == expected["source_revision"]
assert readiness["ui_revision"] == expected["source_revision"]
assert readiness["runtime_image_digest"] == expected["runtime_digest"]
assert readiness["ui_image_digest"] == expected["ui_digest"]
assert readiness["platform_versions_match"] is True
python_profiles = [
    profile for profile in readiness["environment_profiles"] if profile["id"] == "python-3.11"
]
assert len(python_profiles) == 1
assert python_profiles[0]["image"].endswith("@" + expected["runner_digest"])

print(
    "V3 characterization passed: completed WorkItem and Release, 17-turn coding "
    "run, aligned immutable PHarness artifacts, and rollback writer readiness "
    "without rollback execution."
)
PY
