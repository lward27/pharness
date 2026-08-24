# Decisions

- Dogfooding can start from the local runtime against the `lucas_engineering` cluster through `kubectl`; pharness does not need to run inside the cluster until local typed reads, event durability, approvals, and structured results are stable.
- The first live Kubernetes dogfood run completed through pharness with `kubernetes_get` against pods in the `argocd` namespace. Policy allowed the action as a low-risk typed read-only cluster capability, and the run completed in two turns.
- The initial pod read exposed a context and event-log problem: full pod JSON was persisted as a huge redacted text blob because stdout redaction ran before JSON parsing. Cluster tool output now parses JSON first, redacts structurally, and stores compact Kubernetes resource summaries.
- Command summaries now use the executable name instead of a local absolute executable path. This keeps persisted event payloads portable and avoids leaking local machine paths.
- `argo_get_app` now reads Argo CD Application CRDs through `kubectl` in the configured Argo namespace instead of requiring the `argocd` CLI. That is the right V1 default because Argo Application status is already present in the cluster API and the local machine does not have the Argo CLI installed.
- The live Argo dogfood run completed through pharness with `argo_get_app` for the `ghost` Application. The result was `Synced` and `Healthy`, with no mutation and no secret reads.
- The compact cluster output shape is now part of the control-plane contract: list responses expose `item_count` plus compact resource summaries, not raw Kubernetes object dumps.
- Direct capability execution is now exposed through `POST /api/capabilities/execute` for read-only Kubernetes, Argo, and Prometheus actions. This gives Codex and smoke tests a model-free path for checking typed capabilities.
- The CLI now exposes direct capability smoke commands under `pharness-cli capabilities`.
- Direct capability live smoke passed for `kubernetes_get` pods in the `argocd` namespace and `argo_get_app` for the `ghost` Application.
- Direct capability policy smoke passed for `kubernetes_get` on `secrets`: the API returned `status: denied`, `executed: false`, and did not call `kubectl`.
- Direct Prometheus capability smoke returned structured `tool_error` because `PHARNESS_PROMETHEUS_URL` is not configured. That is acceptable until a read-only Prometheus URL is provided.
- `PHARNESS_PROMETHEUS_URL` was created locally with a non-mutating port-forward to `service/prometheus-server` in the `monitoring` namespace.
- Direct Prometheus success smoke passed with `prometheus_query` for `up`: policy allowed execution, Prometheus returned `status: success`, `result_count: 33`, and the pharness response kept only bounded samples.
- Secret-shaped Prometheus query smoke passed with `kube_secret_info`: policy denied the request and did not execute the query.
- Model-backed Prometheus dogfood passed in two turns: the agent called `prometheus_query`, policy allowed it, the tool returned a compact response, and the model finished with the correct 33-series summary.
- Cluster and observability tool results are now persisted as artifacts. The Prometheus dogfood run produced one `prometheus_tool_result` artifact retrievable through the run artifacts endpoint and the single-artifact endpoint.
- Tekton should enter pharness as a typed read-only SDLC capability before any pipeline mutation is exposed. The first implemented surface is `tekton_get_pipeline_runs`, backed by the PipelineRun CRD through `kubectl`.
- TaskRun inventory is now exposed as `tekton_get_task_runs`, backed by the TaskRun CRD through the same typed read-only path.
- PipelineRun analysis is now exposed as `tekton_analyze_pipeline_run`, which reads one PipelineRun and related TaskRuns and returns a normalized `PipelineRunAnalysis` summary.
- PipelineRun analysis now correlates the build to the declared Deployment and the Argo CD Application linked by the Deployment tracking annotation.
- Live PipelineRun analysis now captures useful SDLC evidence available through read-only APIs: repo URL, image reference, deployment target, commit SHA, image digest/image URL, task identity, pod name, per-task status, rollout health, image alignment, and Argo sync/health.
- The live cluster has the `pipelineruns.tekton.dev` CRD installed. Empty-list handling was proven first, then two user-triggered PipelineRuns were used for real analysis smokes.
- The first live Finance analyses surfaced image alignment mismatches because Tekton reports the in-cluster registry hostname while Deployments reference the external registry hostname.
- Registry aliases are now supported with `PHARNESS_REGISTRY_ALIASES`, so known internal/external registry hostnames can produce `registry_alias_match` without hiding unconfigured registry drift.

# Backlog

- Keep expanding compact Kubernetes summaries by resource kind only when dogfooding shows a real need. Avoid turning the generic read path back into a raw object dump.
- Consider adding an explicit namespace argument to `argo_get_app` later. For now, one configured Argo namespace keeps the tool surface smaller.
- Add CLI artifact commands only if API consumers need operator-facing retrieval outside `curl`/Codex.
- Extend PipelineRun analysis with bounded logs and Prometheus signals once those inputs have their own read policy.
- Move registry alias configuration into parsed config once pharness has a real config loader.
