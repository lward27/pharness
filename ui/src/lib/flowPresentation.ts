import { Cube, GitPullRequest, HardDrives, Kanban, MagnifyingGlass, Pulse, RocketLaunch, ShieldCheck, SignOut } from "@phosphor-icons/react";
import { compactId, formatTimestamp, lifecycleTone, statusText } from "./formatters";

function summarizeJson(value: any, fallback: string) {
  if (!value || typeof value !== "object") return fallback;
  if (typeof value.summary === "string") return value.summary;
  if (typeof value.title === "string") return value.title;
  const keys = Object.keys(value);
  return keys.length ? keys.slice(0, 3).join(", ") : fallback;
}

function compactImageRef(value: unknown) {
  if (!value || typeof value !== "string") return "image verification";
  const [repository, tag] = value.split(":");
  const name = repository?.split("/").pop() ?? repository;
  return tag ? `${compactId(name)}:${compactId(tag)}` : compactId(name);
}

export function buildTopology(flow: any) {
  if (!flow) return [];
  const workPlan = flow.work_plan;
  const changeSet = flow.change_set;
  const pipelineIntent = flow.pipeline_intent;
  const pipelineEvidence = pipelineIntent?.execution_evidence;
  const attachedPipelineEvidence = pipelineIntent?.intent_json?.evidence;
  const hasPipelineRunAnalysis = attachedPipelineEvidence?.source === "observation" && attachedPipelineEvidence?.kind === "pipeline_run_analysis";
  const pipelineEvidenceNode = hasPipelineRunAnalysis ? attachedPipelineEvidence : pipelineEvidence;
  const deploymentIntent = flow.deployment_intent;
  const release = flow.release;
  const registryEvidence = flow.registry_evidence;
  return [
    { id: "work-plan", label: "WorkPlan", icon: Kanban, status: lifecycleTone(workPlan?.status), statusLabel: statusText(workPlan?.status), meta: compactId(workPlan?.id), subline: workPlan?.title ?? workPlan?.summary ?? "bounded plan" },
    { id: "change-set", label: "ChangeSet", icon: GitPullRequest, status: changeSet ? lifecycleTone(changeSet.status) : "future", statusLabel: changeSet ? statusText(changeSet.status) : "Not created", meta: changeSet ? compactId(changeSet.id) : "0 changesets", subline: changeSet?.title ?? "waiting for source changes" },
    { id: "pipeline-intent", label: "PipelineIntent", icon: RocketLaunch, status: pipelineIntent ? lifecycleTone(pipelineIntent.status) : "future", statusLabel: pipelineIntent ? statusText(pipelineIntent.status) : "Not created", meta: pipelineIntent ? compactId(pipelineIntent.id) : "0 intents", subline: pipelineIntent?.intent_kind ?? "build/test/package" },
    { id: "pipeline-analysis", label: hasPipelineRunAnalysis ? "PipelineRunAnalysis" : "PipelineRun Receipt", icon: MagnifyingGlass, status: lifecycleTone(pipelineEvidenceNode?.status), statusLabel: statusText(pipelineEvidenceNode?.status, "Missing"), meta: hasPipelineRunAnalysis ? pipelineEvidenceNode?.resource?.name ?? "no analysis" : pipelineEvidenceNode?.pipeline_run?.name ?? "no evidence", subline: compactId(pipelineEvidenceNode?.artifact_id ?? pipelineEvidenceNode?.observation_id ?? "Tekton evidence") },
    { id: "deployment-intent", label: "DeploymentIntent", icon: SignOut, status: deploymentIntent ? lifecycleTone(deploymentIntent.status) : "future", statusLabel: deploymentIntent ? statusText(deploymentIntent.status) : "Not created", meta: deploymentIntent ? compactId(deploymentIntent.id) : "0 intents", subline: deploymentIntent?.argo_application ?? deploymentIntent?.target_namespace ?? "Argo sync gated" },
    { id: "release", label: "Release", icon: Cube, status: release ? lifecycleTone(release.status) : "future", statusLabel: release ? statusText(release.status) : "Not created", meta: release?.version ?? release?.id ?? "0 releases", subline: release?.release_kind ?? "release pending" },
    { id: "registry-evidence", label: "RegistryEvidence", icon: HardDrives, status: registryEvidence ? lifecycleTone(registryEvidence.status) : "future", statusLabel: registryEvidence ? statusText(registryEvidence.status) : "Not created", meta: registryEvidence?.verification_status ?? "no evidence", subline: compactImageRef(registryEvidence?.image_ref) },
  ];
}

export function buildEvidenceRows(flow: any) {
  if (!flow) return [];
  return [
    { source: "Readiness", icon: ShieldCheck, status: flow.readiness?.ready ? "Ready" : "Blocked", tone: flow.readiness?.ready ? "healthy" : "blocked", resource: flow.resource_kind, target: flow.resource_id, finding: flow.readiness?.summary ?? "readiness unavailable", lastEvent: `${flow.readiness?.blockers?.length ?? 0} blockers, ${flow.readiness?.warnings?.length ?? 0} warnings`, link: "Readiness" },
    { source: "WorkPlan", icon: Kanban, status: statusText(flow.work_plan?.status), tone: lifecycleTone(flow.work_plan?.status), resource: "WorkPlan", target: flow.work_plan?.id ?? "missing", finding: flow.work_plan?.summary ?? flow.work_plan?.title ?? "plan available", lastEvent: `revision ${flow.work_plan?.revision ?? 1}`, link: "Plan" },
    { source: "ChangeSet", icon: GitPullRequest, status: flow.change_set ? statusText(flow.change_set.status) : "Missing", tone: flow.change_set ? lifecycleTone(flow.change_set.status) : "future", resource: "ChangeSet", target: flow.change_set?.id ?? "not created", finding: flow.change_set?.summary ?? "source changes not created yet", lastEvent: flow.change_set ? `revision ${flow.change_set.revision}` : "waiting", link: "Diff" },
    { source: "Pipeline", icon: RocketLaunch, status: flow.pipeline_intent ? statusText(flow.pipeline_intent.status) : "Missing", tone: flow.pipeline_intent ? lifecycleTone(flow.pipeline_intent.status) : "future", resource: "PipelineIntent", target: flow.pipeline_intent?.id ?? "not created", finding: summarizeJson(flow.pipeline_intent?.execution_evidence ?? flow.pipeline_intent?.intent_json?.evidence, "pipeline evidence not attached"), lastEvent: flow.pipeline_intent?.intent_kind ?? "planned", link: "Tekton" },
    { source: "Deployment", icon: Pulse, status: flow.deployment_intent ? statusText(flow.deployment_intent.status) : "Missing", tone: flow.deployment_intent ? lifecycleTone(flow.deployment_intent.status) : "future", resource: "DeploymentIntent", target: flow.deployment_intent?.id ?? "not created", finding: summarizeJson(flow.deployment_intent?.intent_json?.deployment_evidence, "deployment evidence not attached"), lastEvent: flow.deployment_intent?.argo_application ?? "planned", link: "Argo" },
    { source: "Registry", icon: HardDrives, status: flow.registry_evidence ? statusText(flow.registry_evidence.status) : "Missing", tone: flow.registry_evidence ? lifecycleTone(flow.registry_evidence.status) : "future", resource: "RegistryEvidence", target: flow.registry_evidence?.image_ref ?? "not created", finding: flow.registry_evidence?.verification_status ?? "supply-chain evidence not attached", lastEvent: flow.registry_evidence?.source ?? "planned", link: "Image" },
  ];
}

export function buildEvents(flow: any) {
  if (!flow?.audit_events?.length) return [];
  return flow.audit_events.slice(-6).map((event: any) => ({ kind: event.kind, tone: event.kind.includes("audit") ? "audit" : event.kind.includes("gate") ? "policy" : "tool", time: formatTimestamp(event.created_at), createdAt: event.created_at, detail: `${event.resource_kind}/${event.resource_id}`, resourceKind: event.resource_kind, resourceId: event.resource_id }));
}
