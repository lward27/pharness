# Decisions

- Add Tekton visibility as a typed read-only capability named `tekton_get_pipeline_runs`.
- Add TaskRun visibility as a typed read-only capability named `tekton_get_task_runs`.
- Add normalized PipelineRun analysis as a typed read-only capability named `tekton_analyze_pipeline_run`.
- Back the capability with the Kubernetes API through `kubectl get pipelineruns.tekton.dev -o json`, not the `tkn` CLI. The control-plane contract should be Kubernetes-native first because V2/V3 will run inside the cluster.
- Back TaskRun reads with `kubectl get taskruns.tekton.dev -o json` through the same typed execution path.
- Back PipelineRun analysis by fetching one PipelineRun, its related TaskRuns with label `tekton.dev/pipelineRun=<name>`, the declared Deployment target, and the related Argo CD Application when the Deployment carries an Argo tracking annotation.
- Keep the first Tekton surface intentionally narrow: PipelineRun and TaskRun reads only, with optional namespace, name, all-namespaces, and label-selector fields.
- Keep the first analysis shape intentionally narrow but SDLC-useful: PipelineRun status, reason, timing, TaskRun status counts, compact per-TaskRun status, build outputs, Deployment rollout status, image alignment, and Argo sync/health.
- Do not expose Tekton start/cancel/rerun actions yet. Those are production-impacting workflow controls and belong behind explicit approval gates after the read path is stable.
- Persist Tekton tool results as artifacts with `tekton_tool_result` so future Codex callers can retrieve SDLC evidence without scraping event payloads.
- Persist normalized PipelineRun analysis as `pipeline_run_analysis` artifacts instead of generic Tekton tool results.
- The live cluster has the `pipelineruns.tekton.dev` CRD installed. Initial inventory was empty, then two user-triggered PipelineRuns became available for analysis smoke tests.
- Direct capability smoke passed: `tekton_get_pipeline_runs` returned `status: ok`, policy `allow`, `executed: true`, and `item_count: 0`.
- Direct capability smoke passed: `tekton_get_task_runs` returned `status: ok`, policy `allow`, `executed: true`, and `item_count: 0`.
- Direct analysis smoke passed for control flow: a missing PipelineRun returned `status: tool_error` after an allowed read-only policy decision.
- Live analysis smoke passed for `finance-frontend-run-6mwcl`: status `succeeded`, 3 succeeded TaskRuns, commit captured, image digest captured, deployment target captured, Deployment rollout observed as healthy, and Argo app correlation available.
- Live analysis smoke passed for `finance-app-db-service-run-jkx6k`: status moved from `running` during first analysis to `succeeded` after completion, 3 succeeded TaskRuns, commit captured, image digest captured, Deployment rollout observed as healthy, and Argo app correlation available.
- Both live PipelineRun analyses currently report `image_alignment.status: mismatch` because the Tekton output image uses the in-cluster registry host while the Deployment image uses the external registry host. Treat that as evidence worth surfacing, not a pharness failure.
- Direct local API smoke passed after Argo correlation was added: both Finance PipelineRuns returned `status: ok`, `executed: true`, `summary.status: succeeded`, `deployment.status: healthy`, `argo_sync_status: Synced`, and `argo_health_status: Healthy`.
- Secret-shaped analysis smoke passed: name `token-build` returned `status: denied`, `executed: false`, and did not call the tool.
- Secret-shaped direct smoke passed: namespace `token-store` returned `status: denied`, `executed: false`, and did not call the tool.

# Backlog

- Add compact Tekton fields only when dogfooding shows they are needed. Current analysis includes status, reason, timing, task identity, pod name, commit, image URL/digest, deployment target, Deployment rollout, image alignment, and Argo sync/health.
- Extend `PipelineRunAnalysis` once pharness can safely combine bounded logs and Prometheus signals.
- Add mutating `PipelineIntent` execution later as a separate approved capability. It should never be hidden inside `run_shell`.
- Consider a Kubernetes service-account mode for in-cluster execution so V2 can stop depending on a local kubeconfig.
