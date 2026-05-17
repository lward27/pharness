# Live Smoke Results - 2026-05-15

## Scope

Validate the current machine-facing control-plane path against the running local API and a live Fireworks worker.

Focus areas:

- API health and effective worker config.
- Durable run lifecycle for a read-only task.
- Policy-gated file write approval and resume.
- Approval denial without filesystem mutation.
- Static workspace verification after the approval-resume implementation.

## Environment

- API: local `pharness-api` on `http://127.0.0.1:4777`.
- Provider: Fireworks.
- Model: `accounts/fireworks/models/kimi-k2p5`.
- Worker: local worker enabled.
- CWD input: workspace root.

## Static Verification

Commands run:

```sh
CARGO_HOME=<local-temp-cargo-home> CARGO_TARGET_DIR=target cargo fmt --all -- --check
CARGO_HOME=<local-temp-cargo-home> CARGO_TARGET_DIR=target cargo test --workspace
CARGO_HOME=<local-temp-cargo-home> CARGO_TARGET_DIR=target cargo clippy --workspace --all-targets -- -D warnings
```

Result: pass.

Observed test coverage:

- `pharness-api`: 5 tests passed.
- `pharness-cli`: 3 tests passed.
- `pharness-core`: 39 tests passed.
- `pharness-core-types`: 5 tests passed.
- `pharness-fireworks`: 12 tests passed.
- `pharness-store`: 3 tests passed.

## API Health

Checks run:

```sh
curl -sS http://127.0.0.1:4777/health
curl -sS http://127.0.0.1:4777/api/config/effective
```

Result: pass.

Observed effective worker config:

```json
{
  "worker": {
    "base_url": "https://api.fireworks.ai/inference/v1",
    "enabled": true,
    "model": "accounts/fireworks/models/kimi-k2p5",
    "provider": "fireworks"
  }
}
```

## Read-Only Run

Task: list top-level workspace files and finish with a short summary.

Run id: `run_1778886007417224000`

Result: pass.

Final status:

```json
{
  "status": "completed",
  "result": {
    "status": "completed",
    "turns": 2,
    "summary": "Workspace contains 29 entries: Cargo.lock, Cargo.toml, README.md, config/, crates/ (5 subdirs: pharness-api, pharness-cli, pharness-core, pharness-fireworks, pharness-store), docs/adr/, planning/ (3 .md files), prompt.md, rust-toolchain.toml, and target/ (with debug builds and SQLite databases). This is a Rust workspace for the pharness project with a multi-crate architecture."
  }
}
```

Event sequence:

1. `run.queued`
2. `run.started`
3. `model.request_started`
4. `model.response_finished`
5. `action.proposed` with `list_dir`
6. `policy.evaluated` with `allow`
7. `tool.started`
8. `tool.finished`
9. `model.request_started`
10. `model.response_finished`
11. `action.proposed` with `finish`
12. `run.finished`

Conclusion: the read-only machine-facing run path is healthy.

## Write Approval And Resume

Task: use `write_file` to create a harmless smoke-test file, then finish.

Run id: `run_1778886036624108000`

Approval id: `appr_run_1778886036624108000_1778886044417753000`

Target file: `pharness-live-write-smoke-1778886036.txt`

Result: pass.

Before approval:

```json
{
  "status": "approval_required",
  "result": {
    "approval_id": "appr_run_1778886036624108000_1778886044417753000",
    "status": "approval_required",
    "summary": "file write requires approval: pharness-live-write-smoke-1778886036.txt",
    "turns": 1
  }
}
```

The model proposed the concrete gated action:

```json
{
  "action": "write_file",
  "path": "pharness-live-write-smoke-1778886036.txt",
  "content": "pharness live approval smoke test"
}
```

After approval:

```json
{
  "status": "completed",
  "result": {
    "status": "completed",
    "turns": 2,
    "summary": "Created pharness-live-write-smoke-1778886036.txt containing 'pharness live approval smoke test' (33 bytes)."
  }
}
```

Event sequence after approval:

1. `action.proposed` with `write_file`
2. `policy.evaluated` with `ask`
3. `approval.required`
4. `approval.decided` with `approved`
5. `run.resumed`
6. `tool.started` with `write_file`
7. `tool.finished`
8. `model.request_started`
9. `model.response_finished`
10. `action.proposed` with `finish`
11. `run.finished`

File content:

```text
pharness live approval smoke test
```

Conclusion: the approval-resume bug from the prior smoke test is fixed. The model no longer calls `request_approval`; it calls `write_file`, policy creates the approval gate, and the API resumes the reviewed action.

## Approval Denial

Task: use `write_file` to create a harmless smoke-test file, then deny the approval.

Run id: `run_1778886116598284000`

Approval id: `appr_run_1778886116598284000_1778886128236578000`

Target file: `pharness-live-deny-smoke-1778886116.txt`

Result: pass.

Before denial:

```json
{
  "status": "approval_required",
  "result": {
    "approval_id": "appr_run_1778886116598284000_1778886128236578000",
    "status": "approval_required",
    "summary": "file write requires approval: pharness-live-deny-smoke-1778886116.txt",
    "turns": 1
  }
}
```

After denial:

```json
{
  "status": "failed",
  "result": {
    "approval_id": "appr_run_1778886116598284000_1778886128236578000",
    "error": "approval denied",
    "status": "failed",
    "summary": "file write requires approval: pharness-live-deny-smoke-1778886116.txt",
    "turns": 1
  }
}
```

The target file was absent after denial.

Conclusion: denial is enforcing the policy boundary. It does not execute the reviewed action.

## Notes

- One ad hoc shell script used a variable name that conflicts with zsh's readonly `status` parameter. The product behavior was unaffected, but future smoke scripts should avoid that name.
- The workspace path used in submitted runs was supplied from the shell with `$PWD`; no hard-coded local absolute path is required for the smoke flow.
- The write smoke file was intentionally left in place as evidence of the approved write path.

## Next Review Items

- Add CLI approval commands so this flow does not require raw `curl`.
- Add durable diff/file artifact retrieval for write and patch operations.
- Add SSE event streaming for low-friction run observation.
- Decide whether denied approvals should leave run status as `failed` or move to a distinct terminal status such as `denied`.
