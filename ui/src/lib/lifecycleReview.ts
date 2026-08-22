export type LifecycleAction = {
  id: string;
  resource: string;
  status: string;
  lifecycle_stage?: string;
  effect_class?: string;
  blockers?: Array<{ code?: string; summary?: string }>;
  approval_requirements?: string[];
  external_effect_summary?: string;
  state_hash?: string;
  legacy_reconcile?: boolean;
};

export type ReviewFact = {
  label: string;
  value: string;
  tone?: "healthy" | "risk";
};

export type ReviewGroup = {
  title: string;
  summary?: string;
  facts: ReviewFact[];
  evidence?: string[];
};

export type LifecycleReviewModel = {
  kind: string;
  label: string;
  heading: string;
  resourceId: string;
  resourceStatus: string;
  effectSummary: string;
  approvalRequirements: string[];
  stateHash?: string;
  groups: ReviewGroup[];
  warnings: string[];
};

type ReviewContext = {
  item: any;
  flow: any;
  preview?: any;
  rollbackIntent?: any;
};

function text(value: unknown, fallback = "Not recorded") {
  if (value == null || value === "") return fallback;
  if (typeof value === "boolean") return value ? "Yes" : "No";
  if (Array.isArray(value)) return value.filter(Boolean).join(" · ") || fallback;
  return String(value);
}

function compactRepository(value?: string) {
  if (!value) return "Not recorded";
  const parts = value.replace(/\.git$/, "").split("/").filter(Boolean);
  return parts.slice(-2).join("/") || value;
}

function fact(label: string, value: unknown, tone?: ReviewFact["tone"]): ReviewFact {
  return { label, value: text(value), tone };
}

function evidence(values: unknown[]) {
  return values.flatMap((value) => Array.isArray(value) ? value : [value]).filter((value): value is string => typeof value === "string" && value.trim().length > 0);
}

export function lifecycleReviewKind(action: LifecycleAction) {
  const id = action.id;
  if (id.includes("rollback")) return "rollback_intent";
  if (id.includes("work_plan")) return "work_plan";
  if (id === "authorize_workspace_and_start" || id === "start_coding_attempt") return "workspace";
  if (id.includes("gitops_change_set") || id === "authorize_gitops_delivery" || id.includes("gitops_delivery")) return "gitops_change_set";
  if (id.includes("change_set")) return "change_set";
  if (id.includes("pipeline")) return "pipeline_intent";
  if (id.includes("deployment") || id.includes("argo")) return "deployment_intent";
  if (id.includes("release") || id === "complete_work_item") return "release";
  if (id.startsWith("satisfy_approval_gate:")) return "approval_gate";
  if (id.includes("budget_extension")) return "budget_extension";
  if (id.includes("replan")) return "replan";
  return "controller_action";
}

const labels: Record<string, string> = {
  work_plan: "WorkPlan",
  workspace: "attempt workspace",
  change_set: "source ChangeSet",
  gitops_change_set: "GitOps ChangeSet",
  pipeline_intent: "PipelineIntent",
  deployment_intent: "DeploymentIntent",
  release: "Release",
  rollback_intent: "RollbackIntent",
  approval_gate: "approval gate",
  budget_extension: "run budget",
  replan: "WorkItem replan",
  controller_action: "controller action",
};

function resourceFor(kind: string, action: LifecycleAction, context: ReviewContext) {
  const sdlc = context.flow?.sdlc_flow ?? {};
  if (kind === "work_plan") return sdlc.work_plan;
  if (kind === "workspace") return sdlc.work_plan;
  if (kind === "change_set") return sdlc.change_set;
  if (kind === "gitops_change_set") return sdlc.gitops_change_set;
  if (kind === "pipeline_intent") return sdlc.pipeline_intent;
  if (kind === "deployment_intent") return sdlc.deployment_intent;
  if (kind === "release") return sdlc.release;
  if (kind === "approval_gate") return sdlc.approval_gates?.find((gate: any) => gate.id === action.resource);
  if (kind === "rollback_intent") return context.rollbackIntent?.content ?? context.rollbackIntent;
  return context.item;
}

function commonGroup(action: LifecycleAction, resource: any): ReviewGroup {
  return {
    title: "Decision boundary",
    summary: "This review is bound to one durable resource and one controller state.",
    facts: [
      fact("Resource", action.resource),
      fact("Resource status", resource?.status ?? action.status),
      fact("Lifecycle stage", action.lifecycle_stage),
      fact("Effect class", action.effect_class),
    ],
  };
}

function workPlanGroups(resource: any): ReviewGroup[] {
  const plan = resource?.work_plan_json ?? {};
  const source = plan.source_repository ?? {};
  const target = plan.target ?? {};
  return [{
    title: "Plan scope",
    summary: resource?.summary,
    facts: [
      fact("Risk", resource?.risk_level, resource?.risk_level === "high" || resource?.risk_level === "critical" ? "risk" : undefined),
      fact("Revision", resource?.revision),
      fact("Source", `${compactRepository(source.repo)} @ ${text(source.ref)}`),
      fact("Target", `${text(target.environment)} · ${text(target.namespace)} · ${text(target.argo_application)}`),
    ],
    evidence: evidence([
      ...(plan.acceptance_criteria ?? []).map((command: string) => `Acceptance: ${command}`),
      ...(plan.approval_gates ?? []).map((gate: any) => `${text(gate.kind)}: ${text(gate.required_before)}`),
    ]),
  }];
}

function workspaceGroups(_resource: any, context: ReviewContext): ReviewGroup[] {
  const contract = context.item?.repository_contract ?? {};
  const workspace = context.flow?.workspaces?.slice(-1)[0];
  return [{
    title: "Attempt authorization",
    summary: "The grant is limited to this pinned source, workspace, attempt, and declared writable paths.",
    facts: [
      fact("Pinned source", `${compactRepository(workspace?.source_repo ?? context.item?.source_repo)} @ ${text(workspace?.resolved_commit ?? context.item?.source_commit)}`),
      fact("Attempt branch", workspace?.branch),
      fact("Runner", context.item?.environment_profile_id),
      fact("Next attempt", `${Number(context.item?.attempt_count ?? 0) + 1} of ${text(context.item?.max_attempts)}`),
      fact("Preparation", context.item?.environment_preparation_status),
      fact("Agent network", contract.agent_network),
    ],
    evidence: evidence([
      ...(contract.writable_paths ?? []).map((path: string) => `Writable: ${path}`),
      ...(context.item?.acceptance_criteria ?? []).map((command: string) => `Acceptance: ${command}`),
    ]),
  }];
}

function changeSetGroups(resource: any): ReviewGroup[] {
  const payload = resource?.change_set_json ?? {};
  return [{
    title: "Proposed source change",
    summary: resource?.summary,
    facts: [
      fact("Risk", resource?.risk_level, resource?.risk_level === "high" || resource?.risk_level === "critical" ? "risk" : undefined),
      fact("Revision", resource?.revision),
      fact("Material hash", resource?.material_hash),
      fact("Base commit", payload.source?.base_commit),
      fact("Attempt branch", payload.source?.branch),
      fact("Run", resource?.run_id),
    ],
    evidence: evidence([
      ...(payload.evidence?.changed_paths ?? []).map((path: string) => `Changed: ${path}`),
      payload.evidence?.git_diff_artifact_id ? `Diff evidence: ${payload.evidence.git_diff_artifact_id}` : null,
      payload.evidence?.git_status_artifact_id ? `Git status: ${payload.evidence.git_status_artifact_id}` : null,
      payload.verification?.test_event_count != null ? `Captured test events: ${payload.verification.test_event_count}` : null,
    ]),
  }];
}

function gitOpsChangeSetGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  const payload = resource?.change_set_json ?? resource?.gitops_change_set_json ?? {};
  const delivery = context.flow?.delivery_configuration ?? {};
  return [{
    title: "Digest-only GitOps change",
    summary: resource?.summary,
    facts: [
      fact("Repository", compactRepository(resource?.gitops_repo ?? delivery.gitops?.repository)),
      fact("Kustomization", delivery.gitops?.kustomization_path),
      fact("Image", delivery.gitops?.image_name),
      fact("Desired digest", delivery.desired_digest, delivery.desired_digest ? "healthy" : "risk"),
      fact("Revision", resource?.revision),
      fact("Material hash", resource?.material_hash),
    ],
    evidence: evidence([
      payload.base_revision ? `Immutable base: ${payload.base_revision}` : null,
      payload.image_digest ? `Approved digest: ${payload.image_digest}` : null,
      payload.plan_id ? `Delivery plan: ${payload.plan_id}` : null,
    ]),
  }];
}

function pipelineGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  const payload = resource?.intent_json ?? {};
  const checks = context.preview?.pipeline_execution_preflight?.checks ?? [];
  return [{
    title: "Build and execution evidence",
    summary: resource?.summary,
    facts: [
      fact("Pipeline", `${text(payload.execution?.namespace)} / ${text(payload.execution?.pipeline_ref)}`),
      fact("Contract", payload.pipeline_contract?.id ?? context.item?.pipeline_contract_id),
      fact("Source merge", payload.source_provenance?.merge_commit_sha),
      fact("Execution attempt", payload.execution_attempt ?? 1),
      fact("Build status", payload.build_output?.status ?? resource?.status),
      fact("Image digest", payload.build_output?.image_digest),
    ],
    evidence: evidence([
      ...(checks ?? []).map((check: any) => `${check.passed ? "Passed" : "Blocked"}: ${text(check.summary ?? check.code)}`),
      payload.execution_evidence?.artifact_id ? `Execution evidence: ${payload.execution_evidence.artifact_id}` : null,
      payload.evidence?.artifact_id ? `Pipeline analysis: ${payload.evidence.artifact_id}` : null,
    ]),
  }];
}

function deploymentGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  const payload = resource?.intent_json ?? {};
  const delivery = context.flow?.delivery_configuration ?? {};
  const checks = context.preview?.deployment_execution_preflight?.checks ?? [];
  return [{
    title: "Protected deployment target",
    summary: resource?.summary,
    facts: [
      fact("Environment", resource?.target_environment ?? delivery.target?.environment),
      fact("Namespace", resource?.target_namespace ?? delivery.target?.namespace),
      fact("Argo Application", resource?.argo_application ?? delivery.target?.argo_application),
      fact("DeploymentContract", context.item?.deployment_contract_id),
      fact("Desired digest", delivery.desired_digest),
      fact("GitOps revision", delivery.gitops?.desired_revision),
    ],
    evidence: evidence([
      payload.pipeline_evidence?.status ? `Pipeline evidence: ${payload.pipeline_evidence.status}` : null,
      payload.pipeline_evidence?.artifact_id ? `Pipeline artifact: ${payload.pipeline_evidence.artifact_id}` : null,
      ...(checks ?? []).map((check: any) => `${check.passed ? "Passed" : "Blocked"}: ${text(check.summary ?? check.code)}`),
    ]),
  }];
}

function releaseGroups(resource: any): ReviewGroup[] {
  const payload = resource?.release_json ?? {};
  const verification = payload.post_sync_verification ?? {};
  return [{
    title: "Release evidence",
    summary: resource?.summary,
    facts: [
      fact("Target", `${text(resource?.target_environment)} · ${text(resource?.target_namespace)} · ${text(resource?.argo_application)}`),
      fact("GitOps commit", resource?.commit_sha),
      fact("Image digest", resource?.image_digest),
      fact("Verification", verification.status, verification.status === "verified" ? "healthy" : verification.status ? "risk" : undefined),
      fact("Rollback image", resource?.rollback_ref),
      fact("Release kind", resource?.release_kind),
    ],
    evidence: evidence([
      ...(verification.checks ?? []).map((check: any) => `${check.passed ? "Passed" : "Failed"}: ${text(check.summary ?? check.code)}`),
      verification.argo_observation_id ? `Argo observation: ${verification.argo_observation_id}` : null,
      verification.workload_observation_id ? `Workload observation: ${verification.workload_observation_id}` : null,
    ]),
  }];
}

function rollbackGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  const delivery = context.flow?.delivery_configuration ?? {};
  return [{
    title: "Recovery binding",
    summary: "Rollback is a separately authorized contingency and never runs automatically.",
    facts: [
      fact("Rollback status", resource?.status ?? delivery.rollback_status),
      fact("Owner", delivery.rollback_owner ?? context.item?.rollback_owner),
      fact("Baseline digest", resource?.baseline?.image_digest ?? delivery.baseline_digest),
      fact("Current digest", delivery.current_digest),
      fact("Desired digest", delivery.desired_digest),
      fact("Argo Application", delivery.target?.argo_application ?? context.item?.argo_application),
      fact("GitOps target", compactRepository(delivery.gitops?.repository ?? context.item?.gitops_repo)),
      fact("Authorization expires", delivery.production_window_expires_at),
    ],
  }];
}

function gateGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  return [{
    title: "Governance boundary",
    summary: resource?.summary,
    facts: [
      fact("Gate kind", resource?.gate_kind),
      fact("Gate status", resource?.status),
      fact("Gate order", resource?.gate_order),
      fact("Target", `${text(context.item?.target_environment)} · ${text(context.item?.target_namespace)}`),
      fact("Resource", `${text(resource?.resource_kind)} · ${text(resource?.resource_name)}`),
    ],
  }];
}

function replanGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  const workspace = context.flow?.workspaces?.slice(-1)[0];
  const remaining = Math.max(0, Number(resource?.max_attempts ?? 0) - Number(resource?.attempt_count ?? 0));
  return [{
    title: "Replan boundary",
    summary: resource?.status_reason,
    facts: [
      fact("Attempts remaining", remaining, remaining > 0 ? "healthy" : "risk"),
      fact("Previous workspace", workspace?.id),
      fact("Previous workspace status", workspace?.status),
      fact("Pinned source", resource?.source_commit),
      fact("New attempt starts automatically", false),
    ],
  }];
}

function budgetGroups(resource: any, context: ReviewContext): ReviewGroup[] {
  return [{
    title: "In-place budget extension",
    summary: "The server owns the exact extension amount and preserves the current transcript and workspace.",
    facts: [
      fact("Run", context.item?.current_run_id),
      fact("Initial turns", resource?.run_budget?.initial_turns),
      fact("Hard turn maximum", resource?.run_budget?.hard_turns),
      fact("Initial tokens", resource?.run_budget?.initial_tokens),
      fact("Hard token maximum", resource?.run_budget?.hard_tokens),
    ],
  }];
}

export function buildLifecycleReview(action: LifecycleAction, context: ReviewContext): LifecycleReviewModel {
  const kind = lifecycleReviewKind(action);
  const resource = resourceFor(kind, action, context);
  let groups: ReviewGroup[] = [];
  if (kind === "work_plan") groups = workPlanGroups(resource);
  else if (kind === "workspace") groups = workspaceGroups(resource, context);
  else if (kind === "change_set") groups = changeSetGroups(resource);
  else if (kind === "gitops_change_set") groups = gitOpsChangeSetGroups(resource, context);
  else if (kind === "pipeline_intent") groups = pipelineGroups(resource, context);
  else if (kind === "deployment_intent") groups = deploymentGroups(resource, context);
  else if (kind === "release") groups = releaseGroups(resource);
  else if (kind === "rollback_intent") groups = rollbackGroups(resource, context);
  else if (kind === "approval_gate") groups = gateGroups(resource, context);
  else if (kind === "replan") groups = replanGroups(resource, context);
  else if (kind === "budget_extension") groups = budgetGroups(resource, context);
  else groups = [{
    title: "Controller scope",
    facts: [
      fact("WorkItem", context.item?.id),
      fact("Boundary", context.preview?.boundary),
      fact("Immutable source", context.item?.source_commit),
      fact("Target", `${text(context.item?.target_environment)} · ${text(context.item?.target_namespace)}`),
    ],
  }];

  const label = labels[kind];
  const missingResource = !resource && !["controller_action", "budget_extension"].includes(kind);
  const warnings = [
    ...(missingResource ? [`The server action names ${action.resource}, but its ${label} evidence is not present in this WorkItem flow.`] : []),
    ...(action.legacy_reconcile ? ["This legacy reconcile preview has no action-rail state hash. New lifecycle actions must remain state-hashed."] : []),
  ];

  return {
    kind,
    label,
    heading: kind === "rollback_intent" ? "Review recovery action" : `Review ${label}`,
    resourceId: action.resource,
    resourceStatus: text(resource?.status ?? action.status),
    effectSummary: text(action.external_effect_summary),
    approvalRequirements: action.approval_requirements ?? [],
    stateHash: action.state_hash,
    groups: [commonGroup(action, resource), ...groups],
    warnings,
  };
}

export function reviewAlternatives(selected: LifecycleAction, actions: LifecycleAction[] | undefined) {
  const reviewPrefix = /^(approve|reject)_/;
  if (!reviewPrefix.test(selected.id)) return [selected];
  const candidates = (actions ?? []).filter((action) => action.resource === selected.resource && action.status === "ready" && reviewPrefix.test(action.id));
  return candidates.length ? candidates.sort((left, right) => left.id.startsWith("approve_") ? -1 : right.id.startsWith("approve_") ? 1 : 0) : [selected];
}

export function findCorrectiveAction(blocker: { code?: string; summary?: string }, actions: LifecycleAction[] | undefined) {
  const ready = (actions ?? []).filter((action) => action.status === "ready" && action.lifecycle_stage !== "rollback");
  const value = `${blocker.code ?? ""} ${blocker.summary ?? ""}`.toLowerCase();
  const matches = (pattern: RegExp) => ready.find((action) => pattern.test(action.id));
  if (value.includes("replan")) return matches(/^replan_work_item$/);
  if (value.includes("work plan") || value.includes("work_plan")) return matches(/^(approve|reject)_work_plan$/);
  if (value.includes("gitops") && value.includes("change")) return matches(/^(approve|reject)_gitops_change_set$/);
  if (value.includes("change set") || value.includes("change_set")) return matches(/^(approve|reject)_change_set$/);
  if (value.includes("pipeline")) return matches(/pipeline/);
  if (value.includes("deployment") || value.includes("argo")) return matches(/deployment|argo/);
  if (value.includes("release")) return matches(/release/);
  if (value.includes("gate")) {
    const gateKind = ready.flatMap((action) => action.approval_requirements ?? []).find((kind) => value.includes(kind));
    if (gateKind) return ready.find((action) => action.id.startsWith("satisfy_approval_gate:") && action.approval_requirements?.includes(gateKind));
  }
  return undefined;
}
