use super::projection::source_writer_failure_is_retryable;
use super::state::{
    append_repo_audit, repo_metadata, repo_work_item_state_hash, seal_repo_inapplicable_tail,
};
use crate::app::clock::current_millis;
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::system::capability_verification_summary;
use crate::app::{ApiError, AppState};
use crate::dispatch::{SourceDeliveryExecutionRequest, SourceDeliveryObservationRequest};
use crate::dto::{
    GitDeliveryContextResponse, GitDeliveryObservationContextResponse,
    GitDeliveryObservationOutcomeRequest, GitDeliveryOutcomeRequest,
};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_store::{
    CreateCapabilityVerification, CreateEvidenceValidation, CreateProviderCheckSetObservation,
    CreateSourceDeliveryIntent, CreateStageExecution, SealStageOutcome, StoredSourceDeliveryIntent,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) async fn authorize_and_dispatch_source_delivery(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if !state.worker.supports_remote_workspace() {
        return Err(ApiError::conflict(
            "Repo Mode source delivery requires kubernetes_job worker mode",
        ));
    }
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is required"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .filter(|change_set| change_set.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved ChangeSet is required"))?;
    if state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "source delivery is already bound to this ChangeSet",
        ));
    }
    let repository = state
        .store
        .get_repository(&metadata.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &metadata.repository_id))?;
    if repository.canonical_url != work_item.source_repo {
        return Err(ApiError::conflict(
            "registered Repository does not match the WorkItem source",
        ));
    }
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|allowed| allowed == &repository.canonical_url)
    {
        return Err(ApiError::conflict(
            "Repository is not allowlisted for the isolated Git writer",
        ));
    }
    let source_commit = work_item
        .source_commit
        .clone()
        .filter(|commit| is_git_sha(commit))
        .ok_or_else(|| ApiError::conflict("immutable source commit is unavailable"))?;
    let patch_artifact_id = change_set
        .change_set_json
        .pointer("/patch/artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no patch artifact provenance"))?;
    let patch_hash = change_set
        .change_set_json
        .pointer("/patch/hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no patch hash provenance"))?;
    let run_id = change_set
        .run_id
        .as_ref()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no Builder Run provenance"))?;
    let patch = state
        .store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == patch_artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is unavailable"))?;
    let diff = patch
        .content_text
        .as_deref()
        .filter(|diff| !diff.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != patch_hash {
        return Err(ApiError::conflict(
            "ChangeSet patch artifact does not match its immutable hash",
        ));
    }
    let intent_id = new_prefixed_id("srcintent");
    let execution_id = new_prefixed_id("srcexec");
    let head_branch = format!(
        "pharness/{}/{}",
        work_item_id,
        &change_set.material_hash.trim_start_matches("sha256:")[..12]
    );
    let authorization = json!({
        "schema_version":"pharness.dev/source-delivery-authorization/v1alpha1",
        "actor":actor,
        "reason":reason,
        "work_item_id":work_item_id,
        "work_item_state_hash":repo_work_item_state_hash(&metadata)?,
        "work_plan":{"id":plan.id,"revision":plan.revision},
        "change_set":{"id":change_set.id,"revision":change_set.revision,"material_hash":change_set.material_hash},
        "repository_id":repository.id,
        "source_repo":repository.canonical_url,
        "base_ref":repository.default_branch,
        "base_commit":source_commit,
        "head_branch":head_branch,
        "patch_hash":patch_hash,
        "external_effect":"create one GitHub branch, commit, and pull request; merge is not authorized",
    });
    let intent = state
        .store
        .create_source_delivery_intent(CreateSourceDeliveryIntent {
            id: intent_id,
            subject_kind: "work_item_change_set".into(),
            subject_id: change_set.id.clone(),
            repository_id: repository.id,
            source_repo: repository.canonical_url,
            base_ref: repository.default_branch,
            base_commit: source_commit,
            head_branch,
            patch_artifact_id: Some(patch.id),
            patch_hash: patch_hash.into(),
            authorization,
            created_by: actor.into(),
            creation_reason: reason.into(),
        })
        .await?;
    match state
        .worker
        .dispatch_source_delivery(SourceDeliveryExecutionRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "writer_dispatched",
                    Some(&execution_id),
                    None,
                    None,
                    None,
                    None,
                    actor,
                    reason,
                )
                .await?;
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    "executing",
                    actor,
                    "isolated Git writer dispatched from exact SourceDeliveryIntent",
                    false,
                )
                .await?;
            append_repo_audit(
                state,
                work_item_id,
                "repo.source_delivery.writer_dispatched",
                actor,
                reason,
                json!({"source_delivery_intent_id":intent.id,"execution_id":execution_id,"job_name":receipt.job_name}),
            )
            .await?;
            Ok(
                json!({"source_delivery_intent":intent,"work_item":item,"job_name":receipt.job_name}),
            )
        }
        Err(error) => {
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "failed",
                    Some(&execution_id),
                    None,
                    None,
                    None,
                    None,
                    "controller:repo-mode",
                    "Git writer dispatch failed",
                )
                .await?;
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    "blocked",
                    "controller:repo-mode",
                    "Git writer dispatch failed before any source mutation was confirmed",
                    false,
                )
                .await?;
            tracing::warn!(source_delivery_intent_id=%intent.id, %error, "Repo Mode Git writer dispatch failed");
            Ok(json!({"source_delivery_intent":intent,"work_item":item,"status":"dispatch_failed"}))
        }
    }
}

pub(super) async fn retry_repo_source_delivery(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if !state.worker.supports_remote_workspace() {
        return Err(ApiError::conflict(
            "Repo Mode source delivery requires kubernetes_job worker mode",
        ));
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is required"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .filter(|change_set| change_set.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved ChangeSet is required"))?;
    let intent = state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent is unavailable"))?;
    if !source_writer_failure_is_retryable(&intent) {
        return Err(ApiError::conflict(
            "source writer failure is not eligible for an in-place retry",
        ));
    }
    if intent.subject_id != change_set.id
        || intent.source_repo != work_item.source_repo
        || work_item.source_commit.as_deref() != Some(intent.base_commit.as_str())
    {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent no longer matches the approved WorkItem provenance",
        ));
    }
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|allowed| allowed == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "Repository is not allowlisted for the isolated Git writer",
        ));
    }
    let artifact_id = intent
        .patch_artifact_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent patch artifact is unavailable"))?;
    let run_id = change_set
        .run_id
        .as_ref()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no Builder Run provenance"))?;
    let patch = state
        .store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent patch artifact is unavailable"))?;
    let diff = patch
        .content_text
        .as_deref()
        .filter(|diff| !diff.is_empty())
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != intent.patch_hash {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent patch artifact no longer matches its immutable hash",
        ));
    }

    let now = current_millis();
    let outcome = state
        .worker
        .verify_capability("source_writer", Some(&intent.source_repo))
        .await;
    let (status, summary, principal, repository, permission) = match outcome {
        Ok(outcome) => {
            let status = if outcome.available {
                "available"
            } else {
                "unavailable"
            };
            (
                status,
                capability_verification_summary(&outcome),
                outcome.principal,
                outcome.repository,
                outcome.permission,
            )
        }
        Err(_) => (
            "unavailable",
            "Isolated source writer verification could not complete for the exact repository"
                .to_string(),
            None,
            Some(intent.source_repo.clone()),
            None,
        ),
    };
    let verification = state
        .store
        .create_capability_verification(CreateCapabilityVerification {
            id: new_prefixed_id("capverify"),
            capability: "source_writer".into(),
            status: status.into(),
            summary,
            principal,
            repository,
            permission,
            verified_at: now.to_string(),
            expires_at: (now + 15 * 60 * 1_000).to_string(),
        })
        .await?;
    if verification.status != "available"
        || verification.repository.as_deref() != Some(intent.source_repo.as_str())
    {
        return Err(ApiError::conflict(format!(
            "exact source writer verification failed: {}",
            verification.summary
        )));
    }

    let execution_id = new_prefixed_id("srcexec");
    let receipt = state
        .worker
        .dispatch_source_delivery(SourceDeliveryExecutionRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
        .map_err(|_| ApiError::conflict("Git writer retry dispatch could not complete"))?;
    let prior_failure = intent.status_reason.clone();
    let intent = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "writer_dispatched",
            Some(&execution_id),
            None,
            None,
            None,
            None,
            actor,
            reason,
        )
        .await?;
    let item = state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "executing",
            actor,
            "isolated Git writer retry dispatched from the unchanged SourceDeliveryIntent",
            false,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.source_delivery.writer_retry_dispatched",
        actor,
        reason,
        json!({
            "source_delivery_intent_id":intent.id,
            "execution_id":execution_id,
            "job_name":receipt.job_name,
            "capability_verification_id":verification.id,
            "repository":intent.source_repo,
            "prior_failure":prior_failure,
            "base_commit":intent.base_commit,
            "patch_hash":intent.patch_hash,
            "head_branch":intent.head_branch,
        }),
    )
    .await?;
    Ok(json!({
        "source_delivery_intent":intent,
        "work_item":item,
        "capability_verification":verification,
        "job_name":receipt.job_name,
    }))
}

pub(super) async fn dispatch_source_delivery_observation(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("ChangeSet is unavailable"))?;
    let intent = state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent is unavailable"))?;
    if !matches!(
        intent.status.as_str(),
        "pull_request_open" | "waiting_checks" | "waiting_merge" | "head_drift"
    ) {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent is not ready for observation",
        ));
    }
    if intent.pull_request.is_none() {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent has no pull-request provenance",
        ));
    }
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|allowed| allowed == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "Repository is not allowlisted for the isolated Git observer",
        ));
    }
    let execution_id = new_prefixed_id("srcobserve");
    let dispatched = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "observer_dispatched",
            None,
            Some(&execution_id),
            None,
            None,
            None,
            actor,
            reason,
        )
        .await?;
    match state
        .worker
        .dispatch_source_delivery_observation(SourceDeliveryObservationRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => Ok(json!({"source_delivery_intent":dispatched,"job_name":receipt.job_name})),
        Err(error) => {
            let restored = state
                .store
                .update_source_delivery_intent(
                    &dispatched.id,
                    dispatched.state_version,
                    &intent.status,
                    None,
                    None,
                    None,
                    None,
                    None,
                    "controller:repo-mode",
                    "Git observer dispatch failed; observation remains retryable",
                )
                .await?;
            tracing::warn!(source_delivery_intent_id=%restored.id, %error, "Repo Mode Git observer dispatch failed");
            Ok(json!({"source_delivery_intent":restored,"status":"dispatch_failed"}))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct InternalSourceDeliveryQuery {
    execution_id: String,
}

pub(in crate::app) async fn internal_source_delivery_context(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Query(query): Query<InternalSourceDeliveryQuery>,
) -> Result<Json<GitDeliveryContextResponse>, ApiError> {
    let intent = current_source_delivery_writer(&state, &intent_id, &query.execution_id).await?;
    let artifact_id = intent
        .patch_artifact_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent has no patch artifact"))?;
    let (diff, subject, commit_body, pull_request_body) = match intent.subject_kind.as_str() {
        "work_item_change_set" => {
            let change_set = state
                .store
                .get_change_set(&intent.subject_id)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent ChangeSet is unavailable")
                })?;
            let run_id = change_set
                .run_id
                .as_ref()
                .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent has no Builder Run"))?;
            let diff = state
                .store
                .list_artifacts(run_id)
                .await?
                .into_iter()
                .find(|artifact| {
                    artifact.id == artifact_id && artifact.kind == "workspace_git_diff"
                })
                .and_then(|artifact| artifact.content_text)
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent patch evidence is invalid")
                })?;
            let subject = change_set.title.trim().replace(['\r', '\n'], " ");
            let commit_body = format!(
                "PHarness WorkItem {}\n\nChangeSet: {}",
                change_set.work_item_id.as_deref().unwrap_or("unknown"),
                change_set.id
            );
            let pull_request_body = format!(
                "Controller-derived source delivery for ChangeSet `{}`. Manual merge is required.",
                change_set.id
            );
            (diff, subject, commit_body, pull_request_body)
        }
        "repository_onboarding_proposal" => {
            let proposal = state
                .store
                .get_repository_onboarding_proposal(&intent.subject_id)
                .await?
                .filter(|proposal| proposal.status == "approved")
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding proposal is unavailable")
                })?;
            let onboarding = state
                .store
                .get_repository_onboarding(&proposal.onboarding_id)
                .await?
                .filter(|onboarding| {
                    onboarding.source_delivery_intent_id.as_deref() == Some(intent.id.as_str())
                        && onboarding.approved_proposal_hash.as_deref()
                            == Some(proposal.content_hash.as_str())
                })
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding provenance is unavailable")
                })?;
            let diff = state
                .store
                .get_artifact(artifact_id)
                .await?
                .filter(|artifact| artifact.kind == "repository_onboarding_patch")
                .and_then(|artifact| artifact.content_text)
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding patch is unavailable")
                })?;
            let subject = format!("Onboard repository with PHarness ({})", onboarding.id);
            let commit_body = format!(
                "PHarness Repository onboarding\n\nOnboarding: {}\nProposal: {}",
                onboarding.id, proposal.id
            );
            let pull_request_body = format!(
                "Controller-materialized onboarding contract for proposal `{}`. Manual merge is required.",
                proposal.id
            );
            (diff, subject, commit_body, pull_request_body)
        }
        _ => {
            return Err(ApiError::conflict(
                "SourceDeliveryIntent subject kind is unsupported",
            ))
        }
    };
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != intent.patch_hash {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent patch evidence hash is invalid",
        ));
    }
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent repository is not writer-allowlisted",
        ));
    }
    Ok(Json(GitDeliveryContextResponse {
        execution_id: query.execution_id,
        repository: intent.source_repo,
        base_ref: intent.base_ref,
        base_commit: intent.base_commit,
        head_branch: intent.head_branch,
        diff,
        commit_subject: subject.clone(),
        commit_body,
        pull_request_title: subject,
        pull_request_body,
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    }))
}

pub(in crate::app) async fn internal_source_delivery_writer_outcome(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(request): Json<GitDeliveryOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let intent = current_source_delivery_writer(&state, &intent_id, &request.execution_id).await?;
    let subject = source_delivery_subject(&state, &intent).await?;
    match request.status.as_str() {
        "completed" => {
            let branch = request
                .branch
                .filter(|value| value == &intent.head_branch)
                .ok_or_else(|| {
                    ApiError::conflict(
                        "writer outcome branch does not match the SourceDeliveryIntent",
                    )
                })?;
            let commit_sha = request
                .commit_sha
                .filter(|value| is_git_sha(value))
                .ok_or_else(|| {
                    ApiError::bad_request("writer outcome requires a full commit SHA")
                })?;
            let pull_request_url = request
                .pull_request_url
                .filter(|value| crate::app::identifiers::is_github_pr_url(value))
                .ok_or_else(|| {
                    ApiError::bad_request("writer outcome requires a valid GitHub pull-request URL")
                })?;
            let pull_request_number = request.pull_request_number.ok_or_else(|| {
                ApiError::bad_request("writer outcome requires a pull-request number")
            })?;
            let expected_prefix = format!("{}/pull/", intent.source_repo.trim_end_matches(".git"));
            if !pull_request_url.starts_with(&expected_prefix)
                || !pull_request_url.ends_with(&format!("/{pull_request_number}"))
            {
                return Err(ApiError::conflict("writer outcome pull request does not match the SourceDeliveryIntent repository"));
            }
            let pull_request = json!({
                "url":pull_request_url,
                "number":pull_request_number,
                "head_branch":branch,
                "head_sha":commit_sha,
            });
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "pull_request_open",
                    None,
                    None,
                    Some(&pull_request),
                    None,
                    None,
                    "agent:git-writer",
                    "isolated writer reported exact pull-request provenance",
                )
                .await?;
            let subject_response = match subject {
                SourceDeliverySubject::WorkItem(work_item_id) => {
                    let item = state.store.update_repo_work_item_status(
                        &work_item_id, "waiting_external", "controller:repo-mode",
                        "source pull request is open; authoritative checks and manual merge are pending", false,
                    ).await?;
                    json!({"work_item":item})
                }
                SourceDeliverySubject::Onboarding(onboarding_id) => {
                    let onboarding = state.store.update_repository_onboarding_source_delivery(
                        &onboarding_id, &intent.id, "waiting_external", None,
                        "controller:repo-mode", "onboarding pull request is open; authoritative checks and manual merge are pending",
                    ).await?;
                    json!({"onboarding":onboarding})
                }
            };
            Ok(Json(
                json!({"source_delivery_intent":intent,"subject":subject_response}),
            ))
        }
        "failed" => {
            let error = request
                .error_code
                .unwrap_or_else(|| "git_writer_failed".into());
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "failed",
                    None,
                    None,
                    None,
                    None,
                    None,
                    "agent:git-writer",
                    &error,
                )
                .await?;
            let subject_response = match subject {
                SourceDeliverySubject::WorkItem(work_item_id) => {
                    let item = state
                        .store
                        .update_repo_work_item_status(
                            &work_item_id,
                            "blocked",
                            "controller:repo-mode",
                            "source writer failed before pull-request provenance was confirmed",
                            false,
                        )
                        .await?;
                    json!({"work_item":item})
                }
                SourceDeliverySubject::Onboarding(onboarding_id) => {
                    let onboarding = state.store.update_repository_onboarding_source_delivery(
                        &onboarding_id, &intent.id, "delivery_failed", None,
                        "controller:repo-mode", "onboarding source writer failed before pull-request provenance was confirmed",
                    ).await?;
                    json!({"onboarding":onboarding})
                }
            };
            Ok(Json(
                json!({"source_delivery_intent":intent,"subject":subject_response}),
            ))
        }
        _ => Err(ApiError::bad_request(
            "source delivery writer status must be completed or failed",
        )),
    }
}

pub(in crate::app) async fn internal_source_delivery_observation_context(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Query(query): Query<InternalSourceDeliveryQuery>,
) -> Result<Json<GitDeliveryObservationContextResponse>, ApiError> {
    let intent = current_source_delivery_observer(&state, &intent_id, &query.execution_id).await?;
    let pull_request = intent
        .pull_request
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("SourceDeliveryIntent pull-request provenance is unavailable")
        })?;
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent repository is not observer-allowlisted",
        ));
    }
    Ok(Json(GitDeliveryObservationContextResponse {
        execution_id: query.execution_id,
        repository: intent.source_repo,
        base_ref: intent.base_ref,
        head_branch: pull_request
            .get("head_branch")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("pull-request head branch is unavailable"))?
            .into(),
        source_commit_sha: pull_request
            .get("head_sha")
            .and_then(Value::as_str)
            .filter(|sha| is_git_sha(sha))
            .ok_or_else(|| ApiError::conflict("pull-request head SHA is unavailable"))?
            .into(),
        pull_request_url: pull_request
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("pull-request URL is unavailable"))?
            .into(),
        pull_request_number: pull_request
            .get("number")
            .and_then(Value::as_u64)
            .ok_or_else(|| ApiError::conflict("pull-request number is unavailable"))?,
        github_api_url: settings.github_api_url,
    }))
}

pub(in crate::app) async fn internal_source_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(request): Json<GitDeliveryObservationOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let intent =
        current_source_delivery_observer(&state, &intent_id, &request.execution_id).await?;
    let subject = source_delivery_subject(&state, &intent).await?;
    if request.status == "failed" {
        let restored = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                "pull_request_open",
                None,
                None,
                None,
                None,
                None,
                "agent:git-observer",
                request
                    .error_code
                    .as_deref()
                    .unwrap_or("git_observer_failed"),
            )
            .await?;
        if let SourceDeliverySubject::Onboarding(onboarding_id) = &subject {
            state
                .store
                .update_repository_onboarding_source_delivery(
                    onboarding_id,
                    &intent.id,
                    "waiting_external",
                    None,
                    "controller:repo-mode",
                    "Git observer failed; onboarding observation remains retryable",
                )
                .await?;
        }
        return Ok(Json(
            json!({"source_delivery_intent":restored,"status":"observation_failed"}),
        ));
    }
    if request.status != "observed" || !request.authoritative_rules_succeeded {
        return Err(ApiError::conflict(
            "authoritative GitHub branch-rule observation is required",
        ));
    }
    let pull_request = intent
        .pull_request
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("SourceDeliveryIntent pull-request provenance is unavailable")
        })?;
    let expected_head = pull_request
        .get("head_sha")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent expected head is unavailable"))?;
    let head_sha = request
        .head_commit_sha
        .as_deref()
        .filter(|sha| is_git_sha(sha))
        .ok_or_else(|| ApiError::bad_request("observation requires a full head SHA"))?;
    let merged = request
        .merged
        .ok_or_else(|| ApiError::bad_request("observation requires merged"))?;
    let pull_request_state = request
        .pull_request_state
        .as_deref()
        .filter(|state| matches!(*state, "open" | "closed"))
        .ok_or_else(|| {
            ApiError::bad_request("observation requires an open or closed pull-request state")
        })?;
    let provider_status = derive_provider_check_status(&request.required_checks)?;
    if request.provider_check_status.as_deref() != Some(provider_status) {
        return Err(ApiError::conflict(
            "provider-check result does not match controller derivation",
        ));
    }
    if !request.check_runs.is_array() || !request.commit_statuses.is_array() {
        return Err(ApiError::bad_request(
            "provider-check evidence must be bounded arrays",
        ));
    }
    let required_set_hash = canonical_material_hash(&request.required_checks)?;
    let observation_material = json!({
        "source_delivery_intent_id":intent.id,
        "phase":if merged {"merge"} else {"pre_merge"},
        "head_sha":head_sha,
        "required_set_hash":required_set_hash,
        "status":provider_status,
        "required_checks":request.required_checks,
        "check_runs":request.check_runs,
        "commit_statuses":request.commit_statuses,
    });
    let provider_observation = state
        .store
        .create_provider_check_set_observation(CreateProviderCheckSetObservation {
            id: new_prefixed_id("providerchecks"),
            source_delivery_intent_id: intent.id.clone(),
            phase: if merged {
                "merge".into()
            } else {
                "pre_merge".into()
            },
            repository_id: intent.repository_id.clone(),
            pull_request_number: pull_request
                .get("number")
                .and_then(Value::as_u64)
                .ok_or_else(|| ApiError::conflict("pull-request number is unavailable"))?,
            head_sha: head_sha.into(),
            required_set_hash: required_set_hash.clone(),
            authoritative_rules_succeeded: true,
            status: provider_status.into(),
            required_checks: request.required_checks.clone(),
            check_runs: request.check_runs.clone(),
            commit_statuses: request.commit_statuses.clone(),
            content_hash: canonical_material_hash(&observation_material)?,
            expires_at: (current_millis() + 15 * 60 * 1_000).to_string(),
        })
        .await?;
    let checks_summary = json!({"observation_id":provider_observation.id,"required_set_hash":required_set_hash,"status":provider_status,"expires_at":provider_observation.expires_at});

    if head_sha != expected_head {
        if !merged && pull_request_state == "closed" {
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "pull_request_closed",
                    None,
                    None,
                    None,
                    None,
                    Some(&checks_summary),
                    "controller:repo-mode",
                    "drifted pull request was closed without merge",
                )
                .await?;
            let subject_response = match &subject {
                SourceDeliverySubject::WorkItem(work_item_id) => {
                    let item = state
                        .store
                        .update_repo_work_item_status(
                            work_item_id,
                            "blocked",
                            "controller:repo-mode",
                            "drifted source pull request is closed; explicit replan is available",
                            false,
                        )
                        .await?;
                    json!({"work_item":item})
                }
                SourceDeliverySubject::Onboarding(onboarding_id) => {
                    let onboarding = state
                        .store
                        .update_repository_onboarding_source_delivery(
                            onboarding_id,
                            &intent.id,
                            "blocked",
                            None,
                            "controller:repo-mode",
                            "drifted onboarding pull request was closed; start a new onboarding",
                        )
                        .await?;
                    json!({"onboarding":onboarding})
                }
            };
            return Ok(Json(json!({
                "source_delivery_intent":intent,
                "subject":subject_response,
                "provider_checks":provider_observation,
            })));
        }
        let terminal = merged;
        if terminal {
            if let SourceDeliverySubject::WorkItem(work_item_id) = &subject {
                seal_source_delivery_closure(
                    &state,
                    work_item_id,
                    &intent,
                    &provider_observation,
                    "failed",
                    "merged pull-request head does not match approved source provenance",
                    request.merge_commit_sha.as_deref(),
                )
                .await?;
            }
        }
        let drift_provenance = terminal.then(|| {
            json!({
                "merge_commit_sha":request.merge_commit_sha,
                "head_sha":head_sha,
            })
        });
        let intent = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                if terminal { "failed" } else { "head_drift" },
                None,
                None,
                None,
                drift_provenance.as_ref(),
                Some(&checks_summary),
                "controller:repo-mode",
                "pull-request head drifted from approved provenance",
            )
            .await?;
        let subject_response = match &subject {
            SourceDeliverySubject::WorkItem(work_item_id) => {
                let item = state
                    .store
                    .update_repo_work_item_status(
                        work_item_id,
                        if terminal { "failed" } else { "blocked" },
                        "controller:repo-mode",
                        if terminal {
                            "merged source provenance does not match the approved ChangeSet"
                        } else {
                            "unapproved pull-request head drift; close the PR before correction"
                        },
                        terminal,
                    )
                    .await?;
                json!({"work_item":item})
            }
            SourceDeliverySubject::Onboarding(onboarding_id) => {
                let onboarding = state.store.update_repository_onboarding_source_delivery(
                    onboarding_id, &intent.id, if terminal { "delivery_failed" } else { "blocked" },
                    request.merge_commit_sha.as_deref(), "controller:repo-mode",
                    if terminal { "merged onboarding head does not match approved proposal provenance" } else { "unapproved onboarding pull-request head drift; close the PR before correction" },
                ).await?;
                json!({"onboarding":onboarding})
            }
        };
        return Ok(Json(
            json!({"source_delivery_intent":intent,"subject":subject_response,"provider_checks":provider_observation}),
        ));
    }
    if !merged {
        let next_status = if pull_request_state == "closed" {
            "pull_request_closed"
        } else if provider_status == "passing" {
            "waiting_merge"
        } else {
            "waiting_checks"
        };
        let intent = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                next_status,
                None,
                None,
                None,
                None,
                Some(&checks_summary),
                "agent:git-observer",
                "fresh pre-merge provider observation recorded",
            )
            .await?;
        let subject_response = match &subject {
            SourceDeliverySubject::WorkItem(work_item_id) => {
                let item = state
                    .store
                    .update_repo_work_item_status(
                        work_item_id,
                        if pull_request_state == "closed" {
                            "blocked"
                        } else {
                            "waiting_external"
                        },
                        "controller:repo-mode",
                        if pull_request_state == "closed" {
                            "source pull request closed without merge"
                        } else {
                            "manual merge and provider checks remain external"
                        },
                        false,
                    )
                    .await?;
                json!({"work_item":item})
            }
            SourceDeliverySubject::Onboarding(onboarding_id) => {
                let onboarding_status = if pull_request_state == "closed" {
                    "blocked"
                } else if provider_status == "passing" {
                    "waiting_merge"
                } else {
                    "waiting_checks"
                };
                let onboarding = state
                    .store
                    .update_repository_onboarding_source_delivery(
                        onboarding_id,
                        &intent.id,
                        onboarding_status,
                        None,
                        "controller:repo-mode",
                        if pull_request_state == "closed" {
                            "onboarding pull request closed without merge"
                        } else {
                            "manual onboarding merge and provider checks remain external"
                        },
                    )
                    .await?;
                json!({"onboarding":onboarding})
            }
        };
        return Ok(Json(
            json!({"source_delivery_intent":intent,"subject":subject_response,"provider_checks":provider_observation}),
        ));
    }
    let merge_sha = request
        .merge_commit_sha
        .as_deref()
        .filter(|sha| is_git_sha(sha));
    let pre_merge = state
        .store
        .latest_provider_check_set_observation(&intent.id, "pre_merge")
        .await?;
    let current = current_millis();
    let delivery_succeeded = pull_request_state == "closed"
        && merge_sha.is_some()
        && provider_status == "passing"
        && pre_merge.as_ref().is_some_and(|observation| {
            observation.authoritative_rules_succeeded
                && observation.status == "passing"
                && observation.head_sha == head_sha
                && observation.required_set_hash == required_set_hash
                && observation
                    .expires_at
                    .parse::<u128>()
                    .is_ok_and(|expiry| expiry >= current)
        });
    let terminal_status = if delivery_succeeded {
        "succeeded"
    } else {
        "failed"
    };
    let stop_reason = if delivery_succeeded {
        "manual merge matched the approved head and fresh authoritative required checks"
    } else {
        "merge occurred without matching fresh passing pre-merge provider evidence"
    };
    if let SourceDeliverySubject::WorkItem(work_item_id) = &subject {
        seal_source_delivery_closure(
            &state,
            work_item_id,
            &intent,
            &provider_observation,
            terminal_status,
            stop_reason,
            merge_sha,
        )
        .await?;
    }
    let provenance = json!({
        "pull_request":pull_request,
        "head_sha":head_sha,
        "merge_commit_sha":merge_sha,
        "required_set_hash":required_set_hash,
        "pre_merge_observation_id":pre_merge.as_ref().map(|observation| &observation.id),
        "merge_observation_id":provider_observation.id,
        "status":terminal_status,
    });
    let intent = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            if delivery_succeeded {
                "merged"
            } else {
                "failed"
            },
            None,
            None,
            None,
            Some(&provenance),
            Some(&checks_summary),
            "controller:repo-mode",
            stop_reason,
        )
        .await?;
    let subject_response = match &subject {
        SourceDeliverySubject::WorkItem(work_item_id) => {
            let hosted = repo_metadata(&state, work_item_id)
                .await?
                .workflow_policy
                .is_some();
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    if delivery_succeeded && hosted {
                        "executing"
                    } else if delivery_succeeded {
                        "completed"
                    } else {
                        "failed"
                    },
                    "controller:repo-mode",
                    if delivery_succeeded && hosted {
                        "source merge verified; build, deployment, production approval, and runtime verification remain required"
                    } else {
                        stop_reason
                    },
                    !delivery_succeeded || !hosted,
                )
                .await?;
            json!({"work_item":item})
        }
        SourceDeliverySubject::Onboarding(onboarding_id) => {
            let onboarding = state.store.update_repository_onboarding_source_delivery(
                onboarding_id, &intent.id,
                if delivery_succeeded { "merge_observed" } else { "delivery_failed" },
                merge_sha, "controller:repo-mode",
                if delivery_succeeded { "onboarding merge matched approved provenance; canonical contract validation is required" } else { stop_reason },
            ).await?;
            json!({"onboarding":onboarding})
        }
    };
    Ok(Json(
        json!({"source_delivery_intent":intent,"subject":subject_response,"provider_checks":provider_observation,"delivery_status":terminal_status}),
    ))
}

async fn current_source_delivery_writer(
    state: &AppState,
    intent_id: &str,
    execution_id: &str,
) -> Result<StoredSourceDeliveryIntent, ApiError> {
    state
        .store
        .get_source_delivery_intent(intent_id)
        .await?
        .filter(|intent| {
            intent.status == "writer_dispatched"
                && intent.writer_execution_id.as_deref() == Some(execution_id)
        })
        .ok_or_else(|| ApiError::conflict("source delivery writer execution is not current"))
}

async fn current_source_delivery_observer(
    state: &AppState,
    intent_id: &str,
    execution_id: &str,
) -> Result<StoredSourceDeliveryIntent, ApiError> {
    state
        .store
        .get_source_delivery_intent(intent_id)
        .await?
        .filter(|intent| {
            intent.status == "observer_dispatched"
                && intent.observer_execution_id.as_deref() == Some(execution_id)
        })
        .ok_or_else(|| ApiError::conflict("source delivery observer execution is not current"))
}

enum SourceDeliverySubject {
    WorkItem(String),
    Onboarding(String),
}

async fn source_delivery_subject(
    state: &AppState,
    intent: &StoredSourceDeliveryIntent,
) -> Result<SourceDeliverySubject, ApiError> {
    match intent.subject_kind.as_str() {
        "work_item_change_set" => state
            .store
            .get_change_set(&intent.subject_id)
            .await?
            .and_then(|change_set| change_set.work_item_id)
            .map(SourceDeliverySubject::WorkItem)
            .ok_or_else(|| {
                ApiError::conflict("SourceDeliveryIntent WorkItem provenance is unavailable")
            }),
        "repository_onboarding_proposal" => {
            let proposal = state
                .store
                .get_repository_onboarding_proposal(&intent.subject_id)
                .await?
                .filter(|proposal| proposal.status == "approved")
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding proposal is unavailable")
                })?;
            state
                .store
                .get_repository_onboarding(&proposal.onboarding_id)
                .await?
                .filter(|onboarding| {
                    onboarding.source_delivery_intent_id.as_deref() == Some(intent.id.as_str())
                        && onboarding.approved_proposal_hash.as_deref()
                            == Some(proposal.content_hash.as_str())
                })
                .map(|onboarding| SourceDeliverySubject::Onboarding(onboarding.id))
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding provenance is unavailable")
                })
        }
        _ => Err(ApiError::conflict(
            "SourceDeliveryIntent subject kind is unsupported",
        )),
    }
}

pub(super) fn derive_provider_check_status(
    required_checks: &Value,
) -> Result<&'static str, ApiError> {
    let checks = required_checks
        .as_array()
        .ok_or_else(|| ApiError::bad_request("required_checks must be an array"))?;
    if checks.len() > 100 {
        return Err(ApiError::bad_request(
            "required_checks exceeds the bounded provider inventory",
        ));
    }
    let mut status = "passing";
    for check in checks {
        match check.get("status").and_then(Value::as_str) {
            Some("failed") => return Ok("failed"),
            Some("passing") => {}
            Some("pending") => status = "pending",
            _ => {
                return Err(ApiError::bad_request(
                    "required check has an invalid status",
                ))
            }
        }
    }
    Ok(status)
}

async fn seal_source_delivery_closure(
    state: &AppState,
    work_item_id: &str,
    intent: &StoredSourceDeliveryIntent,
    provider: &pharness_store::StoredProviderCheckSetObservation,
    status: &str,
    stop_reason: &str,
    merge_commit_sha: Option<&str>,
) -> Result<(), ApiError> {
    let existing = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    if existing
        .iter()
        .any(|outcome| outcome.stage_key == "source_delivery")
    {
        seal_repo_inapplicable_tail(&state.store, work_item_id).await?;
        return Ok(());
    }
    let input = json!({
        "source_delivery_intent_id":intent.id,
        "subject_kind":intent.subject_kind,
        "subject_id":intent.subject_id,
        "base_commit":intent.base_commit,
        "approved_head_sha":intent.pull_request.as_ref().and_then(|pr| pr.get("head_sha")),
        "provider_check_observation_id":provider.id,
        "provider_check_observation_hash":provider.content_hash,
        "merge_commit_sha":merge_commit_sha,
    });
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: new_prefixed_id("stageexec"),
            work_item_id: work_item_id.into(),
            stage_key: "source_delivery".into(),
            sequence: 1,
            status: status.into(),
            agent_profile_id: None,
            agent_profile_version: None,
            agent_profile_hash: None,
            context_pack_id: None,
            run_id: None,
            workspace_id: None,
            input_hash: canonical_material_hash(&input)?,
            input_snapshot: input.clone(),
        })
        .await?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let outcome = json!({
        "schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,
        "work_item_id":work_item_id,
        "stage_execution_id":execution.id,
        "stage":"source_delivery",
        "status":status,
        "objective":{"kind":"deliver_reviewed_source_change"},
        "pinned_inputs":input,
        "verified_facts":[{"kind":"provider_check_set","id":provider.id,"hash":provider.content_hash,"status":provider.status}],
        "agent_claims":[],
        "outputs":[{"kind":"source_delivery_intent","id":intent.id}],
        "acceptance":[],"decisions":[],"authorizations":[intent.authorization],
        "contradictions":if status == "succeeded" {json!([])} else {json!([{"kind":"source_delivery_failure","reason":stop_reason}])},
        "risks":[],"unavailable_capabilities":[],"recommendations":[],
        "stop_reason":stop_reason,"sealed_state_version":metadata.state_version,
    });
    state.store.create_evidence_validation(CreateEvidenceValidation {
        id:new_prefixed_id("evalid"), work_item_id:work_item_id.into(), stage_execution_id:Some(execution.id.clone()),
        validator_key:"source_delivery_merge_provenance".into(), status:if status == "succeeded" {"valid".into()} else {"invalid".into()},
        subject:json!({"source_delivery_intent_id":intent.id}),
        evidence_refs:json!([{"kind":"provider_check_set_observation","id":provider.id,"hash":provider.content_hash}]),
        facts:json!({"head_sha":provider.head_sha,"required_set_hash":provider.required_set_hash,"merge_commit_sha":merge_commit_sha}),
        contradictions:outcome.get("contradictions").cloned().unwrap_or_else(|| json!([])),
        content_hash:canonical_material_hash(&json!({"provider":provider.content_hash,"status":status,"merge_commit_sha":merge_commit_sha}))?,
    }).await?;
    state
        .store
        .seal_stage_outcome(SealStageOutcome {
            id: new_prefixed_id("stageout"),
            stage_execution_id: execution.id,
            work_item_id: work_item_id.into(),
            stage_key: "source_delivery".into(),
            status: status.into(),
            content_hash: canonical_material_hash(&outcome)?,
            outcome,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            effective: true,
            actor: "controller:repo-mode".into(),
            reason: stop_reason.into(),
        })
        .await?;
    seal_repo_inapplicable_tail(&state.store, work_item_id).await?;
    Ok(())
}
