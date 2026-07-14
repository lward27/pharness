# Executor PipelineRunAnalysis

## Decisions

- A terminal Tekton executor callback now carries a normalized
  `PipelineRunAnalysis` for the exact PipelineRun it created. The API remains
  the only SQLite writer: it persists the typed artifact and observation, then
  attaches the observation as PipelineIntent evidence.
- The executor collects only its PipelineRun and related TaskRuns. It does not
  inherit the broad observer service account or probe deployments, Argo CD,
  registry credentials, logs, or secrets.
- A successful PipelineRun remains a successful execution even when bounded
  analysis collection fails. Pharness preserves the compact receipt, records a
  dedicated audit event, and leaves deployment evidence unsatisfied rather
  than incorrectly turning an observation failure into a build failure.
- PipelineRun and TaskRun RBAC is namespaced and attached only to the dedicated
  executor service account. The normal model worker remains read-only.
- The UI distinguishes the executor receipt from a typed analysis. It labels
  the flow node `PipelineRunAnalysis` only after the observation is attached.

## Backlog

- Add a separately approved enrichment path for deployment, Argo CD, registry,
  and observability correlation. Do not give the executor those reads.
- Add analysis schema versioning before storing richer task outputs or
  cross-system evidence.
- Add a retention workflow for disposable PipelineRuns and their analysis
  artifacts after export and audit retention policy are explicit.
