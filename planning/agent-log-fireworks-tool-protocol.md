# Decisions

- The API worker now defaults to native Fireworks tool calls for agent actions. The smoke test reached Fireworks but failed because JSON action mode accepted a malformed object without an `action` field.
- The default worker tool surface includes terminal actions, read-only filesystem tools, policy-gated shell/git tools, and read-only Kubernetes, Argo CD, and Prometheus tools.
- `write_file` and `patch_file` are now exposed in the default worker schema behind policy approval.
- `request_approval` is not exposed in the default Fireworks native tool schema because it creates non-resumable approvals when the model calls it instead of the concrete gated action.
- Fireworks native tool requests use required tool choice. With `respond` and `finish` exposed as tools, every model turn can remain a single typed action.
- The 2026-05-15 Fireworks smoke test with `accounts/fireworks/models/kimi-k2p5` completed successfully in two turns: `list_dir` followed by `finish`.
- The API worker default Fireworks model is `accounts/fireworks/models/kimi-k2p5`; `PHARNESS_FIREWORKS_MODEL` remains the explicit override. A live run with `.zshrc` sourced and no model override completed successfully with this default.
- Approval resume now persists the exact reviewed action and transcript, exposes `POST /api/runs/:id/approvals`, emits `approval.decided` and `run.resumed`, and resumes by executing the stored action payload.
- The live approval smoke test passed with a real Fireworks run: `write_file` paused at policy, approval resumed the exact reviewed action, the file was written, the model finished, and denial left its target file absent.
- API access logging is enabled with `tracing` and `tower-http`; `pharness-cli run --follow-events` tails durable run events to stderr while preserving final JSON on stdout.
- `GET /api/approvals` and `pharness-cli approvals list|approve|deny` exist. Approval decisions remain run-scoped for now and produce machine-readable JSON.
- `GET /api/runs/:id/events/stream` streams durable events as SSE with `Last-Event-ID` replay. The implementation intentionally polls SQLite first instead of introducing a broadcast bus.
- `write_file` tool results now persist file diffs into `file_changes`, and `GET /api/runs/:id/diff` returns both structured changes and combined diff text.
- `patch_file` uses a structured exact replacement payload and persists diffs through the same file-change path as `write_file`.
- Live patch smoke passed with Fireworks: model proposed `patch_file`, policy paused for file-write approval, approval resumed the reviewed patch action, and diff retrieval showed one stored change.
- The first live Kubernetes dogfood run proved local pharness can read the `lucas_engineering` cluster through typed `kubernetes_get`; pharness does not need to run inside the cluster for the first dogfood milestone.
- Cluster read tool output now parses JSON before redaction, compacts Kubernetes resources, and avoids local absolute executable paths in command summaries.
- `argo_get_app` now reads Argo CD Application CRDs through `kubectl` instead of requiring the local `argocd` CLI.
- `POST /api/capabilities/execute` and `pharness-cli capabilities` now provide a model-free smoke path for typed read-only Kubernetes, Argo, and Prometheus actions.
- Direct capability execution returns policy and execution state as structured JSON: allowed actions execute, denied actions do not execute, and tool failures return `status: tool_error` instead of HTTP 500.
- Prometheus live success dogfood uses a local loopback URL created through `kubectl port-forward` to `service/prometheus-server` in `monitoring`; this keeps V1 local while avoiding cluster mutation.
- Prometheus responses are compacted before entering events/model context: `result_count`, `results_truncated`, and bounded sample results.
- Cluster and observability tool results are now persisted as artifacts and exposed through `GET /api/runs/:id/artifacts` and `GET /api/artifacts/:id`.

# Backlog

- Add a fallback model protocol setting only if a chosen Fireworks model rejects native required tool calls.
- Add a real-time event fanout only if SQLite polling becomes a measured bottleneck.
- Add general artifact retrieval after the diff path is stable.
- Add CLI artifact list/get commands only if operator workflow needs them; the machine-facing API is already enough for Codex.
- Add patch preview only if operator review needs generated diff before approval.
