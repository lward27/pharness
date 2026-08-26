use super::clock::current_millis;
use super::products::ensure_repo_mode_enabled;
use super::system::{capability_statuses, environment_profile_capability_status};
use super::{ApiError, AppState};
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use pharness_store::{RunListFilter, StoredWorkItem, WorkItemListFilter};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/products/:product_id/overview", get(product_overview))
        .route("/api/repositories", get(list_repositories))
        .route(
            "/api/repositories/:repository_id/overview",
            get(repository_overview),
        )
        .route("/api/search", get(search))
}

#[derive(Debug, Default, Deserialize)]
struct PageQuery {
    product_id: Option<String>,
    search: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<u32>,
}

fn text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn all_work_items(
    state: &AppState,
    mut filter: WorkItemListFilter,
) -> Result<Vec<StoredWorkItem>, ApiError> {
    let total = state.store.count_work_items(filter.clone()).await?;
    let mut items = Vec::with_capacity(total);
    filter.limit = 200;
    let mut offset = 0u32;
    while items.len() < total {
        filter.offset = offset;
        let page = state.store.list_work_items(filter.clone()).await?;
        if page.is_empty() {
            break;
        }
        offset = offset.saturating_add(page.len() as u32);
        items.extend(page);
    }
    Ok(items)
}

async fn work_item_summary(state: &AppState, item: StoredWorkItem) -> Result<Value, ApiError> {
    let metadata = state.store.get_repo_work_item_metadata(&item.id).await?;
    let current_execution = match metadata
        .as_ref()
        .and_then(|metadata| metadata.current_stage_execution_id.as_deref())
    {
        Some(id) => state.store.get_stage_execution(id).await?,
        None => None,
    };
    let effective_outcome = match current_execution.as_ref() {
        Some(execution) => {
            state
                .store
                .get_stage_outcome_for_execution(&execution.id)
                .await?
        }
        None => None,
    };
    let active_run = match item.current_run_id.as_ref() {
        Some(run_id) => state.store.get_run(run_id).await?,
        None => None,
    };
    Ok(json!({
        "id": item.id,
        "title": item.title,
        "intent": item.intent,
        "status": item.status,
        "status_reason": item.status_reason,
        "source_commit": item.source_commit,
        "mode": metadata.as_ref().map(|value| value.mode.as_str()),
        "product_id": metadata.as_ref().map(|value| value.product_id.as_str()),
        "repository_id": metadata.as_ref().map(|value| value.repository_id.as_str()),
        "closed_at": metadata.as_ref().and_then(|value| value.closed_at.as_deref()),
        "closure_reason": metadata.as_ref().and_then(|value| value.closure_reason.as_deref()),
        "current_stage": current_execution.as_ref().map(|value| value.stage_key.as_str()).unwrap_or("discover"),
        "stage_execution": current_execution,
        "effective_outcome": effective_outcome,
        "active_agent_run": active_run.as_ref().filter(|run| run.finished_at.is_none()).map(|run| json!({
            "id":run.id,
            "status":run.status,
            "profile_id":run.execution_target_json.pointer("/agent_profile/id"),
            "stage_execution_id":run.execution_target_json.pointer("/repo_mode/stage_execution_id"),
        })),
        "updated_at": item.updated_at,
    }))
}

pub(in crate::app) async fn organization_overview_value(
    state: &AppState,
    organization: &pharness_store::StoredOrganization,
) -> Result<Value, ApiError> {
    let products = state.store.list_products(&organization.id).await?;
    let repo_items = all_work_items(
        state,
        WorkItemListFilter {
            mode: Some("repo".into()),
            ..Default::default()
        },
    )
    .await?;
    let legacy_items = all_work_items(
        state,
        WorkItemListFilter {
            mode: Some("legacy".into()),
            ..Default::default()
        },
    )
    .await?;
    let source_capability_posture = capability_statuses(state)
        .await?
        .into_iter()
        .filter(|capability| {
            matches!(
                capability.capability.as_str(),
                "source_reader" | "source_writer" | "source_observer"
            )
        })
        .collect::<Vec<_>>();
    let mut repo_metadata_by_item = BTreeMap::new();
    let repo_status_by_item = repo_items
        .iter()
        .map(|item| (item.id.clone(), item.status.clone()))
        .collect::<BTreeMap<_, _>>();
    for item in &repo_items {
        if let Some(metadata) = state.store.get_repo_work_item_metadata(&item.id).await? {
            repo_metadata_by_item.insert(item.id.clone(), metadata);
        }
    }
    let mut product_summaries = Vec::with_capacity(products.len());
    let mut repository_ids = BTreeSet::new();
    let mut readiness_gap_repository_ids = BTreeSet::new();
    let mut readiness_gap_repositories = Vec::new();
    for product in &products {
        let repositories = state.store.list_product_repositories(&product.id).await?;
        repository_ids.extend(repositories.iter().map(|repository| repository.id.clone()));
        let product_items = repo_items
            .iter()
            .filter(|item| {
                repo_metadata_by_item
                    .get(&item.id)
                    .is_some_and(|metadata| metadata.product_id == product.id)
            })
            .collect::<Vec<_>>();
        let latest_work_item_update = product_items
            .iter()
            .map(|item| item.updated_at.as_str())
            .max();
        let current = product_items
            .iter()
            .filter(|item| {
                repo_metadata_by_item
                    .get(&item.id)
                    .is_some_and(|metadata| metadata.closed_at.is_none())
            })
            .count();
        let attention = product_items
            .iter()
            .filter(|item| {
                repo_metadata_by_item
                    .get(&item.id)
                    .is_some_and(|metadata| metadata.closed_at.is_none())
                    && matches!(item.status.as_str(), "blocked" | "waiting_external")
            })
            .count();
        product_summaries.push(json!({
            "id":product.id,
            "product_key":product.product_key,
            "display_name":product.display_name,
            "owner_principal":product.owner_principal,
            "repository_count":repositories.len(),
            "current_work_items":current,
            "actionable_waits":attention,
            "evidence_freshness":{"latest_work_item_update":latest_work_item_update,"as_of":current_millis().to_string()},
            "capability_posture":&source_capability_posture,
            "updated_at":product.updated_at,
        }));
        for repository in repositories {
            let assessment = state
                .store
                .latest_repository_readiness_assessment(
                    &repository.id,
                    &repository.registered_commit,
                )
                .await?;
            if !assessment.as_ref().is_some_and(|assessment| {
                assessment.contract_status == "ready" && assessment.coding_status == "ready"
            }) && readiness_gap_repository_ids.insert(repository.id.clone())
            {
                readiness_gap_repositories.push(json!({
                    "repository_id":repository.id,
                    "product_id":product.id,
                    "canonical_url":repository.canonical_url,
                    "contract_status":assessment.as_ref().map(|value| value.contract_status.as_str()).unwrap_or("unavailable"),
                    "coding_status":assessment.as_ref().map(|value| value.coding_status.as_str()).unwrap_or("unavailable"),
                    "blockers":assessment.as_ref().map(|value| value.blockers.clone()).unwrap_or_else(|| json!(["assessment_missing"])),
                }));
            }
        }
    }

    let mut by_status = BTreeMap::<String, usize>::new();
    let mut by_stage = BTreeMap::<String, usize>::new();
    let mut attention = Vec::new();
    let mut active_runs = Vec::new();
    for item in repo_items {
        *by_status.entry(item.status.clone()).or_default() += 1;
        let summary = work_item_summary(state, item).await?;
        let stage = summary
            .get("current_stage")
            .and_then(Value::as_str)
            .unwrap_or("discover");
        let is_current = summary.get("closed_at").is_some_and(Value::is_null);
        if is_current {
            *by_stage.entry(stage.to_string()).or_default() += 1;
        }
        if is_current
            && matches!(
                summary.get("status").and_then(Value::as_str),
                Some("blocked" | "waiting_external")
            )
        {
            attention.push(json!({
                "kind":"work_item",
                "resource_id":summary.get("id"),
                "product_id":summary.get("product_id"),
                "repository_id":summary.get("repository_id"),
                "status":summary.get("status"),
                "reason":summary.get("status_reason"),
            }));
        }
        if let Some(run) = summary
            .get("active_agent_run")
            .filter(|value| !value.is_null())
        {
            active_runs.push(json!({
                "run":run,
                "product_id":summary.get("product_id"),
                "work_item_id":summary.get("id"),
                "repository_id":summary.get("repository_id"),
            }));
        }
    }
    let current = repo_metadata_by_item
        .values()
        .filter(|metadata| metadata.closed_at.is_none())
        .count();
    let recent_cutoff = current_millis().saturating_sub(7 * 24 * 60 * 60 * 1_000);
    let recently_completed = repo_metadata_by_item
        .iter()
        .filter(|(work_item_id, metadata)| {
            metadata
                .closed_at
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok())
                .is_some_and(|closed_at| closed_at >= recent_cutoff)
                && repo_status_by_item
                    .get(*work_item_id)
                    .is_some_and(|status| status == "completed")
        })
        .count();
    let ready_repositories = repository_ids
        .len()
        .saturating_sub(readiness_gap_repositories.len());
    Ok(json!({
        "organization":{
            "id":organization.id,
            "organization_key":organization.organization_key,
            "display_name":organization.display_name,
            "repo_mode_v1_enabled":state.repo_mode.enabled,
            "repo_mode_v1_ui_enabled":state.repo_mode.ui_enabled,
        },
        "products":products.len(),
        "repositories":repository_ids.len(),
        "product_summaries":product_summaries,
        "work_items":{
            "current":current,
            "waiting":by_status.get("waiting_external").copied().unwrap_or(0),
            "blocked":by_status.get("blocked").copied().unwrap_or(0),
            "failed":by_status.get("failed").copied().unwrap_or(0),
            "recently_completed":recently_completed,
            "by_lifecycle_boundary":by_stage,
            "denominator":by_status.values().sum::<usize>(),
        },
        "attention":attention,
        "active_agent_runs":active_runs,
        "repository_readiness_gaps":readiness_gap_repositories,
        "repository_readiness_rate":{
            "ready":ready_repositories,
            "total":repository_ids.len(),
        },
        "unassigned_legacy":{
            "count":legacy_items.len(),
            "current":legacy_items.iter().filter(|item| !matches!(item.status.as_str(), "completed" | "failed" | "cancelled")).count(),
            "history":legacy_items.iter().filter(|item| matches!(item.status.as_str(), "completed" | "failed" | "cancelled")).count(),
        },
        "as_of":current_millis().to_string(),
    }))
}

pub(in crate::app) async fn product_overview(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let product = state
        .store
        .get_product(&product_id)
        .await?
        .ok_or_else(|| ApiError::not_found("product", &product_id))?;
    let services = state.store.list_product_services(&product_id).await?;
    let repositories = repository_catalog(&state, Some(&product_id)).await?;
    let bindings = state
        .store
        .list_product_repository_bindings(&product_id)
        .await?;
    let current = all_work_items(
        &state,
        WorkItemListFilter {
            mode: Some("repo".into()),
            product_id: Some(product_id.clone()),
            lifecycle: Some("current".into()),
            ..Default::default()
        },
    )
    .await?;
    let history = all_work_items(
        &state,
        WorkItemListFilter {
            mode: Some("repo".into()),
            product_id: Some(product_id.clone()),
            lifecycle: Some("history".into()),
            ..Default::default()
        },
    )
    .await?;
    let mut current_summaries = Vec::new();
    for item in current {
        current_summaries.push(work_item_summary(&state, item).await?);
    }
    let mut history_summaries = Vec::new();
    for item in history {
        history_summaries.push(work_item_summary(&state, item).await?);
    }
    let runs = state
        .store
        .list_runs(RunListFilter {
            product_id: Some(product_id.clone()),
            lifecycle: Some("current".into()),
            limit: 200,
            ..Default::default()
        })
        .await?;
    let capabilities = capability_statuses(&state).await?;
    Ok(Json(json!({
        "product":product,
        "services":services,
        "repository_bindings":bindings,
        "repositories":repositories,
        "current_work_items":current_summaries,
        "historical_work_items":history_summaries,
        "active_agent_runs":runs.iter().map(|run| json!({"id":run.id,"status":run.status,"work_item_id":run.execution_target_json.pointer("/run_scope/work_item_id"),"stage_execution_id":run.execution_target_json.pointer("/repo_mode/stage_execution_id"),"profile_id":run.execution_target_json.pointer("/agent_profile/id")})).collect::<Vec<_>>(),
        "capability_posture":capabilities,
        "connected_release_data":{"available":false,"releases":[]},
        "evidence_freshness":{"as_of":current_millis().to_string()},
    })))
}

async fn repository_catalog(
    state: &AppState,
    product_filter: Option<&str>,
) -> Result<Vec<Value>, ApiError> {
    let capability_posture = capability_statuses(state)
        .await?
        .into_iter()
        .filter(|capability| {
            matches!(
                capability.capability.as_str(),
                "source_reader" | "source_writer" | "source_observer"
            )
        })
        .collect::<Vec<_>>();
    let products = state
        .store
        .list_products(&state.repo_mode.organization.id)
        .await?;
    let mut records = BTreeMap::<String, Value>::new();
    for product in products {
        if product_filter.is_some_and(|value| value != product.id) {
            continue;
        }
        for repository in state.store.list_product_repositories(&product.id).await? {
            let onboarding = state
                .store
                .list_repository_onboardings(&repository.id)
                .await?
                .into_iter()
                .last();
            let readiness = state
                .store
                .latest_repository_readiness_assessment(
                    &repository.id,
                    &repository.registered_commit,
                )
                .await?;
            let contract = state
                .store
                .latest_repository_contract_version(&repository.id, &repository.registered_commit)
                .await?;
            let stale_reasons = readiness_stale_reasons(readiness.as_ref(), contract.as_ref());
            let freshness = if readiness.is_none() {
                "unavailable"
            } else if stale_reasons.is_empty() {
                "current"
            } else {
                "stale"
            };
            let record = records.entry(repository.id.clone()).or_insert_with(|| json!({
                "id":repository.id,
                "provider":repository.provider,
                "provider_repository_id":repository.external_id,
                "canonical_url":repository.canonical_url,
                "default_branch":repository.default_branch,
                "registered_commit":repository.registered_commit,
                "state_version":repository.state_version,
                "product_bindings":[],
                "latest_onboarding":onboarding,
                "contract_readiness":readiness.as_ref().map(|value| value.contract_status.as_str()).unwrap_or("unavailable"),
                "coding_readiness":readiness.as_ref().map(|value| value.coding_status.as_str()).unwrap_or("unavailable"),
                "freshness":freshness,
                "stale_reasons":stale_reasons,
                "capability_posture":&capability_posture,
                "readiness":readiness,
                "updated_at":repository.updated_at,
            }));
            record
                .get_mut("product_bindings")
                .and_then(Value::as_array_mut)
                .expect("array")
                .push(json!({"product_id":product.id,"display_name":product.display_name}));
        }
    }
    Ok(records.into_values().collect())
}

async fn list_repositories(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let search = text(query.search).map(|value| value.to_ascii_lowercase());
    let mut repositories = repository_catalog(&state, text(query.product_id).as_deref()).await?;
    if let Some(search) = search.as_deref() {
        repositories.retain(|repository| {
            ["id", "canonical_url", "provider_repository_id"]
                .iter()
                .any(|key| {
                    repository
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.to_ascii_lowercase().contains(search))
                })
        });
    }
    let count = repositories.len();
    let limit = query.limit.unwrap_or(50).clamp(1, 200) as usize;
    let offset = query.offset.unwrap_or(0) as usize;
    let page = repositories
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"repositories":page,"count":count,"limit":limit,"offset":offset,"as_of":current_millis().to_string()}),
    ))
}

fn readiness_stale_reasons(
    readiness: Option<&pharness_store::StoredRepositoryReadinessAssessment>,
    contract: Option<&pharness_store::StoredRepositoryContractVersion>,
) -> Vec<String> {
    let Some(readiness) = readiness else {
        return vec!["assessment_missing".into()];
    };
    let mut reasons = Vec::new();
    if readiness
        .expires_at
        .as_deref()
        .and_then(|value| value.parse::<u128>().ok())
        .is_some_and(|expires| expires <= current_millis())
    {
        reasons.push("assessment_expired".into());
    }
    match contract {
        Some(contract)
            if readiness.contract_version_id.as_deref() != Some(contract.id.as_str()) =>
        {
            reasons.push("contract_version_changed".into())
        }
        Some(contract)
            if readiness.contract_hash.as_deref() != Some(contract.content_hash.as_str()) =>
        {
            reasons.push("contract_hash_changed".into())
        }
        None => reasons.push("canonical_contract_version_missing".into()),
        _ => {}
    }
    reasons
}

async fn repository_overview(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    let products = state
        .store
        .list_products(&state.repo_mode.organization.id)
        .await?;
    let mut bindings = Vec::new();
    for product in products {
        if let Some(binding) = state
            .store
            .get_repository_binding(&product.id, &repository_id)
            .await?
        {
            bindings.push(json!({"product":product,"binding":binding}));
        }
    }
    let onboardings = state
        .store
        .list_repository_onboardings(&repository_id)
        .await?;
    let latest_onboarding = onboardings.last();
    let contract = state
        .store
        .latest_repository_contract_version(&repository_id, &repository.registered_commit)
        .await?;
    let readiness = state
        .store
        .latest_repository_readiness_assessment(&repository_id, &repository.registered_commit)
        .await?;
    let readiness_subject_id = format!("{repository_id}:{}", repository.registered_commit);
    let readiness_preparation = state
        .store
        .latest_subject_environment_preparation("repository_readiness", &readiness_subject_id)
        .await?;
    let stale_reasons = readiness_stale_reasons(readiness.as_ref(), contract.as_ref());
    let current = all_work_items(
        &state,
        WorkItemListFilter {
            mode: Some("repo".into()),
            repository_id: Some(repository_id.clone()),
            lifecycle: Some("current".into()),
            ..Default::default()
        },
    )
    .await?;
    let history = all_work_items(
        &state,
        WorkItemListFilter {
            mode: Some("repo".into()),
            repository_id: Some(repository_id.clone()),
            lifecycle: Some("history".into()),
            ..Default::default()
        },
    )
    .await?;
    let mut current_summaries = Vec::new();
    for item in current {
        current_summaries.push(work_item_summary(&state, item).await?);
    }
    let mut history_summaries = Vec::new();
    for item in history {
        history_summaries.push(work_item_summary(&state, item).await?);
    }
    let selected_profile = contract.as_ref().and_then(|version| {
        version
            .contract
            .get("environment_profile")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let mut capabilities = capability_statuses(&state)
        .await?
        .into_iter()
        .filter(|capability| {
            matches!(
                capability.capability.as_str(),
                "source_reader" | "source_writer" | "source_observer"
            )
        })
        .collect::<Vec<_>>();
    if let Some(profile) = selected_profile {
        if let Some(status) = environment_profile_capability_status(&state, &profile).await? {
            capabilities.push(status);
        }
    }
    Ok(Json(json!({
        "repository":repository,
        "product_bindings":bindings,
        "onboardings":onboardings,
        "latest_onboarding":latest_onboarding,
        "canonical_contract":contract,
        "readiness":readiness,
        "readiness_preparation":readiness_preparation,
        "readiness_stale_reasons":stale_reasons,
        "current_work_items":current_summaries,
        "historical_work_items":history_summaries,
        "capabilities":capabilities,
        "trust_policy":{"source_reader":"configured_policy","source_writer":"separately_gated","provider_observer":"read_only"},
        "authorization":{"source_mutation":"work_item_or_onboarding_scoped","provider_observation":"action_scoped"},
        "as_of":current_millis().to_string(),
    })))
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let SearchQuery { q, limit } = query;
    let query = text(q).unwrap_or_default().to_ascii_lowercase();
    if query.len() < 2 {
        return Ok(Json(
            json!({"query":query,"results":[],"count":0,"as_of":current_millis().to_string()}),
        ));
    }
    let limit = limit.unwrap_or(20).clamp(1, 50) as usize;
    let mut results = Vec::new();
    for product in state
        .store
        .list_products(&state.repo_mode.organization.id)
        .await?
    {
        if format!(
            "{} {} {}",
            product.id, product.display_name, product.description
        )
        .to_ascii_lowercase()
        .contains(&query)
        {
            results.push(json!({"kind":"product","id":product.id,"label":product.display_name,"status":"active","ownership":{"product_id":product.id}}));
        }
    }
    for repository in repository_catalog(&state, None).await? {
        if repository.to_string().to_ascii_lowercase().contains(&query) {
            let product_ids = repository
                .get("product_bindings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|binding| binding.get("product_id").and_then(Value::as_str))
                .collect::<Vec<_>>();
            results.push(json!({"kind":"repository","id":repository.get("id"),"label":repository.get("provider_repository_id"),"status":repository.get("coding_readiness"),"ownership":{"product_ids":product_ids}}));
        }
    }
    for item in all_work_items(
        &state,
        WorkItemListFilter {
            search: Some(query.clone()),
            ..Default::default()
        },
    )
    .await?
    {
        let metadata = state.store.get_repo_work_item_metadata(&item.id).await?;
        results.push(json!({"kind":"work_item","id":item.id,"label":item.title,"status":item.status,"ownership":{"product_id":metadata.as_ref().map(|value| value.product_id.as_str()),"repository_id":metadata.as_ref().map(|value| value.repository_id.as_str())}}));
    }
    let runs = state
        .store
        .list_runs(RunListFilter {
            search: Some(query.clone()),
            limit: 200,
            ..Default::default()
        })
        .await?;
    for run in runs {
        let work_item_id = run
            .execution_target_json
            .pointer("/run_scope/work_item_id")
            .and_then(Value::as_str);
        let metadata = match work_item_id {
            Some(id) => state.store.get_repo_work_item_metadata(id).await?,
            None => None,
        };
        results.push(json!({"kind":"agent_run","id":run.id,"label":run.user_task,"status":run.status,"ownership":{"product_id":metadata.as_ref().map(|value| value.product_id.as_str()),"work_item_id":work_item_id,"repository_id":metadata.as_ref().map(|value| value.repository_id.as_str()),"stage_execution_id":run.execution_target_json.pointer("/repo_mode/stage_execution_id"),"agent_profile_id":run.execution_target_json.pointer("/agent_profile/id")}}));
    }
    results.truncate(limit);
    Ok(Json(
        json!({"query":query,"results":results,"count":results.len(),"as_of":current_millis().to_string()}),
    ))
}
