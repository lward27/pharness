use super::super::*;

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
