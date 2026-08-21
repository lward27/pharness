use super::super::audit::append_deployment_contract_audit_event;
use super::super::auth::OperatorIdentity;
use super::super::clock::unique_suffix;
use super::super::system::{
    PROTECTED_ARGO_APPLICATION, PROTECTED_ENVIRONMENT, PROTECTED_NAMESPACE,
    PROTECTED_WORKLOAD_KIND, PROTECTED_WORKLOAD_NAME,
};
use super::super::validation::{clean_optional_text, required_text, validate_kubernetes_name};
use super::super::{ApiError, AppState};
use crate::dto::{
    CreateDeploymentContractRequest, DeploymentContractResponse, DeploymentContractsResponse,
    TransitionDeploymentContractRequest,
};
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_store::{CreateDeploymentContract, DeploymentContractListFilter};
use serde_json::Value;

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListDeploymentContractsQuery {
    pub(in crate::app) target_environment: Option<String>,
    pub(in crate::app) target_namespace: Option<String>,
    pub(in crate::app) argo_application: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_deployment_contracts(
    State(state): State<AppState>,
    Query(query): Query<ListDeploymentContractsQuery>,
) -> Result<Json<DeploymentContractsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let deployment_contracts = state
        .store
        .list_deployment_contracts(DeploymentContractListFilter {
            target_environment: clean_optional_text(query.target_environment),
            target_namespace: clean_optional_text(query.target_namespace),
            argo_application: clean_optional_text(query.argo_application),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = deployment_contracts.len();
    Ok(Json(DeploymentContractsResponse {
        deployment_contracts,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_deployment_contract(
    State(state): State<AppState>,
    Path(deployment_contract_id): Path<String>,
) -> Result<Json<DeploymentContractResponse>, ApiError> {
    let contract = state
        .store
        .get_deployment_contract(&deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_contract", &deployment_contract_id))?;
    Ok(Json(contract.into()))
}

pub(in crate::app) async fn create_deployment_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreateDeploymentContractRequest>,
) -> Result<Json<DeploymentContractResponse>, ApiError> {
    let target_environment = required_text(request.target_environment, "target_environment")?;
    let target_namespace = required_text(request.target_namespace, "target_namespace")?;
    let argo_application = required_text(request.argo_application, "argo_application")?;
    let version = clean_optional_text(request.version).unwrap_or_else(|| "v1".to_string());
    validate_kubernetes_name("target_environment", &target_environment)?;
    validate_kubernetes_name("target_namespace", &target_namespace)?;
    validate_kubernetes_name("argo_application", &argo_application)?;
    validate_kubernetes_name("version", &version)?;
    let contract_spec = deployment_contract_spec(&request.contract_json)?;
    validate_deployment_contract_spec(&contract_spec)?;
    if target_environment == PROTECTED_ENVIRONMENT
        || target_namespace == PROTECTED_NAMESPACE
        || argo_application == PROTECTED_ARGO_APPLICATION
    {
        if target_environment != PROTECTED_ENVIRONMENT
            || target_namespace != PROTECTED_NAMESPACE
            || argo_application != PROTECTED_ARGO_APPLICATION
        {
            return Err(ApiError::bad_request(
                "production DeploymentContract target must exactly match production/apps-prod/yfinance-wrapper",
            ));
        }
        validate_protected_production_deployment_contract(&contract_spec)?;
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .create_deployment_contract(CreateDeploymentContract {
            id: format!("dcontract_{}", unique_suffix()),
            status: "active".to_string(),
            target_environment,
            target_namespace,
            argo_application,
            version,
            contract_json: request.contract_json,
            actor: actor.clone(),
            reason: reason.clone(),
        })
        .await?;
    append_deployment_contract_audit_event(
        &state.store,
        &contract,
        "deployment_contract.created",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

pub(in crate::app) async fn transition_deployment_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(deployment_contract_id): Path<String>,
    Json(request): Json<TransitionDeploymentContractRequest>,
) -> Result<Json<DeploymentContractResponse>, ApiError> {
    let current = state
        .store
        .get_deployment_contract(&deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_contract", &deployment_contract_id))?;
    let target = required_text(request.target_status, "target_status")?;
    if current.status != "active" || target != "retired" {
        return Err(ApiError::conflict(format!(
            "DeploymentContract can only transition from active to retired, not {} to {}",
            current.status, target
        )));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .update_deployment_contract_status(&current.id, "retired", actor.clone(), reason.clone())
        .await?;
    append_deployment_contract_audit_event(
        &state.store,
        &contract,
        "deployment_contract.retired",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct DeploymentContractSpec {
    pub(in crate::app) operation: String,
    #[serde(default)]
    pub(in crate::app) prune: bool,
    #[serde(default)]
    pub(in crate::app) force: bool,
    #[serde(default)]
    pub(in crate::app) workload_kind: Option<String>,
    #[serde(default)]
    pub(in crate::app) workload_name: Option<String>,
    #[serde(default)]
    pub(in crate::app) service_name: Option<String>,
    #[serde(default)]
    pub(in crate::app) service_port: Option<u16>,
    #[serde(default)]
    pub(in crate::app) health_path: Option<String>,
    #[serde(default)]
    pub(in crate::app) post_sync_verification: PostSyncVerificationContract,
}

/// Explicit, deliberately small runtime-verification policy attached to an
/// exact DeploymentContract. More observability sources can be added here as
/// independently reviewed contract fields; this is not an arbitrary query
/// escape hatch.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PostSyncVerificationContract {
    #[serde(default)]
    pub(in crate::app) service_healthz: VerificationRequirement,
    #[serde(default)]
    pub(in crate::app) prometheus_inventory: VerificationRequirement,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::app) enum VerificationRequirement {
    #[default]
    Disabled,
    Required,
}

pub(in crate::app) fn deployment_contract_spec(
    value: &Value,
) -> Result<DeploymentContractSpec, ApiError> {
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "deployment contract contract_json must be a JSON object",
        ));
    }
    serde_json::from_value::<DeploymentContractSpec>(value.clone()).map_err(|error| {
        ApiError::bad_request(format!(
            "deployment contract contract_json is invalid: {error}"
        ))
    })
}

pub(in crate::app) fn validate_deployment_contract_spec(
    contract: &DeploymentContractSpec,
) -> Result<(), ApiError> {
    if contract.operation != "sync" {
        return Err(ApiError::bad_request(
            "deployment contract operation must be sync",
        ));
    }
    if contract.prune || contract.force {
        return Err(ApiError::bad_request(
            "deployment contract prune and force must remain false",
        ));
    }
    Ok(())
}

pub(in crate::app) fn validate_protected_production_deployment_contract(
    contract: &DeploymentContractSpec,
) -> Result<(), ApiError> {
    if contract.workload_kind.as_deref() != Some(PROTECTED_WORKLOAD_KIND)
        || contract.workload_name.as_deref() != Some(PROTECTED_WORKLOAD_NAME)
        || contract.service_name.as_deref() != Some(PROTECTED_WORKLOAD_NAME)
        || contract.service_port != Some(8090)
        || contract.health_path.as_deref() != Some("/healthz")
        || contract.post_sync_verification.service_healthz != VerificationRequirement::Required
    {
        return Err(ApiError::bad_request(
            "protected production DeploymentContract must pin Deployment/yfinance-wrapper and the exact yfinance-wrapper:8090/healthz check",
        ));
    }
    Ok(())
}
