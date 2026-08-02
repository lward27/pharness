# Pipeline Contracts

## Decisions

- Add a durable PipelineContract record as operator-managed policy data. It is
  not proposed by an agent and cannot be inferred from an untrusted intent.
- An active contract is keyed by namespace, PipelineRef, and version. Preflight
  requires exactly one active contract for the intended namespace and pipeline.
- Contracts declare allowed parameters (`scalar` or `array`) and workspace
  bindings (`persistent_volume_claim` or `volume_claim_template`), including
  required inputs. Unknown, missing, or wrongly shaped intent inputs block the
  preview and apply path.
- A WorkItem delivery contract must use `source_revision_param` to identify a
  required scalar input. Preflight binds that input to the separately observed
  GitHub merge SHA, preventing a mutable branch from becoming a build source.
- Expose contracts through `GET`/`POST /api/pipeline-contracts` and
  `pharness-cli pipeline-contracts`. Contract creation is audited as an
  operator action.
- Retire contracts through the explicit `active -> retired` transition. The
  record and audit trail remain durable; retirement blocks preflight until a
  single replacement contract becomes active.
- Replace an active contract through one SQLite transaction. Replacement retires
  the old version and activates the new version together, avoiding either an
  authorization gap or two active versions. Both records receive audit events.
- The deterministic smoke creates a minimal contract for the test PipelineRef
  before requiring a ready execution preview.
- A completed terminal PipelineRunAnalysis now yields a compact
  `pipeline_build_output` artifact when Tekton reports a safe `IMAGE_URL` and
  valid `IMAGE_DIGEST`. Pharness composes the immutable image reference and
  records the reported source commit. For WorkItem delivery, a reported commit
  that differs from the observed GitHub merge is stored as `untrusted`; it
  cannot be used for GitOps planning.

## Backlog

- Add optimistic concurrency or a revision precondition to replacement so an
  operator can detect a concurrent policy edit before retiring the current
  contract.
- Import contracts from the observed Tekton Pipeline spec, but retain explicit
  operator approval before activating an imported contract.
- Add explicit per-contract image-result names, timeout, and richer provenance
  expectations once multiple build styles need to be supported. The current
  typed default consumes Tekton's conventional `IMAGE_URL`, `IMAGE_DIGEST`,
  and optional `commit` outputs only.
