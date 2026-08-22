export type DeliveryEvidenceStatus = "complete" | "active" | "waiting" | "blocked" | "unreached";

export type DeliveryFact = {
  label: string;
  value: string;
  tone?: "healthy" | "running" | "pending" | "blocked" | "future";
};

export type DeliveryEvidence = {
  label: string;
  detail: string;
  status: "passed" | "failed" | "waiting" | "recorded";
};

export type DeliveryResource = {
  id: string;
  kind: string;
  label: string;
  summary?: string;
};

export type DeliveryStageModel = {
  key: "source" | "build" | "gitops" | "deploy" | "verify";
  label: string;
  status: DeliveryEvidenceStatus;
  controllerStatus: string;
  summary: string;
  checkpoint?: string;
  facts: DeliveryFact[];
  evidence: DeliveryEvidence[];
  resources: DeliveryResource[];
};

export type DeliveryGuardrails = {
  target: string;
  argoApplication: string;
  productionWindow: string;
  productionWindowStatus: "open" | "expired" | "not_opened";
  desiredDigest: string;
  currentDigest: string;
  digestEquality: string;
  digestEqualityStatus: "verified" | "mismatch" | "unverified";
  baselineDigest: string;
  rollbackOwner: string;
  rollbackStatus: string;
};

export type DeliveryWorkspaceModel = {
  stages: DeliveryStageModel[];
  guardrails: DeliveryGuardrails;
  completedStages: number;
  consistencyIssues: string[];
};

function content(artifact: any) {
  return artifact?.content_json ?? artifact?.content ?? {};
}

function resource(value: any, fallbackLabel: string): DeliveryResource | null {
  if (!value?.id) return null;
  return {
    id: String(value.id),
    kind: String(value.kind ?? value.resource_kind ?? "resource"),
    label: String(value.label ?? value.title ?? fallbackLabel),
    summary: value.summary ?? value.status_reason,
  };
}

function resources(values: Array<DeliveryResource | null | undefined>) {
  const seen = new Set<string>();
  return values.filter((entry): entry is DeliveryResource => {
    if (!entry || seen.has(entry.id)) return false;
    seen.add(entry.id);
    return true;
  });
}

function stageStatus(flow: any, key: string) {
  return flow?.delivery_segments?.find((segment: any) => segment.key === key)?.status ?? "unreached";
}

function short(value?: string | null) {
  if (!value) return "Not recorded";
  return value.length <= 24 ? value : `${value.slice(0, 12)}...${value.slice(-8)}`;
}

function repository(value?: string | null) {
  if (!value) return "Not configured";
  const normalized = value.replace(/\.git$/, "");
  const parts = normalized.split("/").filter(Boolean);
  return parts.slice(-2).join("/") || value;
}

function latestEvent(events: any[] = [], kinds: string[]) {
  return events
    .filter((event) => kinds.includes(event.kind ?? event.event_type ?? event.type))
    .sort((left, right) => Number(right.created_at ?? 0) - Number(left.created_at ?? 0))[0];
}

function isFailure(status?: string | null) {
  return ["failed", "rejected", "blocked"].includes(String(status ?? ""));
}

function manualMergeState(result: any, observation: any, merge: any) {
  const resultBody = content(result);
  const observationBody = content(observation);
  const mergeBody = content(merge);
  const details = resultBody.details ?? {};
  const pullRequestNumber = mergeBody.pull_request_number ?? observationBody.pull_request_number ?? details.pull_request_number;
  const pullRequestUrl = mergeBody.pull_request_url ?? observationBody.pull_request_url ?? details.pull_request_url;
  const mergeCommit = mergeBody.merge_commit_sha ?? observationBody.merge_commit_sha;
  const merged = Boolean(mergeCommit || observationBody.merged === true);
  return {
    created: Boolean(pullRequestNumber || pullRequestUrl),
    merged,
    pullRequestNumber,
    pullRequestUrl,
    mergeCommit,
  };
}

function windowState(expiresAt: any, now: number) {
  const expiry = Number(expiresAt);
  if (!Number.isFinite(expiry)) return { status: "not_opened" as const, label: "Not opened" };
  const formatted = new Date(expiry).toLocaleString();
  if (expiry > now) return { status: "open" as const, label: `Open until ${formatted}` };
  return { status: "expired" as const, label: `Expired ${formatted}` };
}

export function buildDeliveryWorkspace(flow: any, item: any, now = Date.now()): DeliveryWorkspaceModel {
  const lifecycle = flow?.sdlc_flow ?? {};
  const configuration = flow?.delivery_configuration ?? {};
  const gitDelivery = lifecycle.git_delivery ?? {};
  const gitopsDelivery = lifecycle.gitops_delivery ?? {};
  const sourceMerge = manualMergeState(gitDelivery.latest_result, gitDelivery.latest_observation, gitDelivery.latest_merge);
  const gitopsMerge = manualMergeState(gitopsDelivery.latest_result, gitopsDelivery.latest_observation, gitopsDelivery.latest_merge);
  const pipelineIntent = lifecycle.pipeline_intent;
  const pipelineJson = pipelineIntent?.intent_json ?? {};
  const pipelineEvidence = pipelineIntent?.execution_evidence ?? pipelineJson.execution_evidence ?? {};
  const pipelineState = pipelineIntent?.execution_state ?? pipelineJson.execution_state ?? {};
  const buildOutput = pipelineJson.build_output ?? {};
  const gitopsChangeSet = lifecycle.gitops_change_set;
  const deploymentIntent = lifecycle.deployment_intent;
  const release = lifecycle.release;
  const releaseJson = release?.release_json ?? {};
  const verification = releaseJson.post_sync_verification ?? {};
  const verificationChecks = Array.isArray(verification.checks) ? verification.checks : [];
  const argoEvent = latestEvent(flow?.audit_events, ["deployment_intent.execution_observed", "deployment_intent.argo_sync_completed"]);
  const argoEvidence = argoEvent?.payload?.extra ?? argoEvent?.payload_json?.extra ?? {};
  const argoSync = configuration.argo?.sync_status ?? argoEvidence.sync_status;
  const argoHealth = configuration.argo?.health_status ?? argoEvidence.health_status;
  const argoOperation = argoEvidence.operation_phase;

  const sourceStatus: DeliveryEvidenceStatus = sourceMerge.merged
    ? "complete"
    : sourceMerge.created
      ? "waiting"
      : isFailure(lifecycle.change_set?.status)
        ? "blocked"
        : lifecycle.change_set || lifecycle.work_plan
          ? "active"
          : "unreached";
  const buildSucceeded = pipelineEvidence.status === "succeeded" && buildOutput.status === "verified";
  const buildStatus: DeliveryEvidenceStatus = buildSucceeded
    ? "complete"
    : isFailure(pipelineEvidence.status) || String(pipelineState.state ?? "").includes("failed")
      ? "blocked"
      : pipelineIntent
        ? "active"
        : sourceMerge.merged
          ? "waiting"
          : "unreached";
  const gitopsStatus: DeliveryEvidenceStatus = gitopsMerge.merged
    ? "complete"
    : gitopsMerge.created
      ? "waiting"
      : isFailure(gitopsChangeSet?.status)
        ? "blocked"
        : gitopsChangeSet
          ? "active"
          : buildSucceeded
            ? "waiting"
            : "unreached";
  const argoSucceeded = argoSync === "Synced" && (argoOperation === "Succeeded" || verificationChecks.some((check: any) => check.code === "completed_argo_sync" && check.passed));
  const deployStatus: DeliveryEvidenceStatus = argoSucceeded
    ? "complete"
    : [argoSync, argoHealth, argoOperation].some((value) => isFailure(String(value ?? "").toLowerCase()))
      ? "blocked"
      : gitopsMerge.merged
        ? "waiting"
        : "unreached";
  const verificationFailed = verificationChecks.some((check: any) => check.passed === false);
  const verifyStatus: DeliveryEvidenceStatus = release?.status === "completed" && verification.status === "verified"
    ? "complete"
    : verificationFailed || isFailure(release?.status)
      ? "blocked"
      : release || argoSucceeded
        ? "active"
        : "unreached";

  const sourceAttempt = sourceMerge.merged
    ? `PR${sourceMerge.pullRequestNumber ? ` #${sourceMerge.pullRequestNumber}` : ""} merged manually`
    : sourceMerge.created
      ? `PR${sourceMerge.pullRequestNumber ? ` #${sourceMerge.pullRequestNumber}` : ""} awaits manual merge`
      : "Source PR not created";
  const gitopsAttempt = gitopsMerge.merged
    ? `PR${gitopsMerge.pullRequestNumber ? ` #${gitopsMerge.pullRequestNumber}` : ""} merged manually`
    : gitopsMerge.created
      ? `PR${gitopsMerge.pullRequestNumber ? ` #${gitopsMerge.pullRequestNumber}` : ""} awaits manual merge`
      : "GitOps PR not created";
  const executionHistory = Array.isArray(pipelineJson.execution_history) ? pipelineJson.execution_history : [];
  const failedBuilds = executionHistory.filter((entry: any) => entry.status === "failed").length + (pipelineEvidence.status === "failed" ? 1 : 0);
  const passedChecks = verificationChecks.filter((check: any) => check.passed === true).length;

  const sourceStage: DeliveryStageModel = {
    key: "source",
    label: "Source",
    status: sourceStatus,
    controllerStatus: stageStatus(flow, "source"),
    summary: sourceMerge.merged ? "Immutable source merge provenance is recorded." : sourceMerge.created ? "The source pull request is waiting for its required manual merge." : "Source review and delivery have not produced immutable merge provenance.",
    checkpoint: sourceAttempt,
    facts: [
      { label: "Repository", value: repository(item?.source_repo) },
      { label: "Pinned base", value: short(item?.source_commit), tone: item?.source_commit ? "healthy" : "blocked" },
      { label: "ChangeSet", value: lifecycle.change_set ? `${short(lifecycle.change_set.id)} · ${lifecycle.change_set.status}` : "Not created" },
      { label: "Merge commit", value: short(sourceMerge.mergeCommit), tone: sourceMerge.merged ? "healthy" : "pending" },
    ],
    evidence: sourceMerge.created ? [{ label: sourceAttempt, detail: sourceMerge.pullRequestUrl ?? "Durable pull request evidence", status: sourceMerge.merged ? "passed" : "waiting" }] : [],
    resources: resources([
      resource(lifecycle.change_set, "ChangeSet"),
      resource(gitDelivery.latest_result, "Source PR evidence"),
      resource(gitDelivery.latest_merge, "Source merge evidence"),
    ]),
  };

  const buildStage: DeliveryStageModel = {
    key: "build",
    label: "Build",
    status: buildStatus,
    controllerStatus: stageStatus(flow, "build"),
    summary: buildSucceeded ? "Tekton completed and produced a verified immutable image digest." : pipelineIntent ? "The declared PipelineIntent has not produced verified build output." : "Build is waiting for immutable source merge provenance.",
    checkpoint: pipelineState.pipeline_run_name ? `${pipelineState.pipeline_run_namespace ?? "pipeline"}/${pipelineState.pipeline_run_name}` : undefined,
    facts: [
      { label: "Pipeline", value: pipelineJson.execution?.pipeline_ref ?? pipelineJson.pipeline?.name ?? "Not selected" },
      { label: "Source revision", value: short(buildOutput.source_commit ?? pipelineJson.source_provenance?.merge_commit_sha), tone: buildOutput.source_commit ? "healthy" : "future" },
      { label: "PipelineRun", value: pipelineEvidence.status ? `${pipelineEvidence.status}${failedBuilds ? ` · ${failedBuilds} earlier failure${failedBuilds === 1 ? "" : "s"}` : ""}` : "Not started", tone: buildSucceeded ? "healthy" : isFailure(pipelineEvidence.status) ? "blocked" : "pending" },
      { label: "Image digest", value: short(buildOutput.image_digest), tone: buildOutput.status === "verified" ? "healthy" : "future" },
    ],
    evidence: pipelineEvidence.status ? [{ label: "Tekton result", detail: pipelineEvidence.error ?? pipelineJson.evidence?.summary?.pipeline_run_reason ?? pipelineEvidence.status, status: buildSucceeded ? "passed" : isFailure(pipelineEvidence.status) ? "failed" : "recorded" }] : [],
    resources: resources([
      resource(pipelineIntent, "PipelineIntent"),
      buildOutput.artifact_id ? { id: String(buildOutput.artifact_id), kind: "pipeline_build_output", label: "Build output", summary: buildOutput.image_ref } : null,
    ]),
  };

  const gitopsStage: DeliveryStageModel = {
    key: "gitops",
    label: "GitOps",
    status: gitopsStatus,
    controllerStatus: stageStatus(flow, "gitops"),
    summary: gitopsMerge.merged ? "The digest-only GitOps pull request has immutable merge provenance." : gitopsMerge.created ? "The GitOps pull request is waiting for its required manual merge." : "GitOps delivery is waiting for reviewed digest-pinned build output.",
    checkpoint: gitopsAttempt,
    facts: [
      { label: "Repository", value: repository(gitopsChangeSet?.gitops_repo ?? configuration.gitops?.repository) },
      { label: "Kustomization", value: gitopsChangeSet?.kustomization_path ?? configuration.gitops?.kustomization_path ?? "Not configured" },
      { label: "Approved image", value: short(configuration.desired_digest ?? gitopsChangeSet?.image_ref), tone: configuration.desired_digest ? "healthy" : "future" },
      { label: "Merge revision", value: short(gitopsMerge.mergeCommit ?? configuration.gitops?.desired_revision), tone: gitopsMerge.merged ? "healthy" : "pending" },
    ],
    evidence: gitopsMerge.created ? [{ label: gitopsAttempt, detail: gitopsMerge.pullRequestUrl ?? "Durable pull request evidence", status: gitopsMerge.merged ? "passed" : "waiting" }] : [],
    resources: resources([
      resource(gitopsChangeSet, "GitOps ChangeSet"),
      resource(gitopsDelivery.latest_result, "GitOps PR evidence"),
      resource(gitopsDelivery.latest_merge, "GitOps merge evidence"),
    ]),
  };

  const deployStage: DeliveryStageModel = {
    key: "deploy",
    label: "Deploy",
    status: deployStatus,
    controllerStatus: stageStatus(flow, "deploy"),
    summary: argoSucceeded ? "The exact Argo Application sync completed from the approved GitOps revision." : gitopsMerge.merged ? "GitOps is merged; deployment remains behind explicit production-window and Argo approval." : "Deployment is waiting for immutable GitOps merge provenance.",
    checkpoint: argoSucceeded ? `Argo ${argoSync} · ${argoOperation ?? argoHealth ?? "observed"}` : gitopsMerge.merged ? "Explicit Argo approval required" : undefined,
    facts: [
      { label: "Target", value: `${configuration.target?.environment ?? item?.target_environment ?? "Unknown"} · ${configuration.target?.namespace ?? item?.target_namespace ?? "Unknown"}` },
      { label: "Argo Application", value: configuration.target?.argo_application ?? deploymentIntent?.argo_application ?? item?.argo_application ?? "Not configured" },
      { label: "GitOps revision", value: short(configuration.gitops?.desired_revision ?? gitopsMerge.mergeCommit) },
      { label: "Argo state", value: argoSync || argoHealth || argoOperation ? [argoSync, argoHealth, argoOperation].filter(Boolean).join(" · ") : "Not observed", tone: argoSucceeded ? "healthy" : "pending" },
    ],
    evidence: argoEvent ? [{ label: "Argo observation", detail: [argoSync, argoHealth, argoOperation].filter(Boolean).join(" · ") || "Durable Argo execution evidence", status: argoSucceeded ? "passed" : "recorded" }] : [],
    resources: resources([
      resource(deploymentIntent, "DeploymentIntent"),
      argoEvidence.result_artifact_id ? { id: String(argoEvidence.result_artifact_id), kind: "argo_sync_result", label: "Argo sync result" } : null,
    ]),
  };

  const digestCheck = verificationChecks.find((check: any) => check.code === "running_image_digest");
  const verifyStage: DeliveryStageModel = {
    key: "verify",
    label: "Verify",
    status: verifyStatus,
    controllerStatus: stageStatus(flow, "verify"),
    summary: verifyStatus === "complete" ? "Every required DeploymentContract check passed and the Release completed." : verificationFailed ? "One or more required release checks failed; rollback remains operator-controlled." : "Release verification has not recorded a complete contract-scoped result.",
    checkpoint: verificationChecks.length ? `${passedChecks}/${verificationChecks.length} required checks passed` : undefined,
    facts: [
      { label: "DeploymentContract", value: verification.deployment_contract_id ?? configuration.deployment_contract_id ?? item?.deployment_contract_id ?? "Not selected" },
      { label: "Release", value: release ? `${short(release.id)} · ${release.status}` : "Not created", tone: release?.status === "completed" ? "healthy" : "pending" },
      { label: "Release digest", value: short(release?.image_digest ?? releaseJson.release?.image_digest), tone: digestCheck?.passed ? "healthy" : "future" },
      { label: "Digest equality", value: digestCheck ? (digestCheck.passed ? "Verified equal" : "Verification failed") : "Not verified", tone: digestCheck?.passed ? "healthy" : digestCheck?.passed === false ? "blocked" : "future" },
    ],
    evidence: verificationChecks.map((check: any) => ({ label: String(check.code ?? "verification_check"), detail: String(check.summary ?? "No summary recorded"), status: check.passed ? "passed" : "failed" })),
    resources: resources([
      resource(release, "Release"),
      verification.argo_observation_id ? { id: String(verification.argo_observation_id), kind: "argo_observation", label: "Argo health evidence" } : null,
      verification.workload_observation_id ? { id: String(verification.workload_observation_id), kind: "workload_observation", label: "Workload evidence" } : null,
      verification.observability?.prometheus_inventory?.observation_id ? { id: String(verification.observability.prometheus_inventory.observation_id), kind: "prometheus_inventory", label: "Prometheus inventory" } : null,
    ]),
  };

  const stages = [sourceStage, buildStage, gitopsStage, deployStage, verifyStage];
  const consistencyIssues = stages
    .filter((stage) => stage.status === "complete" && stage.controllerStatus !== "complete")
    .map((stage) => `${stage.label} evidence is complete while the controller stage reports ${stage.controllerStatus}.`);
  const window = windowState(configuration.production_window_expires_at, now);
  let digestEqualityStatus: DeliveryGuardrails["digestEqualityStatus"] = "unverified";
  let digestEquality = "Runtime equality not verified";
  if (digestCheck?.passed === true) {
    digestEqualityStatus = "verified";
    digestEquality = "Running digest verified equal";
  } else if (digestCheck?.passed === false) {
    digestEqualityStatus = "mismatch";
    digestEquality = "Running digest verification failed";
  } else if (configuration.current_digest && configuration.desired_digest) {
    const equal = configuration.current_digest === configuration.desired_digest;
    digestEqualityStatus = equal ? "verified" : "unverified";
    digestEquality = equal ? "Reported current and desired digests match" : "Reported current digest differs before verification";
  }

  return {
    stages,
    completedStages: stages.filter((stage) => stage.status === "complete").length,
    consistencyIssues,
    guardrails: {
      target: `${configuration.target?.environment ?? item?.target_environment ?? "Unknown"} · ${configuration.target?.namespace ?? item?.target_namespace ?? "Unknown"} · ${configuration.target?.workload_name ?? item?.workload_name ?? "Unknown workload"}`,
      argoApplication: configuration.target?.argo_application ?? item?.argo_application ?? "Not configured",
      productionWindow: window.label,
      productionWindowStatus: window.status,
      desiredDigest: configuration.desired_digest ?? buildOutput.image_digest ?? "Not built",
      currentDigest: configuration.current_digest ?? "Not observed",
      digestEquality,
      digestEqualityStatus,
      baselineDigest: configuration.baseline_digest ?? "Not captured",
      rollbackOwner: configuration.rollback_owner ?? item?.rollback_owner ?? "Not assigned",
      rollbackStatus: configuration.rollback_status ?? "Unavailable",
    },
  };
}
