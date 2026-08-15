import { fetchJson, postJson, withQuery } from "./http";
import { getOperatorName } from "./operator";

const TEKTON_E2E_NAMESPACE = "tekton-pipelines";
const TEKTON_E2E_PIPELINE = "pharness-e2e-noop";
const TEKTON_E2E_CONTRACT = { params: [], workspaces: [] };

function smokeReason(stage) {
  return `console bounded Tekton e2e smoke: ${stage}`;
}

function transition(path, targetStatus) {
  return postJson(path, { target_status: targetStatus, actor: getOperatorName(), reason: smokeReason(`transition to ${targetStatus}`) });
}

async function ensureTektonE2eContract() {
  const payload = await fetchJson(withQuery("/api/pipeline-contracts", {
    namespace: TEKTON_E2E_NAMESPACE, pipeline_ref: TEKTON_E2E_PIPELINE, status: "active", limit: 10,
  }));
  const contracts = Array.isArray(payload?.pipeline_contracts) ? payload.pipeline_contracts : [];
  if (contracts.length === 0) {
    return postJson("/api/pipeline-contracts", {
      namespace: TEKTON_E2E_NAMESPACE, pipeline_ref: TEKTON_E2E_PIPELINE, version: "e2e-v1", contract_json: TEKTON_E2E_CONTRACT,
      actor: getOperatorName(), reason: smokeReason("create fixture contract"),
    });
  }
  if (contracts.length !== 1 || JSON.stringify(contracts[0].contract_json) !== JSON.stringify(TEKTON_E2E_CONTRACT)) {
    throw new Error("The active e2e PipelineContract is missing, duplicated, or does not match the fixture's empty inputs.");
  }
  return contracts[0];
}

export async function prepareTektonE2eSmoke() {
  const actor = getOperatorName();
  const observation = await postJson("/api/observations", {
    source: "tekton_e2e_smoke", kind: "pipeline_execution_request", subject: "finance-experiment-safety-check",
    summary: "Bounded execution smoke; finance experiment resources are observation-only and unchanged.",
    resource_namespace: TEKTON_E2E_NAMESPACE, resource_kind: "Pipeline", resource_name: TEKTON_E2E_PIPELINE,
    resource_ref: { apiVersion: "tekton.dev/v1", kind: "Pipeline", namespace: TEKTON_E2E_NAMESPACE, name: TEKTON_E2E_PIPELINE },
    data_json: { fixture: true, application_resources_changed: false }, actor, reason: smokeReason("create observation"),
  });
  const incident = await postJson("/api/incidents", {
    observation_id: observation.id, severity: "low", title: "Validate bounded Tekton execution",
    summary: "Exercise the inert Pharness delivery path without changing an application.", data_json: { fixture: true }, actor, reason: smokeReason("create incident"),
  });
  const remediationPlan = await postJson("/api/remediation-plans", {
    incident_id: incident.id, title: "Execute inert Tekton fixture", summary: "Preflight and execute a no-op PipelineRun; retain durable evidence.",
    risk_level: "medium", requires_approval: true,
    plan_json: { steps: ["verify contract", "dispatch inert PipelineRun", "record terminal evidence"], approval_gates: [
      { kind: "pipeline_mutation", required_before: "starting the inert PipelineRun" },
      { kind: "cluster_mutation", required_before: "creating the inert PipelineRun" },
    ] }, actor, reason: smokeReason("create remediation plan"),
  });
  const gates = await fetchJson(withQuery("/api/approval-gates", { remediation_plan_id: remediationPlan.id, limit: 10 }));
  for (const gate of gates.approval_gates ?? []) {
    if (["pipeline_mutation", "cluster_mutation"].includes(gate.gate_kind)) {
      await postJson(`/api/approval-gates/${encodeURIComponent(gate.id)}/satisfy`, { decided_by: actor, reason: smokeReason("approve bounded execution gate") });
    }
  }
  const workPlan = (await postJson("/api/work-plans/from-remediation-plan", { remediation_plan_id: remediationPlan.id })).work_plan;
  await transition(`/api/work-plans/${encodeURIComponent(workPlan.id)}/transition`, "proposed");
  const approvedWorkPlan = (await transition(`/api/work-plans/${encodeURIComponent(workPlan.id)}/transition`, "approved")).work_plan;
  const changeSet = (await postJson("/api/change-sets", {
    work_plan_id: approvedWorkPlan.id, title: "Bounded Tekton e2e change", summary: "No application code or configuration changes.", risk_level: "medium",
    change_set_json: { changes: [], fixture: TEKTON_E2E_PIPELINE, application_resources_changed: false }, actor, reason: smokeReason("create change set"),
  })).change_set;
  await transition(`/api/change-sets/${encodeURIComponent(changeSet.id)}/transition`, "proposed");
  const approvedChangeSet = (await transition(`/api/change-sets/${encodeURIComponent(changeSet.id)}/transition`, "approved")).change_set;
  await postJson(`/api/change-sets/${encodeURIComponent(approvedChangeSet.id)}/trusted-envelope`, {
    created_by: actor, reason: smokeReason("authorize bounded change set"), environment: "homelab", namespace: TEKTON_E2E_NAMESPACE, production_impacting: false,
  });
  const pipelineIntent = (await postJson("/api/pipeline-intents/from-change-set", {
    change_set_id: approvedChangeSet.id, title: "Execute inert Tekton fixture", summary: "No-op Pipeline that only emits a marker.", risk_level: "medium", intent_kind: "build_test_package",
    intent_json: { execution: { enabled: true, namespace: TEKTON_E2E_NAMESPACE, pipeline_ref: TEKTON_E2E_PIPELINE, production_impacting: false, params: {}, workspaces: [] } }, actor, reason: smokeReason("create pipeline intent"),
  })).pipeline_intent;
  const approvedPipelineIntent = (await transition(`/api/pipeline-intents/${encodeURIComponent(pipelineIntent.id)}/transition`, "approved")).pipeline_intent;
  const pipelineContract = await ensureTektonE2eContract();
  await postJson(`/api/pipeline-intents/${encodeURIComponent(approvedPipelineIntent.id)}/trusted-envelope`, { created_by: actor, reason: smokeReason("authorize only this inert pipeline intent") });
  const preview = await postJson(`/api/pipeline-intents/${encodeURIComponent(approvedPipelineIntent.id)}/execute`, { dry_run: true, actor, reason: smokeReason("preflight") });
  if (!preview.ready || preview.status !== "ready" || preview.manifest?.metadata?.namespace !== TEKTON_E2E_NAMESPACE) {
    throw new Error("The bounded execution preflight did not pass. No PipelineRun was created.");
  }
  return { observation, incident, remediationPlan, workPlan: approvedWorkPlan, changeSet: approvedChangeSet, pipelineIntent: approvedPipelineIntent, pipelineContract, preview };
}

export function dispatchTektonE2eSmoke(pipelineIntentId) {
  return postJson(`/api/pipeline-intents/${encodeURIComponent(pipelineIntentId)}/execute`, { dry_run: false, actor: getOperatorName(), reason: smokeReason("explicit execution") });
}

export function loadPipelineIntent(pipelineIntentId) {
  return fetchJson(`/api/pipeline-intents/${encodeURIComponent(pipelineIntentId)}`);
}
