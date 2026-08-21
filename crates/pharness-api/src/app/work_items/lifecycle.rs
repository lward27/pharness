use super::super::*;

pub(in crate::app) fn work_item_approval_gate_specs(item: &StoredWorkItem) -> Vec<Value> {
    let mut gates = vec![
        json!({ "kind": "source_mutation", "required_before": "creating a source branch, commit, or pull request" }),
        json!({ "kind": "git_mutation", "required_before": "creating a source branch, commit, or pull request" }),
        json!({ "kind": "pipeline_mutation", "required_before": "starting a Tekton PipelineRun" }),
        json!({ "kind": "gitops_mutation", "required_before": "creating a GitOps branch, commit, or pull request" }),
        json!({ "kind": "cluster_mutation", "required_before": "syncing an Argo CD application" }),
    ];
    if item.production_impacting {
        gates.push(json!({ "kind": "production_impact", "required_before": "executing a production-impacting action" }));
        gates.push(json!({ "kind": "production_deployment", "required_before": "opening the bound production window and dispatching the exact Argo sync" }));
    }
    gates
}

pub(in crate::app) fn approval_gates_from_work_item(
    item: &StoredWorkItem,
    work_plan: &StoredWorkPlan,
) -> Vec<CreateApprovalGate> {
    work_item_approval_gate_specs(item)
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let mut gate_json = gate_json;
            gate_json["scope"] = work_item_approval_gate_scope(item, work_plan, &gate_kind);
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(Value::as_str)
                .unwrap_or("executing a risky action");
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    item.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                work_item_id: Some(item.id.clone()),
                remediation_plan_id: None,
                incident_id: None,
                session_id: work_plan.session_id.clone(),
                run_id: work_plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title: format!("Approve {}", gate_kind.replace('_', " ")),
                summary: format!("Approval required before {required_before}."),
                risk_level: work_plan.risk_level.clone(),
                resource_namespace: work_plan.resource_namespace.clone(),
                resource_kind: work_plan.resource_kind.clone(),
                resource_name: work_plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

pub(in crate::app) fn work_item_approval_gate_scope(
    item: &StoredWorkItem,
    work_plan: &StoredWorkPlan,
    gate_kind: &str,
) -> Value {
    json!({
        "work_item_id": item.id,
        "work_plan_id": work_plan.id,
        "environment": item.target_environment,
        "production_impacting": item.production_impacting,
        "source_repository": item.source_repo,
        "source_ref": item.source_ref,
        "gitops_repository": item.gitops_repo,
        "gitops_ref": item.gitops_ref,
        "target_namespace": item.target_namespace,
        "argo_application": item.argo_application,
        "actions": approval_gate_actions(gate_kind),
    })
}

pub(in crate::app) fn approval_gate_actions(gate_kind: &str) -> &'static [&'static str] {
    match gate_kind {
        "source_mutation" | "git_mutation" => &GIT_DELIVERY_ACTIONS,
        "gitops_mutation" => &GITOPS_DELIVERY_ACTIONS,
        "pipeline_mutation" => &PIPELINE_DELIVERY_ACTIONS,
        "cluster_mutation" => &CLUSTER_DELIVERY_ACTIONS,
        "production_impact" | "production_deployment" => &PRODUCTION_DELIVERY_ACTIONS,
        _ => &[],
    }
}

pub(in crate::app) fn work_item_gate_scope_matches(
    gate: &StoredApprovalGate,
    item: &StoredWorkItem,
    work_plan: &StoredWorkPlan,
    gate_kind: &str,
) -> bool {
    if gate.work_item_id.as_deref() != Some(item.id.as_str()) || gate.gate_kind != gate_kind {
        return false;
    }
    let Some(scope) = gate.gate_json.get("scope").and_then(Value::as_object) else {
        return false;
    };
    let actions = scope.get("actions").and_then(Value::as_array);
    let expected_actions = approval_gate_actions(gate_kind);
    scope.get("work_item_id").and_then(Value::as_str) == Some(item.id.as_str())
        && scope.get("work_plan_id").and_then(Value::as_str) == Some(work_plan.id.as_str())
        && scope.get("environment").and_then(Value::as_str)
            == Some(item.target_environment.as_str())
        && scope.get("production_impacting").and_then(Value::as_bool)
            == Some(item.production_impacting)
        && scope.get("source_repository").and_then(Value::as_str) == Some(item.source_repo.as_str())
        && scope.get("source_ref").and_then(Value::as_str) == Some(item.source_ref.as_str())
        && scope.get("gitops_repository") == Some(&json!(item.gitops_repo))
        && scope.get("gitops_ref") == Some(&json!(item.gitops_ref))
        && scope.get("target_namespace") == Some(&json!(item.target_namespace))
        && scope.get("argo_application") == Some(&json!(item.argo_application))
        && actions.is_some_and(|actions| {
            expected_actions.iter().all(|expected| {
                actions
                    .iter()
                    .any(|action| action.as_str() == Some(*expected))
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum WorkItemStatus {
    Submitted,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

impl WorkItemStatus {
    pub(in crate::app) fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "submitted" => Ok(Self::Submitted),
            "planning" => Ok(Self::Planning),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "executing" => Ok(Self::Executing),
            "verifying" => Ok(Self::Verifying),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(ApiError::bad_request(format!(
                "unsupported work item status: {other}"
            ))),
        }
    }

    pub(in crate::app) fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Planning => "planning",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(in crate::app) fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        if self == target {
            return Ok(());
        }
        let allowed = match self {
            Self::Submitted => matches!(target, Self::Planning | Self::Cancelled),
            Self::Planning => matches!(
                target,
                Self::AwaitingApproval | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::AwaitingApproval => matches!(
                target,
                Self::Executing | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Executing => matches!(
                target,
                Self::Verifying | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Verifying => matches!(
                target,
                Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Blocked | Self::Failed => matches!(
                target,
                Self::Planning | Self::AwaitingApproval | Self::Cancelled
            ),
            Self::Completed | Self::Cancelled => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition work item from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum RemediationPlanStatus {
    Draft,
    Proposed,
    Approved,
    Executing,
    Blocked,
    Completed,
    Rejected,
    Stale,
}

impl RemediationPlanStatus {
    pub(in crate::app) fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "executing" => Ok(Self::Executing),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "rejected" => Ok(Self::Rejected),
            "stale" => Ok(Self::Stale),
            other => Err(ApiError::bad_request(format!(
                "unsupported remediation plan status: {other}"
            ))),
        }
    }

    pub(in crate::app) fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
        }
    }

    pub(in crate::app) fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        if self == target {
            return Ok(());
        }
        let allowed = match self {
            Self::Draft => matches!(target, Self::Proposed | Self::Rejected),
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Draft),
            Self::Approved => matches!(
                target,
                Self::Executing | Self::Rejected | Self::Draft | Self::Stale
            ),
            Self::Executing => matches!(target, Self::Blocked | Self::Completed | Self::Stale),
            Self::Blocked => matches!(
                target,
                Self::Executing | Self::Rejected | Self::Draft | Self::Stale
            ),
            Self::Completed | Self::Rejected | Self::Stale => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition remediation plan from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}
