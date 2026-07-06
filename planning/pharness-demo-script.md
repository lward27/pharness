# PHarness Demo Script

Audience goal: show PHarness as an agentic SDLC control plane, not a chat UI.

Core message:

> PHarness takes an operator intent and turns it into durable SDLC state: observations, incidents, plans, changes, build intent, deployment intent, release evidence, registry evidence, policy decisions, approvals, readiness, and audit.

Use the deterministic demo unless you have already smoke-tested the live cluster path immediately before presenting.

## Demo Setup

Run from the PHarness repo root.

```bash
cargo test --workspace
```

Narration:

> Before I show autonomy, I want to show the boring part: the control plane is testable. This is not a prompt demo; this is state, policy, and evidence.

Expected:

- All tests pass.
- If time is tight, skip this during the live demo and say it is part of the preflight.

## Primary Demo: Deterministic Control Plane

Run the provider-independent E2E smoke.

```bash
scripts/pharness-e2e-smoke.sh --no-model
```

Narration while it runs:

> This starts a fresh local API with an isolated SQLite database. It does not seed the database directly. Everything goes through the same CLI/API contract a worker or Codex would use.

Expected:

- The script prints a final JSON manifest.
- `status` is `passed`.
- `release_observability` is `attention_required`.
- `release_observability_remediation` is `created`.
- `model_run` is `skipped`.
- `cluster_run` is `skipped`.

Show the manifest:

```bash
jq . target/e2e-smoke/latest/manifest.json
```

Narration:

> This is the compact proof that the machine-facing runtime ran end to end: health, config, direct capabilities, denial policy, audit, SDLC resources, release observability, and readiness.

## Show Safety First: Secret Read Denial

```bash
jq '{status, executed, decision: .decision.decision, summary: .decision.summary}' \
  target/e2e-smoke/latest/secret-denial.json
```

Narration:

> The policy decision is explicit: allow, ask, or deny. This is a secret-shaped Kubernetes read. It is denied before execution, and the response says `executed: false`.

Show the audit fact:

```bash
jq '.events[] | {kind, actor, resource_kind, resource_id, executed: .payload.executed}' \
  target/e2e-smoke/latest/audit-secret-denial.json
```

Narration:

> Denials are not just terminal output. They become audit events.

## Show The SDLC Chain

```bash
jq '{
  ready: .readiness.ready,
  blockers: [.readiness.blockers[].code],
  warnings: [.readiness.warnings[].code],
  work_plan: .work_plan.status,
  change_set: .change_set.status,
  pipeline_intent: .pipeline_intent.status,
  deployment_intent: .deployment_intent.status,
  release: .release.status,
  registry_evidence: .registry_evidence.status,
  incidents: [.incidents[] | {id, status, severity}],
  remediation_plans: [.remediation_plans[] | {id, status, risk_level}],
  approval_gates: [.approval_gates[] | {kind: .gate_kind, status}]
}' target/e2e-smoke/latest/change-set-flow.json
```

Narration:

> This is the core product shape. A task becomes SDLC resources with lifecycle state. PHarness can answer whether the work is ready, what evidence is cautionary, and which review artifacts exist without forcing callers to stitch endpoints together.

Expected:

- `ready` is `true`.
- `blockers` is empty.
- Warnings still include registry verification and release observability caution.
- The flow includes release-observability Incident, RemediationPlan, and ApprovalGate records.

Point to the warning:

> Notice that the chain can be unblocked while still preserving cautionary evidence. PHarness does not hide missing supply-chain proof behind a green status.

## Show Release Observability Evidence

```bash
jq '.release.release_json | {
  deployment_evidence,
  observability_evidence
}' target/e2e-smoke/latest/release-attach-observability.json

jq '{
  incident: .incident,
  remediation_plan: .remediation_plan
}' target/e2e-smoke/latest/release-attach-observability-alert.json

jq '{count, gates: [.approval_gates[] | {kind: .gate_kind, status}]}' \
  target/e2e-smoke/latest/release-observability-approval-gates.json
```

Narration:

> Release approval is not enough. Runtime confidence is separate evidence. PHarness attaches clean observability evidence, then attaches an attention-required Prometheus observation to prove the AIOps handoff: Incident, draft remediation plan, and approval gates.

Expected:

- `observability_evidence[0].status` is `observed`.
- `observability_evidence[0].observation_id` is present.
- The alert attachment returns `incident.status = candidate`.
- The alert attachment returns `remediation_plan.status = draft`.
- Approval gates include `cluster_mutation` and `production_impact`.

Then show that readiness no longer complains about missing Release observability:

```bash
jq '[.warnings[].code]' target/e2e-smoke/latest/change-set-readiness.json
```

Narration:

> Runtime evidence is tracked separately from registry evidence and separately from lifecycle approval. The chain remains inspectable while still carrying the warning.

## Show Registry Evidence

```bash
jq '{
  status: .registry_evidence.status,
  source: .registry_evidence.source,
  verification_status: .registry_evidence.verification_status,
  image_ref: .registry_evidence.image_ref
}' target/e2e-smoke/latest/registry-evidence-create.json
```

Narration:

> Registry evidence is proposed from a typed read-only image inspection. Today this is identity/probe evidence, not full provenance, SBOM, signature, or vulnerability proof.

Then show the lifecycle verification:

```bash
jq '{status: .registry_evidence.status, verification_status: .registry_evidence.verification_status}' \
  target/e2e-smoke/latest/registry-evidence-verify.json
```

Narration:

> The lifecycle can be verified by an operator while the supply-chain verification status remains visible. That distinction matters for production autonomy.

## Show Approval Invalidation

```bash
jq '{
  change_set_status: .change_set.status,
  pipeline_intent_status: .pipeline_intent.status,
  deployment_intent_status: .deployment_intent.status,
  release_status: .release.status,
  registry_evidence_status: .registry_evidence.status,
  blockers: [.blockers[].code],
  warnings: [.warnings[].code]
}' target/e2e-smoke/latest/readiness-after-revision.json
```

Narration:

> This is the anti-approval-fatigue story. We can support trusted envelopes, but when the material ChangeSet changes, prior trust becomes stale. PHarness keeps the audit trail but does not reuse stale approval.

Show the audit event:

```bash
jq '.events[] | select(.kind == "permission_grant.stale") | {
  kind,
  actor,
  resource_id,
  reason: .payload.reason
}' target/e2e-smoke/latest/audit-stale-envelope.json
```

## Optional Live Cluster Demo

Use this only if the cluster context is stable.

```bash
PHARNESS_E2E_ARGO_APP=ghost scripts/pharness-e2e-smoke.sh --cluster --no-model
```

Narration:

> This is the same control-plane path, but with live read-only cluster evidence. PHarness reads Tekton PipelineRuns and TaskRuns, analyzes one concrete PipelineRun, optionally reads Argo, Prometheus, and Loki, persists artifacts and observations, and attaches evidence downstream.

Inspect the cluster manifest:

```bash
jq . target/e2e-smoke/latest/manifest.json
```

Show Tekton evidence:

```bash
jq '{
  status,
  artifact_id,
  observation_id,
  pipeline_run: .result.content.analysis.pipeline_run,
  summary: .result.content.analysis.summary
}' target/e2e-smoke/latest/cluster-tekton-analysis.json
```

If Argo was configured:

```bash
jq '.deployment_intent.intent_json.deployment_evidence' \
  target/e2e-smoke/latest/cluster-deployment-attach-evidence.json
```

If Prometheus or Loki was configured:

```bash
jq '.release.release_json.observability_evidence' \
  target/e2e-smoke/latest/release-attach-observability.json
```

## Optional Fireworks Model Demo

Use this only if `FIREWORKS_API_KEY` and the configured model were tested right before the presentation.

```bash
scripts/pharness-e2e-smoke.sh --model
```

Narration:

> The model is just one worker behind the control plane. The important surface is still the durable run, event stream, policy decisions, tool results, and final structured JSON.

Inspect model events:

```bash
jq '.events[] | {seq, type, payload}' target/e2e-smoke/latest/model-run-events.json
```

## Optional Logs

After any smoke run:

```bash
tail -80 target/e2e-smoke/latest/pharness-api.log
```

Narration:

> PHarness is observable from the outside as an API service. The smoke stores the API log next to the generated artifacts.

## Close

Use this closing:

> The UI can become a nice operator surface, but PHarness is already shaped as the backend control plane: durable state, typed evidence, policy, approvals, audit, readiness, and a clean API for agents to call.

## Decisions

- Demo the deterministic E2E smoke first because it proves the machine-facing contract without depending on provider or cluster availability.
- Treat live cluster and Fireworks model runs as optional add-ons, not the backbone of the presentation.
- Show generated artifacts with `jq` instead of manually recreating every API call during the demo.
- Frame PHarness as an SDLC control plane with agent workers behind it, not as a chat product.

## Backlog

- Add a one-command presenter mode that prints the important `jq` summaries after the smoke run.
- Add a small static HTML report from `target/e2e-smoke/latest` for non-terminal audiences.
- Add a live UI-backed version of this script once the minimal UI is wired to the same API surfaces.
