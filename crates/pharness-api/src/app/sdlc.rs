use super::approvals::grant_is_unexpired;
use super::clock::current_millis;
use super::delivery_segments::sdlc_flow_delivery_segments;
use super::pipeline::evidence::{
    deployment_intent_attached_evidence_status, pipeline_execution_evidence_status,
    pipeline_intent_attached_evidence_status, release_observability_evidence_status,
};
use super::pipeline::state::pipeline_intent_is_deployment_eligible;
use super::releases::release_observability_incident_id_for_ids;
use super::source::delivery_flow::git_delivery_flow;
use super::{gitops::delivery_flow::gitops_delivery_flow, ApiError};
use crate::dto::{
    SdlcFlowResponse, SdlcReadinessFinding, SdlcReadinessGateSummary, SdlcReadinessGrantSummary,
    SdlcReadinessResponse,
};
use pharness_core::PermissionGrantScope;
use pharness_store::{
    ApprovalGateListFilter, RemediationPlanListFilter, SqliteStore, StoredApprovalGate,
    StoredAuditEvent, StoredChangeSet, StoredDeploymentIntent, StoredGitOpsChangeSet,
    StoredIncident, StoredPermissionGrant, StoredPipelineIntent, StoredRegistryEvidence,
    StoredRelease, StoredRemediationPlan, StoredWorkPlan,
};
use serde_json::Value;
use std::collections::BTreeSet;

pub(in crate::app) async fn build_sdlc_flow(
    store: &SqliteStore,
    resource_kind: &str,
    resource_id: &str,
    work_plan: StoredWorkPlan,
    change_set: Option<StoredChangeSet>,
) -> Result<SdlcFlowResponse, ApiError> {
    let pipeline_intent = if let Some(change_set) = &change_set {
        store
            .get_pipeline_intent_by_change_set(&change_set.id)
            .await?
    } else {
        None
    };
    let gitops_change_set = if let Some(pipeline_intent) = &pipeline_intent {
        store
            .get_gitops_change_set_by_pipeline_intent(&pipeline_intent.id)
            .await?
    } else {
        None
    };
    let deployment_intent = if let Some(pipeline_intent) = &pipeline_intent {
        store
            .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
            .await?
    } else {
        None
    };
    let release = if let Some(deployment_intent) = &deployment_intent {
        store
            .get_release_by_deployment_intent(&deployment_intent.id)
            .await?
    } else {
        None
    };
    let registry_evidence = if let Some(release) = &release {
        store.get_registry_evidence_by_release(&release.id).await?
    } else {
        None
    };
    let git_delivery = git_delivery_flow(store, change_set.as_ref()).await?;
    let gitops_delivery = gitops_delivery_flow(store, gitops_change_set.as_ref()).await?;
    let readiness = build_sdlc_readiness(
        store,
        resource_kind,
        resource_id,
        work_plan.clone(),
        change_set.clone(),
    )
    .await?;
    let incidents =
        collect_sdlc_flow_incidents(store, work_plan.incident_id.as_deref(), release.as_ref())
            .await?;
    let remediation_plans =
        collect_sdlc_flow_remediation_plans(store, &work_plan, &incidents).await?;
    let approval_gates =
        collect_sdlc_flow_approval_gates(store, &work_plan, &remediation_plans).await?;
    let audit_events = collect_sdlc_flow_audit_events(
        store,
        &work_plan,
        change_set.as_ref(),
        pipeline_intent.as_ref(),
        gitops_change_set.as_ref(),
        deployment_intent.as_ref(),
        release.as_ref(),
        registry_evidence.as_ref(),
        &incidents,
        &remediation_plans,
        &approval_gates,
    )
    .await?;

    let mut flow = SdlcFlowResponse {
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        readiness,
        delivery_segments: Vec::new(),
        work_plan: work_plan.into(),
        change_set: change_set.map(Into::into),
        pipeline_intent: pipeline_intent.map(Into::into),
        gitops_change_set: gitops_change_set.map(Into::into),
        deployment_intent: deployment_intent.map(Into::into),
        release: release.map(Into::into),
        registry_evidence: registry_evidence.map(Into::into),
        git_delivery,
        gitops_delivery,
        incidents: incidents.into_iter().map(Into::into).collect(),
        remediation_plans: remediation_plans.into_iter().map(Into::into).collect(),
        approval_gates: approval_gates.into_iter().map(Into::into).collect(),
        audit_events: audit_events.into_iter().map(Into::into).collect(),
    };
    flow.delivery_segments = sdlc_flow_delivery_segments(&flow, None);
    Ok(flow)
}

pub(in crate::app) async fn collect_sdlc_flow_incidents(
    store: &SqliteStore,
    root_incident_id: Option<&str>,
    release: Option<&StoredRelease>,
) -> Result<Vec<StoredIncident>, ApiError> {
    let mut incident_ids = BTreeSet::new();
    if let Some(root_incident_id) = root_incident_id {
        incident_ids.insert(root_incident_id.to_string());
    }

    if let Some(release) = release {
        if let Some(evidence) = release
            .release_json
            .get("observability_evidence")
            .and_then(Value::as_array)
        {
            for item in evidence {
                let Some(observation_id) = item.get("observation_id").and_then(Value::as_str)
                else {
                    continue;
                };
                incident_ids.insert(release_observability_incident_id_for_ids(
                    &release.id,
                    observation_id,
                ));
            }
        }
    }

    let mut incidents = Vec::new();
    for incident_id in incident_ids {
        if let Some(incident) = store.get_incident(&incident_id).await? {
            incidents.push(incident);
        }
    }
    Ok(incidents)
}

pub(in crate::app) async fn collect_sdlc_flow_remediation_plans(
    store: &SqliteStore,
    work_plan: &StoredWorkPlan,
    incidents: &[StoredIncident],
) -> Result<Vec<StoredRemediationPlan>, ApiError> {
    let mut plan_ids = BTreeSet::new();
    if let Some(remediation_plan_id) = &work_plan.remediation_plan_id {
        plan_ids.insert(remediation_plan_id.clone());
    }
    for incident in incidents {
        for plan in store
            .list_remediation_plans(RemediationPlanListFilter {
                incident_id: Some(incident.id.clone()),
                limit: 50,
                ..RemediationPlanListFilter::default()
            })
            .await?
        {
            plan_ids.insert(plan.id);
        }
    }

    let mut plans = Vec::new();
    for plan_id in plan_ids {
        if let Some(plan) = store.get_remediation_plan(&plan_id).await? {
            plans.push(plan);
        }
    }
    Ok(plans)
}

pub(in crate::app) async fn collect_sdlc_flow_approval_gates(
    store: &SqliteStore,
    work_plan: &StoredWorkPlan,
    remediation_plans: &[StoredRemediationPlan],
) -> Result<Vec<StoredApprovalGate>, ApiError> {
    let mut gates = Vec::new();
    let mut seen_gate_ids = BTreeSet::new();
    if let Some(work_item_id) = &work_plan.work_item_id {
        for gate in store
            .list_approval_gates(ApprovalGateListFilter {
                work_item_id: Some(work_item_id.clone()),
                limit: 100,
                ..ApprovalGateListFilter::default()
            })
            .await?
        {
            if seen_gate_ids.insert(gate.id.clone()) {
                gates.push(gate);
            }
        }
    }
    for plan in remediation_plans {
        for gate in store
            .list_approval_gates(ApprovalGateListFilter {
                remediation_plan_id: Some(plan.id.clone()),
                limit: 100,
                ..ApprovalGateListFilter::default()
            })
            .await?
        {
            if seen_gate_ids.insert(gate.id.clone()) {
                gates.push(gate);
            }
        }
    }
    Ok(gates)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::app) async fn collect_sdlc_flow_audit_events(
    store: &SqliteStore,
    work_plan: &StoredWorkPlan,
    change_set: Option<&StoredChangeSet>,
    pipeline_intent: Option<&StoredPipelineIntent>,
    gitops_change_set: Option<&StoredGitOpsChangeSet>,
    deployment_intent: Option<&StoredDeploymentIntent>,
    release: Option<&StoredRelease>,
    registry_evidence: Option<&StoredRegistryEvidence>,
    incidents: &[StoredIncident],
    remediation_plans: &[StoredRemediationPlan],
    approval_gates: &[StoredApprovalGate],
) -> Result<Vec<StoredAuditEvent>, ApiError> {
    let mut resources = vec![("work_plan", work_plan.id.clone())];
    if let Some(change_set) = change_set {
        resources.push(("change_set", change_set.id.clone()));
    }
    if let Some(pipeline_intent) = pipeline_intent {
        resources.push(("pipeline_intent", pipeline_intent.id.clone()));
    }
    if let Some(gitops_change_set) = gitops_change_set {
        resources.push(("gitops_change_set", gitops_change_set.id.clone()));
    }
    if let Some(deployment_intent) = deployment_intent {
        resources.push(("deployment_intent", deployment_intent.id.clone()));
    }
    if let Some(release) = release {
        resources.push(("release", release.id.clone()));
    }
    if let Some(registry_evidence) = registry_evidence {
        resources.push(("registry_evidence", registry_evidence.id.clone()));
    }
    resources.extend(
        incidents
            .iter()
            .map(|incident| ("incident", incident.id.clone())),
    );
    resources.extend(
        remediation_plans
            .iter()
            .map(|plan| ("remediation_plan", plan.id.clone())),
    );
    resources.extend(
        approval_gates
            .iter()
            .map(|gate| ("approval_gate", gate.id.clone())),
    );

    let mut events = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    for (resource_kind, resource_id) in resources {
        for event in store
            .list_audit_events(Some(resource_kind), Some(&resource_id), None, 25)
            .await?
        {
            if seen_event_ids.insert(event.id.clone()) {
                events.push(event);
            }
        }
    }
    events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    if events.len() > 200 {
        events.drain(0..events.len() - 200);
    }
    Ok(events)
}

pub(in crate::app) async fn build_sdlc_readiness(
    store: &SqliteStore,
    resource_kind: &str,
    resource_id: &str,
    work_plan: StoredWorkPlan,
    change_set: Option<StoredChangeSet>,
) -> Result<SdlcReadinessResponse, ApiError> {
    let pipeline_intent = if let Some(change_set) = &change_set {
        store
            .get_pipeline_intent_by_change_set(&change_set.id)
            .await?
    } else {
        None
    };
    let deployment_intent = if let Some(pipeline_intent) = &pipeline_intent {
        store
            .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
            .await?
    } else {
        None
    };
    let release = if let Some(deployment_intent) = &deployment_intent {
        store
            .get_release_by_deployment_intent(&deployment_intent.id)
            .await?
    } else {
        None
    };
    let registry_evidence = if let Some(release) = &release {
        store.get_registry_evidence_by_release(&release.id).await?
    } else {
        None
    };
    let gates = match &work_plan.remediation_plan_id {
        Some(remediation_plan_id) => readiness_gate_summary(store, remediation_plan_id).await?,
        None => SdlcReadinessGateSummary {
            pending: Vec::new(),
            stale: Vec::new(),
            rejected: Vec::new(),
        },
    };
    let grants = readiness_grant_summary(store, resource_kind, resource_id).await?;
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    add_status_findings(
        &mut blockers,
        &mut warnings,
        resource_kind,
        resource_id,
        &work_plan,
        change_set.as_ref(),
    );
    add_pipeline_intent_findings(&mut warnings, change_set.as_ref(), pipeline_intent.as_ref());
    add_deployment_intent_findings(
        &mut warnings,
        pipeline_intent.as_ref(),
        deployment_intent.as_ref(),
    );
    add_release_findings(&mut warnings, deployment_intent.as_ref(), release.as_ref());
    add_registry_evidence_findings(&mut warnings, release.as_ref(), registry_evidence.as_ref());
    add_gate_findings(&mut blockers, &gates);
    add_grant_findings(
        &mut blockers,
        &mut warnings,
        resource_kind,
        resource_id,
        &grants,
    );

    let ready = blockers.is_empty();
    let summary = readiness_summary(ready, blockers.len(), warnings.len());

    Ok(SdlcReadinessResponse {
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        ready,
        summary,
        work_plan: work_plan.into(),
        change_set: change_set.map(Into::into),
        pipeline_intent: pipeline_intent.map(Into::into),
        deployment_intent: deployment_intent.map(Into::into),
        release: release.map(Into::into),
        registry_evidence: registry_evidence.map(Into::into),
        blockers,
        warnings,
        approval_gates: gates,
        trusted_envelopes: grants,
    })
}

pub(in crate::app) fn add_status_findings(
    blockers: &mut Vec<SdlcReadinessFinding>,
    warnings: &mut Vec<SdlcReadinessFinding>,
    resource_kind: &str,
    resource_id: &str,
    work_plan: &StoredWorkPlan,
    change_set: Option<&StoredChangeSet>,
) {
    if work_plan.status != "approved" {
        blockers.push(readiness_finding(
            "work_plan_not_approved",
            format!(
                "WorkPlan {} is {}, not approved",
                work_plan.id, work_plan.status
            ),
            "work_plan",
            &work_plan.id,
        ));
    }

    match (resource_kind, change_set) {
        ("change_set", Some(change_set)) if change_set.status != "approved" => {
            blockers.push(readiness_finding(
                "change_set_not_approved",
                format!(
                    "ChangeSet {} is {}, not approved",
                    change_set.id, change_set.status
                ),
                "change_set",
                &change_set.id,
            ));
        }
        ("work_plan", Some(change_set)) if change_set.status != "approved" => {
            blockers.push(readiness_finding(
                "current_change_set_not_approved",
                format!(
                    "Current ChangeSet {} is {}, not approved",
                    change_set.id, change_set.status
                ),
                "change_set",
                &change_set.id,
            ));
        }
        ("work_plan", None) => warnings.push(readiness_finding(
            "missing_change_set",
            "No ChangeSet exists; a WorkPlan trusted envelope is broader than source-change execution",
            "work_plan",
            resource_id,
        )),
        _ => {}
    }
}

pub(in crate::app) fn add_pipeline_intent_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    change_set: Option<&StoredChangeSet>,
    pipeline_intent: Option<&StoredPipelineIntent>,
) {
    let Some(change_set) = change_set else {
        return;
    };
    match pipeline_intent {
        None => warnings.push(readiness_finding(
            "missing_pipeline_intent",
            format!("ChangeSet {} has no PipelineIntent", change_set.id),
            "change_set",
            &change_set.id,
        )),
        Some(intent) if intent.status == "stale" => warnings.push(readiness_finding(
            "stale_pipeline_intent",
            format!("PipelineIntent {} is stale after source changes", intent.id),
            "pipeline_intent",
            &intent.id,
        )),
        Some(intent) if intent.status == "executing" => warnings.push(readiness_finding(
            "pipeline_execution_running",
            format!(
                "PipelineIntent {} has a PipelineRun execution in progress",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some(intent) if intent.status == "failed" => warnings.push(readiness_finding(
            "pipeline_execution_failed",
            format!(
                "PipelineIntent {} has a failed PipelineRun execution",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some(intent) if !pipeline_intent_is_deployment_eligible(&intent.status) => {
            warnings.push(readiness_finding(
                "pipeline_intent_not_approved",
                format!(
                    "PipelineIntent {} is {}, not approved",
                    intent.id, intent.status
                ),
                "pipeline_intent",
                &intent.id,
            ))
        }
        Some(intent) => add_pipeline_evidence_findings(warnings, intent),
    }
}

pub(in crate::app) fn add_pipeline_evidence_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    intent: &StoredPipelineIntent,
) {
    match pipeline_execution_evidence_status(intent) {
        Some("failed") => warnings.push(readiness_finding(
            "pipeline_execution_failed",
            format!(
                "PipelineIntent {} has durable execution evidence showing a failed PipelineRun",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some("succeeded") | None => {}
        Some(_) => warnings.push(readiness_finding(
            "pipeline_execution_unknown",
            format!(
                "PipelineIntent {} has execution evidence with an unknown terminal state",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
    }

    match pipeline_intent_attached_evidence_status(intent) {
        Some("satisfied") => {}
        Some("running") => warnings.push(readiness_finding(
            "pipeline_evidence_running",
            format!(
                "PipelineIntent {} has attached evidence, but the pipeline is still running",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some("attention_required") => warnings.push(readiness_finding(
            "pipeline_evidence_attention_required",
            format!(
                "PipelineIntent {} has attached evidence that requires review before deployment",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some("failed") => warnings.push(readiness_finding(
            "pipeline_evidence_failed",
            format!(
                "PipelineIntent {} has attached evidence from a failed pipeline",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some(_) => warnings.push(readiness_finding(
            "pipeline_evidence_unknown",
            format!(
                "PipelineIntent {} has attached evidence with an unknown status",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        None => warnings.push(readiness_finding(
            "missing_pipeline_evidence",
            format!(
                "PipelineIntent {} is approved but has no attached PipelineRunAnalysis evidence",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
    }
}

pub(in crate::app) fn add_deployment_intent_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    pipeline_intent: Option<&StoredPipelineIntent>,
    deployment_intent: Option<&StoredDeploymentIntent>,
) {
    let Some(pipeline_intent) = pipeline_intent else {
        return;
    };
    if !pipeline_intent_is_deployment_eligible(&pipeline_intent.status) {
        return;
    }

    match deployment_intent {
        None => warnings.push(readiness_finding(
            "missing_deployment_intent",
            format!(
                "PipelineIntent {} has no DeploymentIntent",
                pipeline_intent.id
            ),
            "pipeline_intent",
            &pipeline_intent.id,
        )),
        Some(intent) if intent.status == "stale" => warnings.push(readiness_finding(
            "stale_deployment_intent",
            format!(
                "DeploymentIntent {} is stale after upstream intent changes",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
        Some(intent) if intent.status != "approved" => warnings.push(readiness_finding(
            "deployment_intent_not_approved",
            format!(
                "DeploymentIntent {} is {}, not approved",
                intent.id, intent.status
            ),
            "deployment_intent",
            &intent.id,
        )),
        Some(intent) => add_deployment_evidence_findings(warnings, intent),
    }
}

pub(in crate::app) fn add_deployment_evidence_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    intent: &StoredDeploymentIntent,
) {
    match deployment_intent_attached_evidence_status(intent) {
        Some("satisfied") => {}
        Some("attention_required") => warnings.push(readiness_finding(
            "deployment_evidence_attention_required",
            format!(
                "DeploymentIntent {} has attached Argo evidence that requires review before release",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
        Some(_) => warnings.push(readiness_finding(
            "deployment_evidence_unknown",
            format!(
                "DeploymentIntent {} has attached Argo evidence with an unknown status",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
        None => warnings.push(readiness_finding(
            "missing_deployment_evidence",
            format!(
                "DeploymentIntent {} is approved but has no attached Argo Application evidence",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
    }
}

pub(in crate::app) fn add_release_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    deployment_intent: Option<&StoredDeploymentIntent>,
    release: Option<&StoredRelease>,
) {
    let Some(deployment_intent) = deployment_intent else {
        return;
    };
    if deployment_intent.status != "approved" {
        return;
    }

    match release {
        None => warnings.push(readiness_finding(
            "missing_release",
            format!("DeploymentIntent {} has no Release", deployment_intent.id),
            "deployment_intent",
            &deployment_intent.id,
        )),
        Some(release) if release.status == "stale" => warnings.push(readiness_finding(
            "stale_release",
            format!(
                "Release {} is stale after upstream deployment changes",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some(release) if release.status != "approved" => warnings.push(readiness_finding(
            "release_not_approved",
            format!("Release {} is {}, not approved", release.id, release.status),
            "release",
            &release.id,
        )),
        Some(release) => add_release_observability_findings(warnings, release),
    }
}

pub(in crate::app) fn add_release_observability_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    release: &StoredRelease,
) {
    match release_observability_evidence_status(release) {
        None => warnings.push(readiness_finding(
            "missing_release_observability_evidence",
            format!(
                "Release {} has no attached Prometheus or Loki observability evidence",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some("attention_required") => warnings.push(readiness_finding(
            "release_observability_attention_required",
            format!(
                "Release {} has attached observability evidence that requires review",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some("unknown") => warnings.push(readiness_finding(
            "release_observability_unknown",
            format!(
                "Release {} has attached observability evidence with unknown status",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some(_) => {}
    }
}

pub(in crate::app) fn add_registry_evidence_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    release: Option<&StoredRelease>,
    registry_evidence: Option<&StoredRegistryEvidence>,
) {
    let Some(release) = release else {
        return;
    };
    if release.status != "approved" {
        return;
    }

    let Some(evidence) = registry_evidence else {
        warnings.push(readiness_finding(
            "missing_registry_evidence",
            format!("Release {} has no RegistryEvidence", release.id),
            "release",
            &release.id,
        ));
        return;
    };
    if evidence.status == "stale" {
        warnings.push(readiness_finding(
            "stale_registry_evidence",
            format!(
                "RegistryEvidence {} is stale after upstream release changes",
                evidence.id
            ),
            "registry_evidence",
            &evidence.id,
        ));
        return;
    }
    if evidence.status != "verified" {
        warnings.push(readiness_finding(
            "registry_evidence_not_verified",
            format!(
                "RegistryEvidence {} is {}, not verified",
                evidence.id, evidence.status
            ),
            "registry_evidence",
            &evidence.id,
        ));
    }
    if evidence.verification_status != "verified" {
        warnings.push(readiness_finding(
            "registry_evidence_verification_not_verified",
            format!(
                "RegistryEvidence {} verification status is {}",
                evidence.id, evidence.verification_status
            ),
            "registry_evidence",
            &evidence.id,
        ));
    }
    if evidence.status == "verified"
        && evidence.verification_status == "verified"
        && registry_evidence_is_inspection_backed(evidence)
        && !registry_evidence_has_supply_chain_verification(evidence)
    {
        warnings.push(readiness_finding(
            "registry_evidence_supply_chain_not_verified",
            format!(
                "RegistryEvidence {} is verified but lacks signature, SBOM, provenance, or vulnerability evidence",
                evidence.id
            ),
            "registry_evidence",
            &evidence.id,
        ));
    }
}

pub(in crate::app) fn registry_evidence_is_inspection_backed(
    evidence: &StoredRegistryEvidence,
) -> bool {
    evidence.source == "registry_inspect_image"
        || evidence
            .evidence_json
            .pointer("/execution/capability")
            .and_then(Value::as_str)
            == Some("registry_inspect_image")
}

pub(in crate::app) fn registry_evidence_has_supply_chain_verification(
    evidence: &StoredRegistryEvidence,
) -> bool {
    if matches!(
        evidence.source.as_str(),
        "cosign"
            | "signature"
            | "sbom"
            | "provenance"
            | "slsa_provenance"
            | "vulnerability_scan"
            | "supply_chain"
    ) {
        return true;
    }

    if evidence
        .evidence_json
        .pointer("/verification/supply_chain_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }

    evidence
        .evidence_json
        .pointer("/verification/checks")
        .and_then(Value::as_array)
        .is_some_and(|checks| checks.iter().any(is_supply_chain_check))
}

pub(in crate::app) fn is_supply_chain_check(check: &Value) -> bool {
    let name = check
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let status = check
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let supply_chain_check = [
        "signature",
        "cosign",
        "sbom",
        "provenance",
        "slsa",
        "attestation",
        "vulnerability",
        "vuln",
    ]
    .iter()
    .any(|needle| name.contains(needle));
    let verified_status = ["verified", "pass", "passed", "ok", "success"]
        .iter()
        .any(|allowed| status == *allowed);

    supply_chain_check && verified_status
}

pub(in crate::app) fn add_gate_findings(
    blockers: &mut Vec<SdlcReadinessFinding>,
    gates: &SdlcReadinessGateSummary,
) {
    for gate in &gates.pending {
        blockers.push(readiness_finding(
            "approval_gate_pending",
            format!("ApprovalGate {} is pending", gate.id),
            "approval_gate",
            &gate.id,
        ));
    }
    for gate in &gates.stale {
        blockers.push(readiness_finding(
            "approval_gate_stale",
            format!("ApprovalGate {} is stale", gate.id),
            "approval_gate",
            &gate.id,
        ));
    }
    for gate in &gates.rejected {
        blockers.push(readiness_finding(
            "approval_gate_rejected",
            format!("ApprovalGate {} is rejected", gate.id),
            "approval_gate",
            &gate.id,
        ));
    }
}

pub(in crate::app) fn add_grant_findings(
    blockers: &mut Vec<SdlcReadinessFinding>,
    warnings: &mut Vec<SdlcReadinessFinding>,
    resource_kind: &str,
    resource_id: &str,
    grants: &SdlcReadinessGrantSummary,
) {
    if grants.active.is_empty() {
        blockers.push(readiness_finding(
            "missing_active_trusted_envelope",
            format!("{resource_kind} {resource_id} has no active trusted envelope"),
            resource_kind,
            resource_id,
        ));
    }
    for grant in &grants.stale {
        warnings.push(readiness_finding(
            "stale_trusted_envelope",
            format!("PermissionGrant {} is stale", grant.id),
            "permission_grant",
            &grant.id,
        ));
    }
}

pub(in crate::app) async fn readiness_gate_summary(
    store: &SqliteStore,
    remediation_plan_id: &str,
) -> Result<SdlcReadinessGateSummary, ApiError> {
    let gates = store
        .list_approval_gates(ApprovalGateListFilter {
            remediation_plan_id: Some(remediation_plan_id.to_string()),
            limit: 200,
            ..ApprovalGateListFilter::default()
        })
        .await?;
    let mut pending = Vec::new();
    let mut stale = Vec::new();
    let mut rejected = Vec::new();

    for gate in gates {
        match gate.status.as_str() {
            "pending" => pending.push(gate.into()),
            "stale" => stale.push(gate.into()),
            "rejected" => rejected.push(gate.into()),
            _ => {}
        }
    }

    Ok(SdlcReadinessGateSummary {
        pending,
        stale,
        rejected,
    })
}

pub(in crate::app) async fn readiness_grant_summary(
    store: &SqliteStore,
    resource_kind: &str,
    resource_id: &str,
) -> Result<SdlcReadinessGrantSummary, ApiError> {
    let now = current_millis();
    let grants = store.list_permission_grants(None, 200).await?;
    let mut active = Vec::new();
    let mut stale = Vec::new();

    for grant in grants {
        if !trusted_envelope_matches(&grant, resource_kind, resource_id)? {
            continue;
        }

        match grant.status.as_str() {
            "active" if grant_is_unexpired(&grant, now) => active.push(grant.into()),
            "stale" => stale.push(grant.into()),
            _ => {}
        }
    }

    Ok(SdlcReadinessGrantSummary { active, stale })
}

pub(in crate::app) fn trusted_envelope_matches(
    grant: &StoredPermissionGrant,
    resource_kind: &str,
    resource_id: &str,
) -> Result<bool, ApiError> {
    let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone()).map_err(
        |error| {
            ApiError::internal(format!(
                "permission grant {} has invalid scope: {error}",
                grant.id
            ))
        },
    )?;

    Ok(match resource_kind {
        "work_plan" => {
            !scope.work_plan_ids.is_empty()
                && scope.work_plan_ids.iter().any(|id| id == resource_id)
                && scope.change_set_ids.is_empty()
        }
        "change_set" => {
            !scope.change_set_ids.is_empty()
                && scope.change_set_ids.iter().any(|id| id == resource_id)
        }
        _ => false,
    })
}

pub(in crate::app) fn readiness_finding(
    code: impl Into<String>,
    message: impl Into<String>,
    resource_kind: impl Into<String>,
    resource_id: impl Into<String>,
) -> SdlcReadinessFinding {
    SdlcReadinessFinding {
        code: code.into(),
        message: message.into(),
        resource_kind: resource_kind.into(),
        resource_id: resource_id.into(),
    }
}

pub(in crate::app) fn readiness_summary(
    ready: bool,
    blocker_count: usize,
    warning_count: usize,
) -> String {
    if ready {
        return format!("ready with {warning_count} warning(s)");
    }

    format!("blocked by {blocker_count} blocker(s) and {warning_count} warning(s)")
}
