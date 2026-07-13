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

## Backlog

- Add optimistic concurrency or a revision precondition to replacement so an
  operator can detect a concurrent policy edit before retiring the current
  contract.
- Import contracts from the observed Tekton Pipeline spec, but retain explicit
  operator approval before activating an imported contract.
- Add image, result, timeout, and provenance expectations to the contract once
  PipelineRun analysis feeds the full release workflow.
