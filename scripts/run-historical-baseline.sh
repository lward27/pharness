#!/usr/bin/env bash
# Run the pinned coding suite through a detached, pre-recovery Pharness
# checkout.  The candidate prompt is copied in deliberately so the control
# and candidate reports have identical model/prompt/fixture configuration.
set -euo pipefail

usage() {
  echo "Usage: $0 --revision <git-revision> --output <report.json> [--attempts <n>]" >&2
  exit 2
}

revision=""
output=""
attempts="2"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --revision) revision="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    --attempts) attempts="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$revision" && -n "$output" ]] || usage
[[ "$attempts" =~ ^[1-9][0-9]*$ ]] || { echo "--attempts must be a positive integer" >&2; exit 2; }
[[ -n "${FIREWORKS_API_KEY:-}" ]] || { echo "FIREWORKS_API_KEY is required" >&2; exit 2; }

repo_root="$(git rev-parse --show-toplevel)"
git -C "$repo_root" rev-parse --verify --quiet "${revision}^{commit}" >/dev/null || {
  echo "Revision is not a local commit: $revision" >&2
  exit 2
}
revision_sha="$(git -C "$repo_root" rev-parse "$revision^{commit}")"
output_abs="$repo_root/$output"
if [[ "$output" = /* ]]; then output_abs="$output"; fi
mkdir -p "$(dirname "$output_abs")"

worktree="/private/tmp/pharness-historical-baseline-${revision_sha:0:12}-$$"
artifact_dir="${output_abs%.json}-artifacts"
git -C "$repo_root" worktree add --detach "$worktree" "$revision_sha" >/dev/null

cp "$repo_root/crates/pharness-runhost/src/prompt.rs" "$worktree/crates/pharness-runhost/src/prompt.rs"
perl -0pi -e 's/pub use prompt::\{system_prompt, worker_tool_specs\};/pub use prompt::{system_prompt, worker_tool_specs, SYSTEM_PROMPT_VERSION};/' "$worktree/crates/pharness-runhost/src/lib.rs"
mkdir -p "$worktree/.pharness-historical-eval/src"
cp "$repo_root/scripts/historical-baseline/Cargo.toml" "$worktree/.pharness-historical-eval/Cargo.toml"
cp "$repo_root/scripts/historical-baseline/main.rs" "$worktree/.pharness-historical-eval/src/main.rs"

echo "Historical checkout: $worktree"
echo "Writing baseline report: $output_abs"
PHARNESS_EVAL_ATTEMPTS="$attempts" \
PHARNESS_EVAL_OUTPUT="$output_abs" \
PHARNESS_EVAL_ARTIFACT_DIR="$artifact_dir" \
CARGO_TARGET_DIR="/private/tmp/pharness-historical-target-${revision_sha:0:12}" \
cargo run --quiet --manifest-path "$worktree/.pharness-historical-eval/Cargo.toml"

echo "Baseline artifacts: $artifact_dir"
echo "Historical checkout retained for audit: $worktree"
