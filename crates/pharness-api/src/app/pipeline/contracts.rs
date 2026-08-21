use super::super::audit::append_pipeline_contract_audit_event;
use super::super::auth::OperatorIdentity;
use super::super::clock::unique_suffix;
use super::super::validation::{clean_optional_text, required_text, validate_kubernetes_name};
use super::super::{ApiError, AppState};
use crate::dto::{
    CreatePipelineContractRequest, PipelineContractResponse, PipelineContractsResponse,
    ReplacePipelineContractRequest, ReplacePipelineContractResponse,
    TransitionPipelineContractRequest,
};
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_store::{CreatePipelineContract, PipelineContractListFilter, ReplacePipelineContract};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineContractSpec {
    #[serde(default)]
    pub(in crate::app) params: Vec<PipelineParameterContract>,
    #[serde(default)]
    pub(in crate::app) workspaces: Vec<PipelineWorkspaceContract>,
    #[serde(default)]
    pub(in crate::app) source_revision_param: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineParameterContract {
    pub(in crate::app) name: String,
    #[serde(rename = "type")]
    pub(in crate::app) value_type: String,
    #[serde(default)]
    pub(in crate::app) required: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineWorkspaceContract {
    pub(in crate::app) name: String,
    pub(in crate::app) binding: String,
    #[serde(default)]
    pub(in crate::app) required: bool,
}

pub(in crate::app) fn pipeline_contract_spec(
    value: &Value,
) -> Result<PipelineContractSpec, ApiError> {
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "pipeline contract contract_json must be a JSON object",
        ));
    }
    serde_json::from_value::<PipelineContractSpec>(value.clone()).map_err(|error| {
        ApiError::bad_request(format!(
            "pipeline contract contract_json is invalid: {error}"
        ))
    })
}

pub(in crate::app) fn validate_pipeline_contract_spec(
    contract: &PipelineContractSpec,
) -> Result<(), ApiError> {
    let mut names = BTreeSet::new();
    for parameter in &contract.params {
        validate_kubernetes_name("pipeline contract params.name", &parameter.name)?;
        if !matches!(parameter.value_type.as_str(), "scalar" | "array") {
            return Err(ApiError::bad_request(
                "pipeline contract params.type must be scalar or array",
            ));
        }
        if !names.insert(parameter.name.as_str()) {
            return Err(ApiError::bad_request(
                "pipeline contract params must not repeat a name",
            ));
        }
    }
    let mut workspace_names = BTreeSet::new();
    for workspace in &contract.workspaces {
        validate_kubernetes_name("pipeline contract workspaces.name", &workspace.name)?;
        if !matches!(
            workspace.binding.as_str(),
            "persistent_volume_claim" | "volume_claim_template"
        ) {
            return Err(ApiError::bad_request(
                "pipeline contract workspaces.binding must be persistent_volume_claim or volume_claim_template",
            ));
        }
        if !workspace_names.insert(workspace.name.as_str()) {
            return Err(ApiError::bad_request(
                "pipeline contract workspaces must not repeat a name",
            ));
        }
    }
    if let Some(source_revision_param) = &contract.source_revision_param {
        validate_kubernetes_name(
            "pipeline contract source_revision_param",
            source_revision_param,
        )?;
        let parameter = contract
            .params
            .iter()
            .find(|parameter| parameter.name == *source_revision_param)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "pipeline contract source_revision_param must name a declared parameter",
                )
            })?;
        if !parameter.required || parameter.value_type != "scalar" {
            return Err(ApiError::bad_request(
                "pipeline contract source_revision_param must name a required scalar parameter",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListPipelineContractsQuery {
    pub(in crate::app) namespace: Option<String>,
    pub(in crate::app) pipeline_ref: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_pipeline_contracts(
    State(state): State<AppState>,
    Query(query): Query<ListPipelineContractsQuery>,
) -> Result<Json<PipelineContractsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let pipeline_contracts = state
        .store
        .list_pipeline_contracts(PipelineContractListFilter {
            namespace: clean_optional_text(query.namespace),
            pipeline_ref: clean_optional_text(query.pipeline_ref),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = pipeline_contracts.len();
    Ok(Json(PipelineContractsResponse {
        pipeline_contracts,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_pipeline_contract(
    State(state): State<AppState>,
    Path(pipeline_contract_id): Path<String>,
) -> Result<Json<PipelineContractResponse>, ApiError> {
    let contract = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    Ok(Json(contract.into()))
}

pub(in crate::app) async fn create_pipeline_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreatePipelineContractRequest>,
) -> Result<Json<PipelineContractResponse>, ApiError> {
    let namespace = required_text(request.namespace, "namespace")?;
    let pipeline_ref = required_text(request.pipeline_ref, "pipeline_ref")?;
    let version = clean_optional_text(request.version).unwrap_or_else(|| "v1".to_string());
    validate_kubernetes_name("namespace", &namespace)?;
    validate_kubernetes_name("pipeline_ref", &pipeline_ref)?;
    validate_kubernetes_name("version", &version)?;
    let contract = pipeline_contract_spec(&request.contract_json)?;
    validate_pipeline_contract_spec(&contract)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .create_pipeline_contract(CreatePipelineContract {
            id: format!("pcontract_{}", unique_suffix()),
            status: "active".to_string(),
            namespace,
            pipeline_ref,
            version,
            contract_json: request.contract_json,
            actor: actor.clone(),
            reason: reason.clone(),
        })
        .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &contract,
        "pipeline_contract.created",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

pub(in crate::app) async fn transition_pipeline_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(pipeline_contract_id): Path<String>,
    Json(request): Json<TransitionPipelineContractRequest>,
) -> Result<Json<PipelineContractResponse>, ApiError> {
    let current = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    let target = required_text(request.target_status, "target_status")?;
    if current.status != "active" || target != "retired" {
        return Err(ApiError::conflict(format!(
            "PipelineContract can only transition from active to retired, not {} to {}",
            current.status, target
        )));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .update_pipeline_contract_status(&current.id, "retired", actor.clone(), reason.clone())
        .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &contract,
        "pipeline_contract.retired",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

pub(in crate::app) async fn replace_pipeline_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(pipeline_contract_id): Path<String>,
    Json(request): Json<ReplacePipelineContractRequest>,
) -> Result<Json<ReplacePipelineContractResponse>, ApiError> {
    let current = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    if current.status != "active" {
        return Err(ApiError::conflict(
            "only an active PipelineContract can be replaced",
        ));
    }
    let version = required_text(request.version, "version")?;
    validate_kubernetes_name("version", &version)?;
    if version == current.version {
        return Err(ApiError::conflict(
            "replacement PipelineContract version must differ from the active version",
        ));
    }
    let contract_spec = pipeline_contract_spec(&request.contract_json)?;
    validate_pipeline_contract_spec(&contract_spec)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let (retired_contract, pipeline_contract) = state
        .store
        .replace_pipeline_contract(
            &current.id,
            ReplacePipelineContract {
                id: format!("pcontract_{}", unique_suffix()),
                namespace: current.namespace.clone(),
                pipeline_ref: current.pipeline_ref.clone(),
                version,
                contract_json: request.contract_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &retired_contract,
        "pipeline_contract.replaced",
        actor.clone(),
        reason.clone(),
    )
    .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &pipeline_contract,
        "pipeline_contract.created_by_replacement",
        actor,
        reason,
    )
    .await?;
    Ok(Json(ReplacePipelineContractResponse {
        retired_contract: retired_contract.into(),
        pipeline_contract: pipeline_contract.into(),
    }))
}
