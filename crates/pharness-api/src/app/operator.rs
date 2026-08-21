use super::*;
use axum::routing::get;
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/triage", get(list_triage))
        .route("/api/triage/summary", get(triage_summary))
        .route("/api/scopes/options", get(scope_options))
}

pub(super) fn group_operator_records(
    records: impl IntoIterator<Item = (String, String, String, String, String)>,
) -> Vec<OperatorResourceGroupResponse> {
    let mut groups =
        BTreeMap::<(String, String, String), Vec<OperatorResourceGroupMemberResponse>>::new();
    for (id, label, title, resource, status) in records {
        groups
            .entry((title, resource, status))
            .or_default()
            .push(OperatorResourceGroupMemberResponse { id, label });
    }
    let mut response = groups
        .into_iter()
        .map(
            |((title, resource, status), members)| OperatorResourceGroupResponse {
                key: format!("{title}\u{1f}{resource}\u{1f}{status}"),
                title,
                resource,
                status,
                count: members.len(),
                members,
            },
        )
        .collect::<Vec<OperatorResourceGroupResponse>>();
    response.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.resource.cmp(&right.resource))
            .then_with(|| left.status.cmp(&right.status))
    });
    response
}

const OPERATOR_GROUP_PAGE_SIZE: u32 = 200;

// List responses stay paginated. Groups intentionally enumerate the full matching set so a
// repeated-record count never changes merely because the operator moved to another page.
pub(super) async fn all_runs_for_operator_groups(
    store: &SqliteStore,
    mut filter: RunListFilter,
) -> Result<Vec<RunResponse>, StoreError> {
    let mut runs = Vec::new();
    filter.limit = OPERATOR_GROUP_PAGE_SIZE;
    filter.offset = 0;
    loop {
        let page = store.list_runs(filter.clone()).await?;
        let page_len = page.len();
        runs.extend(page.into_iter().map(Into::into));
        if page_len < OPERATOR_GROUP_PAGE_SIZE as usize {
            return Ok(runs);
        }
        filter.offset = filter
            .offset
            .checked_add(OPERATOR_GROUP_PAGE_SIZE)
            .ok_or_else(|| {
                StoreError::InvalidData(
                    "operator run group pagination exceeded supported range".to_string(),
                )
            })?;
    }
}

pub(super) async fn all_work_plans_for_operator_groups(
    store: &SqliteStore,
    mut filter: WorkPlanListFilter,
) -> Result<Vec<WorkPlanResponse>, StoreError> {
    let mut work_plans = Vec::new();
    filter.limit = OPERATOR_GROUP_PAGE_SIZE;
    filter.offset = 0;
    loop {
        let page = store.list_work_plans(filter.clone()).await?;
        let page_len = page.len();
        work_plans.extend(page.into_iter().map(Into::into));
        if page_len < OPERATOR_GROUP_PAGE_SIZE as usize {
            return Ok(work_plans);
        }
        filter.offset = filter
            .offset
            .checked_add(OPERATOR_GROUP_PAGE_SIZE)
            .ok_or_else(|| {
                StoreError::InvalidData(
                    "operator WorkPlan group pagination exceeded supported range".to_string(),
                )
            })?;
    }
}

pub(super) async fn all_approval_gates_for_operator_groups(
    store: &SqliteStore,
    mut filter: ApprovalGateListFilter,
) -> Result<Vec<ApprovalGateResponse>, StoreError> {
    let mut approval_gates = Vec::new();
    filter.limit = OPERATOR_GROUP_PAGE_SIZE;
    filter.offset = 0;
    loop {
        let page = store.list_approval_gates(filter.clone()).await?;
        let page_len = page.len();
        approval_gates.extend(page.into_iter().map(Into::into));
        if page_len < OPERATOR_GROUP_PAGE_SIZE as usize {
            return Ok(approval_gates);
        }
        filter.offset = filter
            .offset
            .checked_add(OPERATOR_GROUP_PAGE_SIZE)
            .ok_or_else(|| {
                StoreError::InvalidData(
                    "operator approval gate group pagination exceeded supported range".to_string(),
                )
            })?;
    }
}

pub(super) async fn all_approvals_for_operator_groups(
    store: &SqliteStore,
    mut filter: ApprovalListFilter,
) -> Result<Vec<ApprovalResponse>, StoreError> {
    let mut approvals = Vec::new();
    filter.limit = OPERATOR_GROUP_PAGE_SIZE;
    filter.offset = 0;
    loop {
        let page = store.list_approvals(filter.clone()).await?;
        let page_len = page.len();
        approvals.extend(page.into_iter().map(Into::into));
        if page_len < OPERATOR_GROUP_PAGE_SIZE as usize {
            return Ok(approvals);
        }
        filter.offset = filter
            .offset
            .checked_add(OPERATOR_GROUP_PAGE_SIZE)
            .ok_or_else(|| {
                StoreError::InvalidData(
                    "operator approval group pagination exceeded supported range".to_string(),
                )
            })?;
    }
}

pub(super) fn operator_resource_label(
    namespace: Option<&str>,
    kind: Option<&str>,
    name: Option<&str>,
) -> String {
    [namespace, kind, name]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn run_group_resource(run: &RunResponse) -> String {
    let scope = run.scope.as_ref();
    let scoped = [
        scope.and_then(|value| value.repo.as_deref()),
        scope.and_then(|value| value.branch.as_deref()),
        scope.and_then(|value| value.namespace.as_deref()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("/");
    if scoped.is_empty() {
        run.task.clone()
    } else {
        scoped
    }
}

async fn scope_options(
    State(state): State<AppState>,
) -> Result<Json<ScopeOptionsResponse>, ApiError> {
    let work_items = state
        .store
        .list_work_items(WorkItemListFilter {
            limit: 200,
            ..WorkItemListFilter::default()
        })
        .await?;
    let workspaces = state
        .store
        .list_workspaces(WorkspaceListFilter {
            limit: 200,
            ..WorkspaceListFilter::default()
        })
        .await?;
    let gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            limit: 200,
            ..ApprovalGateListFilter::default()
        })
        .await?;
    let audit_events = state.store.list_audit_events(None, None, None, 200).await?;
    let runs = state
        .store
        .list_runs(RunListFilter {
            limit: 200,
            ..RunListFilter::default()
        })
        .await?;
    let approvals = state
        .store
        .list_approvals(ApprovalListFilter {
            limit: 200,
            ..ApprovalListFilter::default()
        })
        .await?;

    let mut environments = BTreeSet::new();
    let mut namespaces = BTreeSet::new();
    let mut repositories = BTreeSet::new();
    let mut branches = BTreeSet::new();
    let mut actors = BTreeSet::new();
    for item in &work_items {
        environments.insert(item.target_environment.clone());
        repositories.insert(item.source_repo.clone());
        branches.insert(item.source_ref.clone());
        if let Some(value) = &item.gitops_repo {
            repositories.insert(value.clone());
        }
        if let Some(value) = &item.gitops_ref {
            branches.insert(value.clone());
        }
        if let Some(value) = &item.target_namespace {
            namespaces.insert(value.clone());
        }
        if let Some(value) = &item.created_by {
            actors.insert(value.clone());
        }
    }
    for workspace in &workspaces {
        repositories.insert(workspace.source_repo.clone());
        branches.insert(workspace.source_ref.clone());
        if let Some(value) = &workspace.branch {
            branches.insert(value.clone());
        }
    }
    for run in &runs {
        if let Some(value) = &run.created_by {
            actors.insert(value.clone());
        }
    }
    for gate in &gates {
        if let Some(value) = &gate.resource_namespace {
            namespaces.insert(value.clone());
        }
        if let Some(value) = &gate.decided_by {
            actors.insert(value.clone());
        }
    }
    for event in &audit_events {
        if let Some(value) = &event.actor {
            actors.insert(value.clone());
        }
    }

    Ok(Json(ScopeOptionsResponse {
        environments: environments.into_iter().collect(),
        namespaces: namespaces.into_iter().collect(),
        repositories: repositories.into_iter().collect(),
        branches: branches.into_iter().collect(),
        actors: actors.into_iter().collect(),
        origins: work_items
            .iter()
            .map(|item| item.origin.clone())
            .chain(runs.iter().map(|run| run.origin.clone()))
            .chain(approvals.iter().map(|approval| approval.origin.clone()))
            .chain(gates.iter().map(|gate| gate.origin.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    }))
}

async fn triage_summary(
    State(state): State<AppState>,
) -> Result<Json<TriageSummaryResponse>, ApiError> {
    Ok(Json(load_triage(&state, None).await?.summary))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListTriageQuery {
    origin: Option<String>,
}

async fn list_triage(
    State(state): State<AppState>,
    Query(query): Query<ListTriageQuery>,
) -> Result<Json<TriageResponse>, ApiError> {
    Ok(Json(
        load_triage(&state, clean_optional_text(query.origin)).await?,
    ))
}

async fn load_triage(state: &AppState, origin: Option<String>) -> Result<TriageResponse, ApiError> {
    let limit = 200;
    let include_legacy = matches!(origin.as_deref(), None | Some("legacy"));
    let pending_gate_filter = ApprovalGateListFilter {
        status: Some("pending".to_string()),
        origin: origin.clone(),
        limit,
        ..ApprovalGateListFilter::default()
    };
    let pending_gates =
        all_approval_gates_for_operator_groups(state.store.as_ref(), pending_gate_filter).await?;
    let pending_approval_filter = ApprovalListFilter {
        status: Some("pending".to_string()),
        origin: origin.clone(),
        limit,
        ..ApprovalListFilter::default()
    };
    let pending_approvals =
        all_approvals_for_operator_groups(state.store.as_ref(), pending_approval_filter).await?;
    let blocked_work_item_filter = WorkItemListFilter {
        status: Some("blocked".to_string()),
        origin: origin.clone(),
        limit,
        ..WorkItemListFilter::default()
    };
    let blocked_work_item_count = state
        .store
        .count_work_items(blocked_work_item_filter.clone())
        .await?;
    let blocked_work_items = state
        .store
        .list_work_items(blocked_work_item_filter)
        .await?;
    let now = i64::try_from(current_millis()).unwrap_or(i64::MAX);
    let expired_wait_count = if include_legacy {
        state.store.count_expired_controller_waits(now).await?
    } else {
        0
    };
    let expired_waits = state
        .store
        .list_expired_controller_waits(now, limit)
        .await?
        .into_iter()
        .filter(|_| include_legacy)
        .collect::<Vec<_>>();
    let proposed_remediation_filter = RemediationPlanListFilter {
        status: Some("proposed".to_string()),
        limit,
        ..RemediationPlanListFilter::default()
    };
    let proposed_remediation_count = if include_legacy {
        state
            .store
            .count_remediation_plans(proposed_remediation_filter.clone())
            .await?
    } else {
        0
    };
    let proposed_remediation = state
        .store
        .list_remediation_plans(proposed_remediation_filter)
        .await?;
    let proposed_remediation = if include_legacy {
        proposed_remediation
    } else {
        Vec::new()
    };

    let summary = TriageSummaryResponse {
        pending_approval_gates: pending_gates.len(),
        pending_tool_approvals: pending_approvals.len(),
        blocked_work_items: blocked_work_item_count,
        expired_controller_waits: expired_wait_count,
        proposed_remediation_plans: proposed_remediation_count,
        total: pending_gates.len()
            + pending_approvals.len()
            + blocked_work_item_count
            + expired_wait_count
            + proposed_remediation_count,
    };
    let mut items = Vec::with_capacity(summary.total);
    items.extend(pending_gates.into_iter().map(|gate| TriageItemResponse {
        kind: "approval_gate".to_string(),
        id: gate.id.clone(),
        title: gate.title,
        summary: gate.summary,
        status: gate.status,
        risk_level: gate.risk_level,
        origin: gate.origin,
        created_at: gate.created_at,
        resource_kind: "approval_gate".to_string(),
        resource_id: gate.id,
        work_item_id: gate.work_item_id,
    }));
    items.extend(
        pending_approvals
            .into_iter()
            .map(|approval| TriageItemResponse {
                kind: "tool_approval".to_string(),
                id: approval.id.clone(),
                title: approval.kind,
                summary: approval.summary,
                status: approval.status,
                risk_level: approval.risk_level,
                origin: approval.origin,
                created_at: approval.requested_at,
                resource_kind: "approval".to_string(),
                resource_id: approval.id,
                work_item_id: None,
            }),
    );
    items.extend(blocked_work_items.into_iter().map(|item| {
        TriageItemResponse {
            kind: "blocked_work_item".to_string(),
            id: item.id.clone(),
            title: item.title,
            summary: item.status_reason.unwrap_or(item.intent),
            status: item.status,
            risk_level: if item.production_impacting {
                "high"
            } else {
                "medium"
            }
            .to_string(),
            origin: item.origin,
            created_at: item.status_changed_at,
            resource_kind: "work_item".to_string(),
            resource_id: item.id.clone(),
            work_item_id: Some(item.id),
        }
    }));
    items.extend(expired_waits.into_iter().map(|wait| TriageItemResponse {
        kind: "expired_controller_wait".to_string(),
        id: wait.id.clone(),
        title: format!("Expired {} wait", wait.wait_kind),
        summary: format!(
            "{} did not resolve before its next observation deadline",
            wait.subject_id
        ),
        status: wait.status,
        risk_level: "high".to_string(),
        origin: "legacy".to_string(),
        created_at: wait.deadline_at,
        resource_kind: "controller_wait".to_string(),
        resource_id: wait.id,
        work_item_id: Some(wait.work_item_id),
    }));
    items.extend(
        proposed_remediation
            .into_iter()
            .map(|plan| TriageItemResponse {
                kind: "remediation_plan".to_string(),
                id: plan.id.clone(),
                title: plan.title,
                summary: plan.summary,
                status: plan.status,
                risk_level: plan.risk_level,
                origin: "legacy".to_string(),
                created_at: plan.created_at,
                resource_kind: "remediation_plan".to_string(),
                resource_id: plan.id,
                work_item_id: None,
            }),
    );
    items.sort_by(|left, right| {
        triage_kind_rank(&left.kind)
            .cmp(&triage_kind_rank(&right.kind))
            .then_with(|| {
                triage_risk_rank(&right.risk_level).cmp(&triage_risk_rank(&left.risk_level))
            })
            .then_with(|| left.created_at.cmp(&right.created_at))
    });

    Ok(TriageResponse { items, summary })
}

fn triage_kind_rank(kind: &str) -> u8 {
    match kind {
        "approval_gate" => 0,
        "tool_approval" => 1,
        "blocked_work_item" => 2,
        "expired_controller_wait" => 3,
        "remediation_plan" => 4,
        _ => u8::MAX,
    }
}

fn triage_risk_rank(risk: &str) -> u8 {
    match risk {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        _ => 1,
    }
}
