# Decisions

- Add machine-facing SDLC readiness endpoints for WorkPlans and ChangeSets:
  - `GET /api/work-plans/:work_plan_id/readiness`
  - `GET /api/change-sets/:change_set_id/readiness`
- Add matching CLI commands:
  - `pharness-cli work-plans readiness`
  - `pharness-cli change-sets readiness`
- Define readiness as "ready for autonomous execution under a trusted envelope", not merely ready for human review.
- Treat blockers as hard stops: unapproved WorkPlan, unapproved ChangeSet, pending/stale/rejected approval gates, and missing active trusted envelope.
- Treat stale trusted envelopes as warnings when reported alongside the missing active envelope blocker. They are evidence explaining why a formerly trusted path is no longer usable.
- WorkPlan readiness warns when no ChangeSet exists because WorkPlan envelopes are broader than source-change execution.
- ChangeSet readiness now includes the current PipelineIntent when present.
- ChangeSet readiness warns on `missing_pipeline_intent`, `stale_pipeline_intent`, or `pipeline_intent_not_approved`. These are warnings in V1 because trusted-envelope execution is filesystem-only; they should become blockers before cluster-backed execution.
- ChangeSet readiness now includes the current DeploymentIntent when present.
- After a PipelineIntent is approved, ChangeSet readiness warns on `missing_deployment_intent`, `stale_deployment_intent`, or `deployment_intent_not_approved`. These are warnings in V1 because deployment execution is not enabled.
- ChangeSet readiness now includes the current Release when present.
- After a DeploymentIntent is approved, ChangeSet readiness warns on `missing_release`, `stale_release`, or `release_not_approved`. These are warnings in V1 because release execution is not enabled.
- ChangeSet readiness now includes the current RegistryEvidence when present.
- After a Release is approved, ChangeSet readiness warns on `missing_release_observability_evidence`, `release_observability_attention_required`, or `release_observability_unknown`. These are warnings in V1 because Release execution is not enabled, but they prevent registry evidence from masquerading as runtime confidence.
- After a Release is approved, ChangeSet readiness warns on `missing_registry_evidence`, `stale_registry_evidence`, `registry_evidence_not_verified`, or `registry_evidence_verification_not_verified`. These are warnings in V1 because registry verification is not enabled.
- Inspection-backed RegistryEvidence that is lifecycle `verified` and has `verification_status = verified` still warns with `registry_evidence_supply_chain_not_verified` unless it carries richer signature, SBOM, provenance, attestation, or vulnerability-check evidence. This keeps identity/probe evidence separate from supply-chain evidence without breaking V1 manual evidence.

# Backlog

- Add typed blocker enums in the Rust API surface once external consumers depend on the exact finding codes.
- Move readiness gate ownership from remediation-plan scope to WorkPlan/ChangeSet scope when approval gates get resource ownership fields.
- Promote registry evidence warnings to blockers for production Release execution once read-only registry verification is live.
- Promote `registry_evidence_supply_chain_not_verified` to a production blocker once signature/SBOM/provenance/vulnerability checks are implemented.
- Promote Release observability warnings to blockers for production Release execution once post-deploy verification policies are defined.
- Add readiness checks for database, RAG context, and production mutation once those resources exist.
