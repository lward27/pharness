use super::clock::unique_suffix;
use pharness_core::RunId;
use pharness_store::{
    CreateAuditEvent, SqliteStore, StoreError, StoredChangeSet, StoredControllerWait,
    StoredDeploymentContract, StoredDeploymentIntent, StoredGitOpsChangeSet, StoredIncident,
    StoredObservation, StoredPipelineContract, StoredPipelineIntent, StoredRegistryEvidence,
    StoredRelease, StoredRemediationPlan, StoredWorkItem, StoredWorkPlan,
};
use serde_json::json;

pub(in crate::app) async fn append_observation_audit_event(
    store: &SqliteStore,
    observation: &StoredObservation,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", observation.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "observation".to_string(),
            resource_id: observation.id.clone(),
            run_id: observation.run_id.clone(),
            payload_json: json!({
                "observation_id": observation.id,
                "run_id": observation.run_id.as_ref().map(RunId::as_str),
                "source": observation.source,
                "kind": observation.kind,
                "subject": observation.subject,
                "summary": observation.summary,
                "reason": reason,
                "resource": {
                    "namespace": observation.resource_namespace,
                    "kind": observation.resource_kind,
                    "name": observation.resource_name,
                },
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_incident_audit_event(
    store: &SqliteStore,
    incident: &StoredIncident,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", incident.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "incident".to_string(),
            resource_id: incident.id.clone(),
            run_id: incident.run_id.clone(),
            payload_json: json!({
                "incident_id": incident.id,
                "observation_id": incident.observation_id,
                "run_id": incident.run_id.as_ref().map(RunId::as_str),
                "status": incident.status,
                "severity": incident.severity,
                "title": incident.title,
                "summary": incident.summary,
                "reason": reason,
                "resource": {
                    "namespace": incident.resource_namespace,
                    "kind": incident.resource_kind,
                    "name": incident.resource_name,
                },
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_remediation_plan_audit_event(
    store: &SqliteStore,
    plan: &StoredRemediationPlan,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", plan.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "remediation_plan".to_string(),
            resource_id: plan.id.clone(),
            run_id: plan.run_id.clone(),
            payload_json: json!({
                "remediation_plan_id": plan.id,
                "incident_id": plan.incident_id,
                "run_id": plan.run_id.as_ref().map(RunId::as_str),
                "status": plan.status,
                "risk_level": plan.risk_level,
                "requires_approval": plan.requires_approval,
                "title": plan.title,
                "summary": plan.summary,
                "reason": reason,
                "resource": {
                    "namespace": plan.resource_namespace,
                    "kind": plan.resource_kind,
                    "name": plan.resource_name,
                },
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_change_set_audit_event(
    store: &SqliteStore,
    change_set: &StoredChangeSet,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", change_set.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "change_set".to_string(),
            resource_id: change_set.id.clone(),
            run_id: change_set.run_id.clone(),
            payload_json: json!({
                "change_set_id": change_set.id,
                "work_plan_id": change_set.work_plan_id,
                "remediation_plan_id": change_set.remediation_plan_id,
                "incident_id": change_set.incident_id,
                "run_id": change_set.run_id.as_ref().map(RunId::as_str),
                "status": change_set.status,
                "revision": change_set.revision,
                "material_hash": change_set.material_hash,
                "risk_level": change_set.risk_level,
                "summary": change_set.summary,
                "reason": reason,
                "resource": {
                    "namespace": change_set.resource_namespace,
                    "kind": change_set.resource_kind,
                    "name": change_set.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_gitops_change_set_audit_event(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", change_set.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "gitops_change_set".to_string(),
            resource_id: change_set.id.clone(),
            run_id: Some(change_set.run_id.clone()),
            payload_json: json!({
                "gitops_change_set_id": change_set.id,
                "work_item_id": change_set.work_item_id,
                "work_plan_id": change_set.work_plan_id,
                "source_change_set_id": change_set.source_change_set_id,
                "pipeline_intent_id": change_set.pipeline_intent_id,
                "deployment_intent_id": change_set.deployment_intent_id,
                "gitops_update_plan_artifact_id": change_set.gitops_update_plan_artifact_id,
                "run_id": change_set.run_id.as_str(),
                "status": change_set.status,
                "material_hash": change_set.material_hash,
                "gitops": {
                    "repository": change_set.gitops_repo,
                    "base_ref": change_set.gitops_ref,
                    "head_branch": change_set.head_branch,
                    "kustomization_path": change_set.kustomization_path,
                    "image_name": change_set.image_name,
                    "image_ref": change_set.image_ref,
                },
                "reason": reason,
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_pipeline_intent_audit_event(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", intent.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "pipeline_intent".to_string(),
            resource_id: intent.id.clone(),
            run_id: intent.run_id.clone(),
            payload_json: json!({
                "pipeline_intent_id": intent.id,
                "change_set_id": intent.change_set_id,
                "work_plan_id": intent.work_plan_id,
                "remediation_plan_id": intent.remediation_plan_id,
                "incident_id": intent.incident_id,
                "run_id": intent.run_id.as_ref().map(RunId::as_str),
                "status": intent.status,
                "intent_kind": intent.intent_kind,
                "risk_level": intent.risk_level,
                "summary": intent.summary,
                "reason": reason,
                "resource": {
                    "namespace": intent.resource_namespace,
                    "kind": intent.resource_kind,
                    "name": intent.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_pipeline_contract_audit_event(
    store: &SqliteStore,
    contract: &StoredPipelineContract,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", contract.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "pipeline_contract".to_string(),
            resource_id: contract.id.clone(),
            run_id: None,
            payload_json: json!({
                "pipeline_contract_id": contract.id,
                "status": contract.status,
                "namespace": contract.namespace,
                "pipeline_ref": contract.pipeline_ref,
                "version": contract.version,
                "reason": reason,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_deployment_contract_audit_event(
    store: &SqliteStore,
    contract: &StoredDeploymentContract,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", contract.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "deployment_contract".to_string(),
            resource_id: contract.id.clone(),
            run_id: None,
            payload_json: json!({
                "deployment_contract_id": contract.id,
                "status": contract.status,
                "target_environment": contract.target_environment,
                "target_namespace": contract.target_namespace,
                "argo_application": contract.argo_application,
                "version": contract.version,
                "reason": reason,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_deployment_intent_audit_event(
    store: &SqliteStore,
    intent: &StoredDeploymentIntent,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", intent.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "deployment_intent".to_string(),
            resource_id: intent.id.clone(),
            run_id: intent.run_id.clone(),
            payload_json: json!({
                "deployment_intent_id": intent.id,
                "pipeline_intent_id": intent.pipeline_intent_id,
                "change_set_id": intent.change_set_id,
                "work_plan_id": intent.work_plan_id,
                "remediation_plan_id": intent.remediation_plan_id,
                "incident_id": intent.incident_id,
                "run_id": intent.run_id.as_ref().map(RunId::as_str),
                "status": intent.status,
                "intent_kind": intent.intent_kind,
                "risk_level": intent.risk_level,
                "summary": intent.summary,
                "target": {
                    "environment": intent.target_environment,
                    "namespace": intent.target_namespace,
                    "argo_application": intent.argo_application,
                },
                "reason": reason,
                "resource": {
                    "namespace": intent.resource_namespace,
                    "kind": intent.resource_kind,
                    "name": intent.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_release_audit_event(
    store: &SqliteStore,
    release: &StoredRelease,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", release.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "release".to_string(),
            resource_id: release.id.clone(),
            run_id: release.run_id.clone(),
            payload_json: json!({
                "release_id": release.id,
                "deployment_intent_id": release.deployment_intent_id,
                "pipeline_intent_id": release.pipeline_intent_id,
                "change_set_id": release.change_set_id,
                "work_plan_id": release.work_plan_id,
                "remediation_plan_id": release.remediation_plan_id,
                "incident_id": release.incident_id,
                "run_id": release.run_id.as_ref().map(RunId::as_str),
                "status": release.status,
                "release_kind": release.release_kind,
                "risk_level": release.risk_level,
                "summary": release.summary,
                "target": {
                    "environment": release.target_environment,
                    "namespace": release.target_namespace,
                    "argo_application": release.argo_application,
                },
                "artifacts": {
                    "version": release.version,
                    "commit_sha": release.commit_sha,
                    "image_digest": release.image_digest,
                    "rollback_ref": release.rollback_ref,
                },
                "reason": reason,
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_registry_evidence_audit_event(
    store: &SqliteStore,
    evidence: &StoredRegistryEvidence,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", evidence.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "registry_evidence".to_string(),
            resource_id: evidence.id.clone(),
            run_id: evidence.run_id.clone(),
            payload_json: json!({
                "registry_evidence_id": evidence.id,
                "release_id": evidence.release_id,
                "deployment_intent_id": evidence.deployment_intent_id,
                "pipeline_intent_id": evidence.pipeline_intent_id,
                "change_set_id": evidence.change_set_id,
                "work_plan_id": evidence.work_plan_id,
                "remediation_plan_id": evidence.remediation_plan_id,
                "incident_id": evidence.incident_id,
                "run_id": evidence.run_id.as_ref().map(RunId::as_str),
                "status": evidence.status,
                "risk_level": evidence.risk_level,
                "summary": evidence.summary,
                "image": {
                    "registry": evidence.registry,
                    "repository": evidence.repository,
                    "image_ref": evidence.image_ref,
                    "image_digest": evidence.image_digest,
                    "tag": evidence.tag,
                },
                "source": evidence.source,
                "verification_status": evidence.verification_status,
                "reason": reason,
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_work_plan_audit_event(
    store: &SqliteStore,
    plan: &StoredWorkPlan,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", plan.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "work_plan".to_string(),
            resource_id: plan.id.clone(),
            run_id: plan.run_id.clone(),
            payload_json: json!({
                "work_plan_id": plan.id,
                "work_item_id": plan.work_item_id,
                "remediation_plan_id": plan.remediation_plan_id,
                "incident_id": plan.incident_id,
                "run_id": plan.run_id.as_ref().map(RunId::as_str),
                "status": plan.status,
                "revision": plan.revision,
                "risk_level": plan.risk_level,
                "requires_approval": plan.requires_approval,
                "summary": plan.summary,
                "reason": reason,
                "resource": {
                    "namespace": plan.resource_namespace,
                    "kind": plan.resource_kind,
                    "name": plan.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_work_item_audit_event(
    store: &SqliteStore,
    item: &StoredWorkItem,
    kind: &str,
    actor: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", item.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "work_item".to_string(),
            resource_id: item.id.clone(),
            run_id: item.current_run_id.clone(),
            payload_json: json!({
                "work_item_id": item.id,
                "status": item.status,
                "title": item.title,
                "intent": item.intent,
                "source": { "repo": item.source_repo, "ref": item.source_ref },
                "target": {
                    "environment": item.target_environment,
                    "namespace": item.target_namespace,
                    "argo_application": item.argo_application,
                    "production_impacting": item.production_impacting,
                },
                "budget": {
                    "attempts": item.max_attempts,
                    "elapsed_seconds": item.max_elapsed_seconds,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_controller_wait_audit_event(
    store: &SqliteStore,
    wait: &StoredControllerWait,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", wait.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "controller_wait".to_string(),
            resource_id: wait.id.clone(),
            run_id: wait.run_id.clone(),
            payload_json: json!({
                "controller_wait_id": wait.id,
                "work_item_id": wait.work_item_id,
                "run_id": wait.run_id.as_ref().map(RunId::as_str),
                "status": wait.status,
                "wait_kind": wait.wait_kind,
                "subject": { "kind": wait.subject_kind, "id": wait.subject_id },
                "next_check_at": wait.next_check_at,
                "deadline_at": wait.deadline_at,
                "max_checks": wait.max_checks,
                "check_count": wait.check_count,
                "reason": reason,
                "automatic_execution": false,
                "automatic_retry": false,
                "automatic_rollback": false,
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) async fn append_workspace_audit_event(
    store: &SqliteStore,
    workspace: &pharness_store::StoredWorkspace,
    kind: &str,
    actor: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", workspace.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "workspace".to_string(),
            resource_id: workspace.id.clone(),
            run_id: workspace.run_id.clone(),
            payload_json: json!({
                "workspace_id": workspace.id,
                "work_item_id": workspace.work_item_id,
                "status": workspace.status,
                "source": { "repo": workspace.source_repo, "ref": workspace.source_ref },
                "resolved_commit": workspace.resolved_commit,
                "branch": workspace.branch,
                "retention_status": workspace.retention_status,
            }),
        })
        .await
        .map(|_| ())
}
