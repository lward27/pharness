use super::super::audit::append_work_item_audit_event;
use super::super::auth::OperatorIdentity;
use super::super::clock::{current_millis, unique_suffix};
use super::super::deployment::contracts::{
    deployment_contract_spec, validate_deployment_contract_spec,
    validate_protected_production_deployment_contract,
};
use super::super::environment::{inspect_remote_project_contract, select_profile};
use super::super::pipeline::contracts::{pipeline_contract_spec, validate_pipeline_contract_spec};
use super::super::system::{
    capability_statuses, immutable_git_object_id, PROTECTED_ARGO_APPLICATION,
    PROTECTED_ENVIRONMENT, PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME,
    PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE, PROTECTED_PIPELINE_NAMESPACE,
    PROTECTED_PIPELINE_REF, PROTECTED_ROLLBACK_OWNER, PROTECTED_SOURCE_REPO,
    PROTECTED_WORKLOAD_KIND, PROTECTED_WORKLOAD_NAME,
};
use super::super::validation::{clean_optional_text, required_text, validate_kubernetes_name};
use super::super::{ApiError, AppState};
use crate::dto::{CreateWorkItemRequest, WorkItemPreflightResponse, WorkItemResponse};
use axum::extract::State;
use axum::{Extension, Json};
use pharness_core::RunBudget;
use pharness_store::{CreateWorkItem, StoredWorkItem};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn run_budget_from_request(request: &CreateWorkItemRequest) -> Result<RunBudget, ApiError> {
    let defaults = RunBudget::default();
    let budget = RunBudget {
        initial_turns: request
            .initial_turn_budget
            .unwrap_or(defaults.initial_turns),
        hard_turns: request.hard_turn_budget.unwrap_or(defaults.hard_turns),
        initial_tokens: request
            .initial_token_budget
            .unwrap_or(defaults.initial_tokens),
        hard_tokens: request.hard_token_budget.unwrap_or(defaults.hard_tokens),
        active_execution_seconds: request
            .active_execution_seconds
            .or(request.max_elapsed_seconds)
            .unwrap_or(defaults.active_execution_seconds),
        recoverable_tool_errors: request
            .recoverable_tool_error_limit
            .unwrap_or(defaults.recoverable_tool_errors),
        identical_failures: request
            .identical_failure_limit
            .unwrap_or(defaults.identical_failures),
        verification_reserve_turns: defaults.verification_reserve_turns,
    };
    budget
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !(60..=86_400).contains(&budget.active_execution_seconds) {
        return Err(ApiError::bad_request(
            "active execution budget must be between 60 and 86400 seconds",
        ));
    }
    Ok(budget)
}

pub(in crate::app) async fn preflight_work_item(
    State(state): State<AppState>,
    Json(request): Json<CreateWorkItemRequest>,
) -> Result<Json<WorkItemPreflightResponse>, ApiError> {
    build_work_item_preflight(&state, &request).await.map(Json)
}

pub(in crate::app) async fn build_work_item_preflight(
    state: &AppState,
    request: &CreateWorkItemRequest,
) -> Result<WorkItemPreflightResponse, ApiError> {
    let source_repo = request.source_repo.trim().trim_end_matches('/');
    let source_ref = request.source_ref.trim();
    let production =
        request.production_impacting || request.target_environment.trim() == PROTECTED_ENVIRONMENT;
    let budget = run_budget_from_request(request);
    let mut normalized_submission = json!({
        "title": request.title.trim(),
        "intent": request.intent.trim(),
        "acceptance_criteria": request.acceptance_criteria.iter().map(|value| value.trim()).filter(|value| !value.is_empty()).collect::<Vec<_>>(),
        "source_repo": source_repo,
        "source_ref": source_ref,
        "source_commit": request.source_commit.as_deref().map(str::trim),
        "pipeline_contract_id": request.pipeline_contract_id.as_deref().map(str::trim),
        "deployment_contract_id": request.deployment_contract_id.as_deref().map(str::trim),
        "gitops_repo": request.gitops_repo.as_deref().map(|value| value.trim().trim_end_matches('/')),
        "gitops_ref": request.gitops_ref.as_deref().map(str::trim),
        "gitops_kustomization_path": request.gitops_kustomization_path.as_deref().map(str::trim),
        "gitops_image_name": request.gitops_image_name.as_deref().map(str::trim),
        "target_environment": request.target_environment.trim(),
        "target_namespace": request.target_namespace.as_deref().map(str::trim),
        "argo_application": request.argo_application.as_deref().map(str::trim),
        "workload_kind": request.workload_kind.as_deref().map(str::trim),
        "workload_name": request.workload_name.as_deref().map(str::trim),
        "rollback_owner": request.rollback_owner.as_deref().map(str::trim),
        "production_impacting": production,
        "max_attempts": request.max_attempts.unwrap_or(3).clamp(1, 10),
        "max_elapsed_seconds": request.max_elapsed_seconds.unwrap_or(3_600).clamp(60, 86_400),
        "environment_profile_id": request.environment_profile_id.as_deref().map(str::trim),
        "run_budget": budget.as_ref().ok(),
    });
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if let Err(error) = &budget {
        blockers.push(error.message.clone());
    }
    if source_repo.is_empty() {
        blockers.push("source repository is required".to_string());
    }
    if source_ref.is_empty() {
        blockers.push("source base branch is required".to_string());
    }
    if request
        .acceptance_criteria
        .iter()
        .all(|value| value.trim().is_empty())
    {
        warnings.push(
            "No acceptance commands are defined; completion evidence will be weak.".to_string(),
        );
    }
    if let Some(commit) = request.source_commit.as_deref().map(str::trim) {
        if !immutable_git_object_id(commit) {
            blockers
                .push("source_commit must be a full 40- or 64-character Git object ID".to_string());
        }
    } else if production {
        blockers.push("production requires an immutable source_commit".to_string());
    }
    if production && !request_matches_protected_target(request) {
        blockers.push(
            "production target does not exactly match the protected yfinance-wrapper target"
                .to_string(),
        );
    } else if request.target_environment.trim() != "dev" && !production {
        blockers.push("only dev or the exact protected production target is supported".to_string());
    }
    if production && !state.protected_target.exact_locked_match {
        blockers.push(
            "deployed protected-target configuration does not exactly match the locked production target"
                .to_string(),
        );
    }

    let profile_id = request
        .environment_profile_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let selected_profile = match profile_id {
        Some(id) => match select_profile(&state.environment_profiles, id, source_repo) {
            Ok(profile) => Some(profile),
            Err(error) => {
                blockers.push(error);
                None
            }
        },
        None if production => {
            blockers.push("production requires environment_profile_id".to_string());
            None
        }
        None => {
            warnings.push(
                "No immutable environment profile selected; legacy generic runner will be used."
                    .to_string(),
            );
            None
        }
    };
    if production {
        if let Some(profile) = selected_profile {
            let capability = format!("environment_profile:{}", profile.id);
            let verified = state
                .store
                .latest_capability_verification(&capability)
                .await?
                .is_some_and(|verification| {
                    verification.status == "available"
                        && verification
                            .expires_at
                            .parse::<u128>()
                            .is_ok_and(|expires| expires > current_millis())
                });
            if !verified {
                blockers.push(format!(
                    "environment profile {} requires a fresh passing isolated verification",
                    profile.id
                ));
            }
        }
    }

    let mut repository_contract = None;
    let mut repository_contract_hash = None;
    if production {
        if let (Some(profile), Some(commit)) = (
            selected_profile,
            request
                .source_commit
                .as_deref()
                .map(str::trim)
                .filter(|value| immutable_git_object_id(value)),
        ) {
            match inspect_remote_project_contract(source_repo, commit).await {
                Ok((contract, hash)) => {
                    if let Err(error) = contract.validate_for_profile(profile) {
                        blockers.push(format!(
                            "repository contract and environment profile are incompatible: {error}"
                        ));
                    }
                    let declared = contract
                        .acceptance_commands
                        .iter()
                        .map(|command| command.command.trim())
                        .collect::<BTreeSet<_>>();
                    for requested in request
                        .acceptance_criteria
                        .iter()
                        .map(|command| command.trim())
                        .filter(|command| !command.is_empty())
                    {
                        if !declared.contains(requested) {
                            blockers.push(format!(
                                "acceptance command is not declared by the repository contract: {requested}"
                            ));
                        }
                    }
                    repository_contract_hash = Some(hash);
                    repository_contract = serde_json::to_value(contract).ok();
                }
                Err(error) => blockers.push(format!(
                    "repository contract preflight failed before submission: {error}"
                )),
            }
        }
    }
    if let Some(object) = normalized_submission.as_object_mut() {
        object.insert(
            "repository_contract".to_string(),
            repository_contract
                .clone()
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "repository_contract_hash".to_string(),
            repository_contract_hash
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
    }

    let pipeline_contract = match request.pipeline_contract_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => match state.store.get_pipeline_contract(id).await? {
            Some(contract) => {
                if contract.status != "active" {
                    blockers.push(format!(
                        "PipelineContract {id} is {} rather than active",
                        contract.status
                    ));
                }
                if production
                    && (contract.namespace != PROTECTED_PIPELINE_NAMESPACE
                        || contract.pipeline_ref != PROTECTED_PIPELINE_REF
                        || pipeline_contract_spec(&contract.contract_json)
                            .and_then(|spec| {
                                validate_pipeline_contract_spec(&spec)?;
                                if spec.source_revision_param.as_deref() != Some("revision") {
                                    return Err(ApiError::bad_request(
                                        "protected production PipelineContract must bind revision as its immutable source parameter",
                                    ));
                                }
                                Ok(())
                            })
                            .is_err())
                {
                    blockers.push(format!(
                        "PipelineContract {id} does not match tekton-pipelines/pharness-yfinance-build with immutable revision"
                    ));
                }
                Some(contract)
            }
            None => {
                blockers.push(format!("PipelineContract {id} does not exist"));
                None
            }
        },
        _ if production => {
            blockers.push("production requires pipeline_contract_id".to_string());
            None
        }
        _ => None,
    };
    let deployment_contract = match request.deployment_contract_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => match state.store.get_deployment_contract(id).await? {
            Some(contract) => {
                if contract.status != "active" {
                    blockers.push(format!(
                        "DeploymentContract {id} is {} rather than active",
                        contract.status
                    ));
                }
                if production {
                    let target_matches = contract.target_environment == PROTECTED_ENVIRONMENT
                        && contract.target_namespace == PROTECTED_NAMESPACE
                        && contract.argo_application == PROTECTED_ARGO_APPLICATION;
                    let spec_matches = deployment_contract_spec(&contract.contract_json)
                        .and_then(|spec| {
                            validate_deployment_contract_spec(&spec)?;
                            validate_protected_production_deployment_contract(&spec)
                        })
                        .is_ok();
                    if !target_matches || !spec_matches {
                        blockers.push(format!(
                            "DeploymentContract {id} does not match the exact protected workload and /healthz contract"
                        ));
                    }
                }
                Some(contract)
            }
            None => {
                blockers.push(format!("DeploymentContract {id} does not exist"));
                None
            }
        },
        _ if production => {
            blockers.push("production requires deployment_contract_id".to_string());
            None
        }
        _ => None,
    };

    let checks = capability_statuses(state).await?;
    if production {
        for capability in [
            "model_provider",
            "source_workspace",
            "source_writer",
            "source_observer",
            "gitops_writer",
            "gitops_observer",
            "tekton",
            "argo",
            "observability",
        ] {
            if let Some(check) = checks.iter().find(|check| check.capability == capability) {
                if check.status != "available" {
                    blockers.push(format!(
                        "{} must have a fresh passing isolated verification: {}",
                        check.capability, check.summary
                    ));
                }
            }
        }
    }
    let selected_contracts = json!({
        "pipeline": pipeline_contract,
        "deployment": deployment_contract,
    });
    let predicted_external_mutations = if production {
        vec![
            format!("source pull request in {PROTECTED_SOURCE_REPO}"),
            "PipelineRun tekton-pipelines/pharness-yfinance-build".to_string(),
            format!("GitOps pull request in {PROTECTED_GITOPS_REPO}"),
            format!("Argo sync of {PROTECTED_ARGO_APPLICATION}"),
        ]
    } else {
        vec![format!("source pull request in {source_repo}")]
    };
    let production_gates = if production {
        vec![
            "source_mutation".to_string(),
            "pipeline_mutation".to_string(),
            "gitops_mutation".to_string(),
            "cluster_mutation".to_string(),
            "production_deployment".to_string(),
        ]
    } else {
        Vec::new()
    };
    let rollback_prerequisites = if production {
        vec![
            "capture current deployment image digest and readiness".to_string(),
            "capture current Argo and GitOps revisions".to_string(),
            format!("bind rollback ownership to {PROTECTED_ROLLBACK_OWNER}"),
            "prepare a digest-only rollback pull request before Argo authorization".to_string(),
        ]
    } else {
        Vec::new()
    };
    let hash_payload = json!({
        "submission": normalized_submission,
        "contracts": selected_contracts,
        "checks": checks,
        "mutations": predicted_external_mutations,
    });
    let state_hash = format!("{:x}", Sha256::digest(hash_payload.to_string().as_bytes()));
    Ok(WorkItemPreflightResponse {
        ready: blockers.is_empty(),
        state_hash,
        normalized_submission,
        selected_contracts,
        checks,
        blockers,
        warnings,
        predicted_external_mutations,
        production_gates,
        rollback_prerequisites,
    })
}

pub(in crate::app) fn request_matches_protected_target(request: &CreateWorkItemRequest) -> bool {
    request.target_environment.trim() == PROTECTED_ENVIRONMENT
        && request.target_namespace.as_deref().map(str::trim) == Some(PROTECTED_NAMESPACE)
        && request.argo_application.as_deref().map(str::trim) == Some(PROTECTED_ARGO_APPLICATION)
        && request.workload_kind.as_deref().map(str::trim) == Some(PROTECTED_WORKLOAD_KIND)
        && request.workload_name.as_deref().map(str::trim) == Some(PROTECTED_WORKLOAD_NAME)
        && request.source_repo.trim().trim_end_matches('/') == PROTECTED_SOURCE_REPO
        && request
            .gitops_repo
            .as_deref()
            .map(|value| value.trim().trim_end_matches('/'))
            == Some(PROTECTED_GITOPS_REPO)
        && request.gitops_kustomization_path.as_deref().map(str::trim)
            == Some(PROTECTED_KUSTOMIZATION_PATH)
        && request.gitops_image_name.as_deref().map(str::trim) == Some(PROTECTED_IMAGE_NAME)
        && request.rollback_owner.as_deref().map(str::trim) == Some(PROTECTED_ROLLBACK_OWNER)
}

pub(in crate::app) fn stored_work_item_matches_protected_target(item: &StoredWorkItem) -> bool {
    item.production_impacting
        && item.target_environment == PROTECTED_ENVIRONMENT
        && item.target_namespace.as_deref() == Some(PROTECTED_NAMESPACE)
        && item.argo_application.as_deref() == Some(PROTECTED_ARGO_APPLICATION)
        && item.workload_kind.as_deref() == Some(PROTECTED_WORKLOAD_KIND)
        && item.workload_name.as_deref() == Some(PROTECTED_WORKLOAD_NAME)
        && item.source_repo.trim_end_matches('/') == PROTECTED_SOURCE_REPO
        && item
            .gitops_repo
            .as_deref()
            .map(|value| value.trim_end_matches('/'))
            == Some(PROTECTED_GITOPS_REPO)
        && item.gitops_kustomization_path.as_deref() == Some(PROTECTED_KUSTOMIZATION_PATH)
        && item.gitops_image_name.as_deref() == Some(PROTECTED_IMAGE_NAME)
        && item.rollback_owner.as_deref() == Some(PROTECTED_ROLLBACK_OWNER)
        && item
            .source_commit
            .as_deref()
            .is_some_and(immutable_git_object_id)
        && item.pipeline_contract_id.is_some()
        && item.deployment_contract_id.is_some()
}

pub(in crate::app) fn work_item_target_supported(item: &StoredWorkItem) -> bool {
    (!item.production_impacting && item.target_environment == "dev")
        || stored_work_item_matches_protected_target(item)
}

pub(in crate::app) fn bounded_production_grant_expiry(
    item: &StoredWorkItem,
    requested: Option<String>,
) -> Result<Option<String>, ApiError> {
    if !item.production_impacting {
        return Ok(requested);
    }
    let expires_at = requested.ok_or_else(|| {
        ApiError::bad_request("production authorization requires expires_at within 30 minutes")
    })?;
    let expires_ms = expires_at
        .parse::<u128>()
        .map_err(|_| ApiError::bad_request("expires_at must be unix milliseconds"))?;
    let now = current_millis();
    if expires_ms <= now || expires_ms > now + 30 * 60 * 1_000 {
        return Err(ApiError::bad_request(
            "production authorization must expire within the next 30 minutes",
        ));
    }
    Ok(Some(expires_at))
}

pub(in crate::app) async fn create_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreateWorkItemRequest>,
) -> Result<Json<WorkItemResponse>, ApiError> {
    let production =
        request.production_impacting || request.target_environment.trim() == PROTECTED_ENVIRONMENT;
    let production_preflight = if production {
        let preflight = build_work_item_preflight(&state, &request).await?;
        if !preflight.ready {
            return Err(ApiError::conflict(format!(
                "production WorkItem preflight failed: {}",
                preflight.blockers.join("; ")
            )));
        }
        if request.preflight_state_hash.as_deref() != Some(preflight.state_hash.as_str()) {
            return Err(ApiError::conflict(
                "production WorkItem preflight is missing or stale; run preflight again",
            ));
        }
        Some(preflight)
    } else {
        None
    };
    let run_budget = run_budget_from_request(&request)?;
    let title = required_text(request.title, "title")?;
    let intent = required_text(request.intent, "intent")?;
    let source_repo = required_text(request.source_repo, "source_repo")?;
    let source_ref = required_text(request.source_ref, "source_ref")?;
    let target_environment = required_text(request.target_environment, "target_environment")?;
    let target_namespace = clean_optional_text(request.target_namespace);
    let argo_application = clean_optional_text(request.argo_application);
    validate_kubernetes_name("target_environment", &target_environment)?;
    if let Some(namespace) = &target_namespace {
        validate_kubernetes_name("target_namespace", namespace)?;
    }
    if let Some(application) = &argo_application {
        validate_kubernetes_name("argo_application", application)?;
    }
    let max_attempts = if production {
        request.max_attempts.unwrap_or(2).clamp(1, 3)
    } else {
        request.max_attempts.unwrap_or(2).clamp(1, 10)
    };
    let max_elapsed_seconds = run_budget.active_execution_seconds;
    let repository_contract_json = production_preflight
        .as_ref()
        .and_then(|preflight| preflight.normalized_submission.get("repository_contract"))
        .filter(|value| !value.is_null())
        .cloned();
    let repository_contract_hash = production_preflight
        .as_ref()
        .and_then(|preflight| {
            preflight
                .normalized_submission
                .get("repository_contract_hash")
        })
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let work_item = state
        .store
        .create_work_item(CreateWorkItem {
            id: format!("witem_{}", unique_suffix()),
            status: "submitted".to_string(),
            title,
            intent,
            acceptance_criteria: request
                .acceptance_criteria
                .into_iter()
                .filter_map(|criterion| clean_optional_text(Some(criterion)))
                .collect(),
            source_repo,
            source_ref,
            source_commit: clean_optional_text(request.source_commit),
            pipeline_contract_id: clean_optional_text(request.pipeline_contract_id),
            deployment_contract_id: clean_optional_text(request.deployment_contract_id),
            gitops_repo: clean_optional_text(request.gitops_repo),
            gitops_ref: clean_optional_text(request.gitops_ref),
            gitops_kustomization_path: clean_optional_text(request.gitops_kustomization_path),
            gitops_image_name: clean_optional_text(request.gitops_image_name),
            target_environment,
            target_namespace,
            argo_application,
            workload_kind: clean_optional_text(request.workload_kind),
            workload_name: clean_optional_text(request.workload_name),
            rollback_owner: clean_optional_text(request.rollback_owner),
            production_impacting: production,
            max_attempts,
            max_elapsed_seconds,
            created_by: actor.clone(),
            environment_profile_id: clean_optional_text(request.environment_profile_id),
            run_budget,
            repository_contract_json,
            repository_contract_hash,
            environment_preparation_status: if production {
                "pending".to_string()
            } else {
                "not_required".to_string()
            },
        })
        .await?;
    let work_item = state
        .store
        .set_work_item_origin(&work_item.id, "operator")
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.submitted",
        actor,
        json!({ "status": "submitted" }),
    )
    .await?;
    Ok(Json(work_item.into()))
}
