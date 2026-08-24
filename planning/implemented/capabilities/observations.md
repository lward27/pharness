# Decisions

- Add `Observation` as a durable run-scoped record for successful read-only cluster, Tekton, Argo, Prometheus, and Loki tool events.
- Observations are derived from `tool.finished` events and point back to the same persisted artifact id when an artifact is created.
- Keep the V1 observation shape intentionally small: source, kind, subject, summary, normalized resource identity, optional resource ref JSON, optional artifact id, compact data JSON, and observed time.
- Expose observations through `GET /api/runs/:run_id/observations`, `GET /api/observations`, `GET /api/observations/:observation_id`, `pharness-cli observations list`, and `pharness-cli observations get`.
- Treat `GET /api/observations` as the V1 machine-facing observation index. It supports optional `run_id`, `source`, `kind`, `subject`, normalized resource identity, observed-time, limit, and offset filters so operators and Codex can find recent cluster/Tekton/LGTM facts without first knowing a run id.
- Store normalized resource identity as separate `resource_namespace`, `resource_kind`, and `resource_name` columns. This is intentionally duplicated from compact JSON because SDLC automation needs stable query keys before the future `Observation` CRD exists.
- Do not create observations for local filesystem writes or shell output yet. The immediate value is indexing cluster and LGTM facts that map cleanly to the future `Observation` CRD.
- Persist successful direct read-only cluster capability results as runless control-plane observations.
  - Direct calls do not create fake runs.
  - The response returns `artifact_id` and `observation_id` when the capability result can be persisted.
  - This gives Codex and future controllers stable handles for live Tekton, Kubernetes, Argo, Prometheus, and Loki evidence.

# Backlog

- Add controller-level correlation fields after `Incident` and `RemediationPlan` define what relationship needs to be queried first.
- Add Observation-to-Incident correlation after the first incident/remediation workflow exists.
- Add a direct-evidence owner identity once API auth exists, so runless observations can distinguish `codex`, controllers, and human operators.
