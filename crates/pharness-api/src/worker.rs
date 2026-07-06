use pharness_core::{
    AgentEvent, CancellationFlag, EventId, EventKind, ReadOnlyClusterTools, ResourceRef, RunId,
    RunScope, SafetyPolicy,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_runhost::{
    execute_attempt, ApprovalRequestPayload, AttemptBackend, AttemptHost, AttemptOutcome,
    AttemptSpec, ResumeSpec, RunSpec,
};
use pharness_store::{
    CreateApproval, CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateFileChange,
    CreateIncident, CreateObservation, CreateRemediationPlan, SqliteStore, StoreError,
    StoredApproval, StoredIncident, StoredObservation, StoredRemediationPlan, StoredRun,
};
use secrecy::SecretString;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct LocalWorker {
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    model: String,
    base_url: String,
    cluster_tools: ReadOnlyClusterTools,
    default_policy: SafetyPolicy,
    cancellations: Arc<Mutex<HashMap<RunId, CancellationFlag>>>,
}

impl LocalWorker {
    pub fn from_options(
        store: Arc<SqliteStore>,
        options: LocalWorkerOptions,
    ) -> anyhow::Result<Option<Self>> {
        let Some(api_key) = options.api_key else {
            return Ok(None);
        };
        let model = options.model;
        let base_url = options.base_url;
        let cluster_tools = options.cluster_tools;
        let default_policy = options.default_policy;

        let provider = FireworksClient::new(
            api_key,
            FireworksProviderConfig {
                base_url: base_url.clone(),
                model: model.clone(),
            },
        )?;

        Ok(Some(Self {
            store,
            model,
            base_url,
            provider,
            cluster_tools,
            default_policy,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }))
    }

    pub fn config(&self) -> LocalWorkerConfig {
        LocalWorkerConfig {
            enabled: true,
            provider: "fireworks".to_string(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
        }
    }

    pub fn spawn_run(&self, run: StoredRun, cwd: impl Into<PathBuf>) {
        self.spawn_task(run, cwd.into(), None);
    }

    pub fn resume_run(&self, run: StoredRun, approval: StoredApproval) {
        self.spawn_task(run.clone(), PathBuf::from(run.cwd.clone()), Some(approval));
    }

    pub fn cancel(&self, run_id: &RunId) -> bool {
        let Some(flag) = self
            .cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .get(run_id)
            .cloned()
        else {
            return false;
        };

        flag.cancel();
        true
    }

    fn spawn_task(&self, run: StoredRun, cwd: PathBuf, approval: Option<StoredApproval>) {
        let store = self.store.clone();
        let host = AttemptHost {
            provider: self.provider.clone(),
            cluster_tools: self.cluster_tools.clone(),
            default_policy: self.default_policy.clone(),
        };
        let cancellations = self.cancellations.clone();
        let cancellation = CancellationFlag::default();

        cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .insert(run.id.clone(), cancellation.clone());

        tokio::spawn(async move {
            let run_id = run.id.clone();
            let result =
                run_local_attempt(store.clone(), host, run, cwd, approval, cancellation).await;

            cancellations
                .lock()
                .expect("cancellation registry mutex should not be poisoned")
                .remove(&run_id);

            if let Err(error) = result {
                let _ = fail_run_from_dispatch(&store, &run_id, error.to_string()).await;
            }
        });
    }
}

async fn run_local_attempt(
    store: Arc<SqliteStore>,
    host: AttemptHost,
    run: StoredRun,
    cwd: PathBuf,
    approval: Option<StoredApproval>,
    cancellation: CancellationFlag,
) -> anyhow::Result<()> {
    let spec = attempt_spec_for_run(&store, &run, &cwd, approval.as_ref()).await?;
    let backend = Arc::new(LocalAttemptBackend { store, run });

    execute_attempt(host, backend, spec, cancellation).await
}

pub(crate) async fn attempt_spec_for_run(
    store: &SqliteStore,
    run: &StoredRun,
    cwd: &std::path::Path,
    approval: Option<&StoredApproval>,
) -> anyhow::Result<AttemptSpec> {
    let event_seq_start = store.list_events(&run.id).await?.len() as u64;
    let resume = approval.map(resume_spec_from_approval).transpose()?;

    Ok(AttemptSpec {
        run: RunSpec {
            run_id: run.id.to_string(),
            session_id: run.session_id.to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            user_task: run.user_task.clone(),
            max_turns: run.max_turns,
            execution_target_json: run.execution_target_json.clone(),
        },
        event_seq_start,
        resume,
    })
}

fn resume_spec_from_approval(approval: &StoredApproval) -> anyhow::Result<ResumeSpec> {
    let action_json = approval
        .action_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("approval has no reviewed action payload"))?;
    let resume_messages_json = approval
        .resume_messages_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("approval has no resumable message transcript"))?;

    Ok(ResumeSpec {
        approval_id: approval.id.clone(),
        action_json,
        resume_messages_json,
        turns_completed: approval.turns_completed,
    })
}

pub(crate) struct LocalAttemptBackend {
    store: Arc<SqliteStore>,
    run: StoredRun,
}

#[async_trait::async_trait]
impl AttemptBackend for LocalAttemptBackend {
    async fn mark_running(&self) -> anyhow::Result<()> {
        self.store.mark_run_running(&self.run.id).await?;
        Ok(())
    }

    async fn ingest_event(&self, event: &AgentEvent) -> anyhow::Result<()> {
        ingest_agent_event(&self.store, event).await?;
        Ok(())
    }

    async fn finish(&self, outcome: AttemptOutcome) -> anyhow::Result<()> {
        finish_run_from_attempt(&self.store, &self.run, outcome).await
    }
}

#[derive(Clone)]
pub struct LocalWorkerOptions {
    pub api_key: Option<SecretString>,
    pub model: String,
    pub base_url: String,
    pub cluster_tools: ReadOnlyClusterTools,
    pub default_policy: SafetyPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalWorkerConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub base_url: String,
}

pub(crate) async fn finish_run_from_attempt(
    store: &SqliteStore,
    run: &StoredRun,
    outcome: AttemptOutcome,
) -> anyhow::Result<()> {
    let error = outcome.error.clone();
    let approval_id = if outcome.status == "approval_required" {
        match &outcome.approval {
            Some(payload) => Some(create_pending_approval(store, run, payload).await?.id),
            None => None,
        }
    } else {
        None
    };
    let result_json = result_json_for_attempt(run, &outcome, approval_id);

    match outcome.status.as_str() {
        "completed" => {
            store
                .complete_run(&run.id, "completed", result_json, None)
                .await?;
        }
        "approval_required" => {
            store
                .mark_run_approval_required(&run.id, result_json)
                .await?;
        }
        "failed" => {
            store
                .complete_run(&run.id, "failed", result_json, error)
                .await?;
        }
        "cancelled" => {
            store
                .complete_run(&run.id, "cancelled", result_json, error)
                .await?;
        }
        other => {
            anyhow::bail!("attempt reported unknown terminal status: {other}");
        }
    }

    Ok(())
}

fn run_scope_for_run(run: &StoredRun) -> RunScope {
    RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default()
}

fn result_json_for_attempt(
    run: &StoredRun,
    outcome: &AttemptOutcome,
    approval_id: Option<String>,
) -> serde_json::Value {
    let run_scope = run_scope_for_run(run);
    serde_json::json!({
        "status": &outcome.status,
        "turns": outcome.turns,
        "summary": &outcome.summary,
        "error": &outcome.error,
        "approval_id": approval_id,
        "run_scope": run_scope.to_optional_json(),
    })
}

async fn create_pending_approval(
    store: &SqliteStore,
    run: &StoredRun,
    payload: &ApprovalRequestPayload,
) -> Result<StoredApproval, StoreError> {
    let run_scope = run_scope_for_run(run);
    let run_scope_json = run_scope.to_optional_json();

    store
        .create_approval(CreateApproval {
            id: format!("appr_{}_{}", run.id.as_str(), unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: run.id.clone(),
            status: "pending".to_string(),
            kind: payload.kind.clone(),
            summary: payload.summary.clone(),
            risk_level: payload.risk.clone(),
            run_scope_json,
            action_json: payload.action_json.clone(),
            preview_json: payload.preview_json.clone(),
            resume_messages_json: Some(payload.resume_messages_json.clone()),
            turns_completed: payload.turns_completed,
        })
        .await
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Persist one agent event plus every derived control-plane record.
///
/// This is the single ingestion path for run events: the in-process local
/// backend and the worker ingest endpoints both go through it, so derivation
/// behavior cannot fork between execution targets.
pub(crate) async fn ingest_agent_event(
    store: &SqliteStore,
    event: &AgentEvent,
) -> Result<(), StoreError> {
    store.append_event(event).await?;
    if let Some(change) = file_change_from_event(event) {
        store.create_file_change(change).await?;
    }
    let artifact_id = if let Some(artifact) = artifact_from_event(event) {
        let artifact_id = artifact.id.clone();
        store.create_artifact(artifact).await?;
        Some(artifact_id)
    } else {
        None
    };
    if let Some(observation) = observation_from_event(event, artifact_id) {
        let observation = store.create_observation(observation).await?;
        if let Some(incident) = incident_from_observation(&observation) {
            let incident = store.create_incident(incident).await?;
            if let Some(plan) = remediation_plan_from_incident(&incident) {
                let plan = store.create_remediation_plan(plan).await?;
                for gate in approval_gates_from_remediation_plan(&plan) {
                    store.create_approval_gate(gate).await?;
                }
            }
        }
    }
    if let Some(audit_event) = grant_used_audit_event_from_event(event) {
        store.create_audit_event(audit_event).await?;
    }

    Ok(())
}

fn grant_used_audit_event_from_event(event: &AgentEvent) -> Option<CreateAuditEvent> {
    if event.kind != EventKind::PolicyEvaluated {
        return None;
    }

    let grant_id = event.payload.get("decision")?.get("grant_id")?.as_str()?;

    Some(CreateAuditEvent {
        id: format!("aud_{}_grant_used", event.event_id.as_str()),
        kind: "permission_grant.used".to_string(),
        actor: Some("agent:local-worker".to_string()),
        resource_kind: "permission_grant".to_string(),
        resource_id: grant_id.to_string(),
        run_id: Some(event.run_id.clone()),
        payload_json: serde_json::json!({
            "grant_id": grant_id,
            "session_id": event.session_id.as_str(),
            "run_id": event.run_id.as_str(),
            "source_event_id": event.event_id.as_str(),
            "action": event.payload.get("action"),
            "decision": event.payload.get("decision"),
            "run_scope": event.payload.get("run_scope"),
        }),
    })
}

fn file_change_from_event(event: &AgentEvent) -> Option<CreateFileChange> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let path = content.get("path")?.as_str()?;
    let diff = content.get("diff")?.as_str()?;

    Some(CreateFileChange {
        id: format!("chg_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        path: path.to_string(),
        before_hash: None,
        after_hash: None,
        diff: diff.to_string(),
    })
}

fn artifact_from_event(event: &AgentEvent) -> Option<CreateArtifact> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let source = content.get("source")?.as_str()?;
    if !matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    ) {
        return None;
    }

    let kind = if source == "tekton"
        && content.get("resource").and_then(serde_json::Value::as_str)
            == Some("pipeline_run_analysis")
    {
        "pipeline_run_analysis".to_string()
    } else {
        format!("{source}_tool_result")
    };

    Some(CreateArtifact {
        id: format!("art_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: Some(event.run_id.clone()),
        kind,
        label: event
            .payload
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tool result")
            .to_string(),
        mime_type: Some("application/json".to_string()),
        path: None,
        content_text: None,
        content_json: Some(content.clone()),
    })
}

fn observation_from_event(
    event: &AgentEvent,
    artifact_id: Option<String>,
) -> Option<CreateObservation> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let source = content.get("source")?.as_str()?;
    if !matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    ) {
        return None;
    }

    let kind = observation_kind(content, source);
    let subject = observation_subject(content, source, &kind);
    let identity = observation_resource_identity(content, source, &kind, &subject);
    let summary = event
        .payload
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("observed tool result")
        .to_string();
    let resource_ref_json = observation_resource_ref(event, content, source, &kind, &subject);

    Some(CreateObservation {
        id: format!("obs_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: Some(event.run_id.clone()),
        source: source.to_string(),
        kind,
        subject,
        summary,
        resource_namespace: identity.namespace,
        resource_kind: identity.kind,
        resource_name: identity.name,
        resource_ref_json,
        artifact_id,
        data_json: observation_data(content),
    })
}

fn observation_kind(content: &serde_json::Value, source: &str) -> String {
    content
        .get("resource")
        .and_then(serde_json::Value::as_str)
        .or_else(|| content.get("action").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{source}_read"))
}

fn observation_subject(content: &serde_json::Value, source: &str, kind: &str) -> String {
    if let Some(query) = content.get("query").and_then(serde_json::Value::as_str) {
        return query.to_string();
    }
    if let Some(name) = content.get("name").and_then(serde_json::Value::as_str) {
        return name.to_string();
    }
    if let Some(namespace) = content.get("namespace").and_then(serde_json::Value::as_str) {
        return format!("{namespace}/{kind}");
    }
    format!("{source}/{kind}")
}

#[derive(Debug, Default)]
struct ObservationResourceIdentity {
    namespace: Option<String>,
    kind: Option<String>,
    name: Option<String>,
}

fn observation_resource_identity(
    content: &serde_json::Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> ObservationResourceIdentity {
    ObservationResourceIdentity {
        namespace: first_string(&[
            content.pointer("/namespace"),
            content.pointer("/output/metadata/namespace"),
            content.pointer("/analysis/pipeline_run/namespace"),
        ]),
        kind: observation_resource_kind(content, source, kind),
        name: first_string(&[
            content.pointer("/name"),
            content.pointer("/output/metadata/name"),
            content.pointer("/analysis/pipeline_run/name"),
        ])
        .or_else(|| normalized_resource_name(source, kind, subject)),
    }
}

fn observation_resource_kind(
    content: &serde_json::Value,
    source: &str,
    kind: &str,
) -> Option<String> {
    let output_kind = content
        .pointer("/output/kind")
        .and_then(serde_json::Value::as_str);
    if output_kind.is_some_and(|value| value != "List") {
        return output_kind.map(str::to_string);
    }
    if source == "tekton" && kind == "pipeline_run_analysis" {
        return Some("PipelineRun".to_string());
    }

    first_string(&[
        content.pointer("/analysis/pipeline_run/kind"),
        content.pointer("/resource"),
    ])
    .or_else(|| normalized_resource_kind(source, kind))
}

fn normalized_resource_kind(source: &str, kind: &str) -> Option<String> {
    match (source, kind) {
        ("argocd", _) => Some("Application".to_string()),
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("prometheus", _) => Some("query".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        ("tekton", "pipeline_run_analysis") => Some("PipelineRun".to_string()),
        (_, value) if !value.trim().is_empty() => Some(value.to_string()),
        _ => None,
    }
}

fn normalized_resource_name(source: &str, kind: &str, subject: &str) -> Option<String> {
    match (source, kind) {
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        _ if !subject.trim().is_empty() && !subject.contains('/') => Some(subject.to_string()),
        _ => None,
    }
}

fn first_string(values: &[Option<&serde_json::Value>]) -> Option<String> {
    values
        .iter()
        .filter_map(|value| value.and_then(serde_json::Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn observation_resource_ref(
    event: &AgentEvent,
    content: &serde_json::Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> Option<serde_json::Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "event_id".to_string(),
        serde_json::Value::String(event.event_id.to_string()),
    );
    metadata.insert(
        "run_id".to_string(),
        serde_json::Value::String(event.run_id.to_string()),
    );

    let mut resource =
        ResourceRef::new(source, kind, subject).with_metadata(serde_json::Value::Object(metadata));
    if let Some(namespace) = content.get("namespace").and_then(serde_json::Value::as_str) {
        resource = resource.with_namespace(namespace);
    }

    serde_json::to_value(resource).ok()
}

fn observation_data(content: &serde_json::Value) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    copy_observation_field(&mut data, content, "source");
    copy_observation_field(&mut data, content, "resource");
    copy_observation_field(&mut data, content, "namespace");
    copy_observation_field(&mut data, content, "name");
    copy_observation_field(&mut data, content, "query");
    copy_observation_field(&mut data, content, "output");
    copy_observation_field(&mut data, content, "response");
    copy_observation_field(&mut data, content, "analysis");

    serde_json::Value::Object(data)
}

fn incident_from_observation(observation: &StoredObservation) -> Option<CreateIncident> {
    if observation.source != "tekton" || observation.kind != "pipeline_run_analysis" {
        return None;
    }

    let analysis = observation.data_json.get("analysis")?;
    let reasons = pipeline_run_incident_reasons(analysis);
    if reasons.is_empty() {
        return None;
    }

    let severity = pipeline_run_incident_severity(&reasons);
    let resource = observation_resource_label(observation);
    let summary = reasons.join("; ");

    Some(CreateIncident {
        id: format!("inc_{}", observation.id),
        observation_id: observation.id.clone(),
        session_id: observation.session_id.clone(),
        run_id: observation.run_id.clone(),
        status: "candidate".to_string(),
        severity: severity.to_string(),
        title: format!("Tekton PipelineRun issue: {resource}"),
        summary: summary.clone(),
        resource_namespace: observation.resource_namespace.clone(),
        resource_kind: observation.resource_kind.clone(),
        resource_name: observation.resource_name.clone(),
        data_json: serde_json::json!({
            "source": "observation",
            "observation_id": observation.id.clone(),
            "reasons": reasons,
            "summary": summary,
        }),
    })
}

fn remediation_plan_from_incident(incident: &StoredIncident) -> Option<CreateRemediationPlan> {
    if incident.status != "candidate" {
        return None;
    }

    let resource = incident_resource_label(incident);
    Some(CreateRemediationPlan {
        id: format!("rplan_{}", incident.id),
        incident_id: incident.id.clone(),
        session_id: incident.session_id.clone(),
        run_id: incident.run_id.clone(),
        status: "draft".to_string(),
        title: format!("Draft remediation for {resource}"),
        summary: "Review the incident evidence, run read-only checks, then require approval before any write, pipeline, or cluster mutation.".to_string(),
        risk_level: incident.severity.clone(),
        requires_approval: true,
        resource_namespace: incident.resource_namespace.clone(),
        resource_kind: incident.resource_kind.clone(),
        resource_name: incident.resource_name.clone(),
        plan_json: remediation_plan_json(incident, &resource),
    })
}

fn remediation_plan_json(incident: &StoredIncident, resource: &str) -> serde_json::Value {
    let reasons = incident
        .data_json
        .get("reasons")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    serde_json::json!({
        "mode": "read_only_draft",
        "incident_id": incident.id.clone(),
        "resource": {
            "namespace": incident.resource_namespace.clone(),
            "kind": incident.resource_kind.clone(),
            "name": incident.resource_name.clone(),
            "label": resource,
        },
        "evidence": {
            "summary": incident.summary.clone(),
            "reasons": reasons,
        },
        "steps": [
            {
                "order": 1,
                "kind": "read_only",
                "capability": "tekton_analyze_pipeline_run",
                "summary": "Re-read PipelineRun, TaskRuns, Deployment health, Argo health, and image alignment before deciding on any action."
            },
            {
                "order": 2,
                "kind": "read_only",
                "capability": "loki_log_summary",
                "summary": "Inspect bounded application and controller logs for the affected namespace if Loki is configured."
            },
            {
                "order": 3,
                "kind": "proposal",
                "capability": "worktree_change",
                "summary": "If evidence points to repo configuration or application code, prepare a ChangeSet and require approval before file writes."
            },
            {
                "order": 4,
                "kind": "proposal",
                "capability": "pipeline_or_deployment_action",
                "summary": "If evidence points to stale deployment state, propose rerun, sync, rollback, or restart intent and require explicit approval before mutation."
            }
        ],
        "approval_gates": [
            {
                "kind": "file_write",
                "required_before": "creating or patching a ChangeSet"
            },
            {
                "kind": "pipeline_mutation",
                "required_before": "rerunning or cancelling Tekton resources"
            },
            {
                "kind": "cluster_mutation",
                "required_before": "Argo sync, rollback, restart, scale, or Kubernetes write"
            },
            {
                "kind": "production_impact",
                "required_before": "any action against production-impacting scope"
            }
        ],
        "non_goals": [
            "No automatic mutation in V1",
            "No ticket creation",
            "No notification dispatch",
            "No secret reads"
        ]
    })
}

fn approval_gates_from_remediation_plan(plan: &StoredRemediationPlan) -> Vec<CreateApprovalGate> {
    let gates = plan
        .plan_json
        .get("approval_gates")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    gates
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("executing a risky action");
            let title = format!("Approve {}", gate_kind.replace('_', " "));
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    plan.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                remediation_plan_id: plan.id.clone(),
                incident_id: plan.incident_id.clone(),
                session_id: plan.session_id.clone(),
                run_id: plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title,
                summary: format!("Approval required before {required_before}."),
                risk_level: plan.risk_level.clone(),
                resource_namespace: plan.resource_namespace.clone(),
                resource_kind: plan.resource_kind.clone(),
                resource_name: plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

fn approval_gate_kind(gate_json: &serde_json::Value) -> Option<String> {
    gate_json
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .or_else(|| gate_json.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(ToOwned::to_owned)
}

fn safe_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn pipeline_run_incident_reasons(analysis: &serde_json::Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(status) = analysis
        .pointer("/summary/status")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(status, "succeeded" | "running") {
            reasons.push(format!("PipelineRun status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/deployment/status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "healthy" && status != "skipped" {
            reasons.push(format!("Deployment status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/argo_sync_status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "Synced" {
            reasons.push(format!("Argo sync status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/argo_health_status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "Healthy" {
            reasons.push(format!("Argo health status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/image_alignment/status")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(status, "exact_match" | "registry_alias_match" | "unknown") {
            reasons.push(format!("Image alignment is {status}"));
        }
    }

    reasons
}

fn pipeline_run_incident_severity(reasons: &[String]) -> &'static str {
    if reasons
        .iter()
        .any(|reason| reason.contains("failed") || reason.contains("error"))
    {
        "high"
    } else {
        "medium"
    }
}

fn observation_resource_label(observation: &StoredObservation) -> String {
    match (
        observation.resource_namespace.as_deref(),
        observation.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
        (_, Some(name)) => name.to_string(),
        _ => observation.subject.clone(),
    }
}

fn incident_resource_label(incident: &StoredIncident) -> String {
    match (
        incident.resource_namespace.as_deref(),
        incident.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
        (_, Some(name)) => name.to_string(),
        _ => incident.title.clone(),
    }
}

fn copy_observation_field(
    data: &mut serde_json::Map<String, serde_json::Value>,
    content: &serde_json::Value,
    field: &str,
) {
    if let Some(value) = content.get(field) {
        data.insert(field.to_string(), value.clone());
    }
}

pub(crate) async fn fail_run_from_dispatch(
    store: &SqliteStore,
    run_id: &RunId,
    message: String,
) -> Result<(), StoreError> {
    let seq = store.list_events(run_id).await?.len() as u64 + 1;
    let Some(run) = store.get_run(run_id).await? else {
        return Ok(());
    };

    store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", run_id.as_str(), seq)),
            session_id: run.session_id,
            run_id: run_id.clone(),
            seq,
            kind: EventKind::RunFailed,
            payload: serde_json::json!({ "error": message }),
        })
        .await?;

    store
        .complete_run(
            run_id,
            "failed",
            serde_json::json!({
                "status": "failed",
                "turns": 0,
                "summary": null,
                "error": message,
            }),
            Some(message),
        )
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        approval_gates_from_remediation_plan, artifact_from_event, file_change_from_event,
        grant_used_audit_event_from_event, incident_from_observation, observation_from_event,
        remediation_plan_from_incident, result_json_for_attempt,
    };
    use pharness_core::{AgentEvent, EventId, EventKind, RunId, SessionId};
    use pharness_runhost::AttemptOutcome;
    use pharness_store::{StoredIncident, StoredObservation, StoredRemediationPlan, StoredRun};

    #[test]
    fn result_json_uses_null_for_absent_run_scope() {
        let run = stored_run(serde_json::json!({
            "kind": "local_process",
            "run_scope": null,
        }));
        let outcome = AttemptOutcome {
            status: "completed".to_string(),
            turns: 2,
            summary: Some("done".to_string()),
            error: None,
            approval: None,
        };

        let result = result_json_for_attempt(&run, &outcome, None);

        assert!(result["run_scope"].is_null());
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn result_json_preserves_non_empty_run_scope() {
        let run = stored_run(serde_json::json!({
            "kind": "local_process",
            "run_scope": {
                "namespace": "apps-dev",
                "repo": "git@example.test/team/app.git",
                "branch": "feature/pharness",
                "production_impacting": false
            },
        }));
        let outcome = AttemptOutcome {
            status: "completed".to_string(),
            turns: 2,
            summary: Some("done".to_string()),
            error: None,
            approval: None,
        };

        let result = result_json_for_attempt(&run, &outcome, None);

        assert_eq!(result["run_scope"]["namespace"], "apps-dev");
    }

    #[test]
    fn extracts_file_change_from_write_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_8"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 8,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "wrote file",
                "content": {
                    "path": "README.md",
                    "diff": "--- before\n+++ after"
                }
            }),
        };

        let change = file_change_from_event(&event).unwrap();

        assert_eq!(change.id, "chg_evt_run_test_8");
        assert_eq!(change.path, "README.md");
        assert!(change.diff.contains("+++ after"));
    }

    #[test]
    fn extracts_permission_grant_used_audit_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_7"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 7,
            kind: EventKind::PolicyEvaluated,
            payload: serde_json::json!({
                "action": "write_file",
                "decision": {
                    "decision": "allow",
                    "risk": "medium",
                    "summary": "allowed by grant",
                    "grant_id": "pgrant_test"
                },
                "run_scope": {
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                }
            }),
        };

        let audit_event = grant_used_audit_event_from_event(&event).unwrap();

        assert_eq!(audit_event.kind, "permission_grant.used");
        assert_eq!(audit_event.resource_id, "pgrant_test");
        assert_eq!(audit_event.run_id.as_ref().unwrap().as_str(), "run_test");
        assert_eq!(
            audit_event.payload_json["run_scope"]["namespace"],
            "apps-dev"
        );
    }

    #[test]
    fn extracts_cluster_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_8"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 8,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Prometheus instant query",
                "content": {
                    "source": "prometheus",
                    "query": "up",
                    "response": {
                        "data": {
                            "result_count": 33
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_8");
        assert_eq!(artifact.kind, "prometheus_tool_result");
        assert_eq!(artifact.label, "read Prometheus instant query");
        assert_eq!(
            artifact.content_json.unwrap()["response"]["data"]["result_count"],
            33
        );
    }

    #[test]
    fn extracts_observation_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_11"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 11,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Prometheus instant query",
                "content": {
                    "source": "prometheus",
                    "query": "up",
                    "response": {
                        "data": {
                            "result_count": 33
                        }
                    }
                }
            }),
        };

        let observation =
            observation_from_event(&event, Some("art_evt_run_test_11".to_string())).unwrap();

        assert_eq!(observation.id, "obs_evt_run_test_11");
        assert_eq!(observation.source, "prometheus");
        assert_eq!(observation.kind, "prometheus_read");
        assert_eq!(observation.subject, "up");
        assert_eq!(observation.resource_kind.as_deref(), Some("query"));
        assert_eq!(observation.resource_name.as_deref(), Some("up"));
        assert_eq!(
            observation.artifact_id.as_deref(),
            Some("art_evt_run_test_11")
        );
        assert_eq!(
            observation.data_json["response"]["data"]["result_count"],
            33
        );
    }

    #[test]
    fn extracts_loki_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_10"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 10,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Loki log summary",
                "content": {
                    "source": "loki",
                    "resource": "log_summary",
                    "response": {
                        "data": {
                            "entry_count": 3
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_10");
        assert_eq!(artifact.kind, "loki_tool_result");
        assert_eq!(artifact.label, "read Loki log summary");
        assert_eq!(
            artifact.content_json.unwrap()["response"]["data"]["entry_count"],
            3
        );
    }

    #[test]
    fn extracts_pipeline_run_analysis_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_9"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 9,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "analyzed Tekton PipelineRun ci/build-app",
                "content": {
                    "source": "tekton",
                    "resource": "pipeline_run_analysis",
                    "namespace": "ci",
                    "name": "build-app",
                    "analysis": {
                        "kind": "PipelineRunAnalysis",
                        "summary": {
                            "status": "failed"
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();
        let observation = observation_from_event(&event, Some(artifact.id.clone())).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_9");
        assert_eq!(artifact.kind, "pipeline_run_analysis");
        assert_eq!(artifact.label, "analyzed Tekton PipelineRun ci/build-app");
        assert_eq!(
            artifact.content_json.unwrap()["analysis"]["summary"]["status"],
            "failed"
        );
        assert_eq!(observation.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(observation.resource_kind.as_deref(), Some("PipelineRun"));
        assert_eq!(observation.resource_name.as_deref(), Some("build-app"));
    }

    #[test]
    fn extracts_incident_candidate_from_failed_pipeline_observation() {
        let observation = StoredObservation {
            id: "obs_test".to_string(),
            session_id: SessionId::new("ses_test"),
            run_id: Some(RunId::new("run_test")),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-app".to_string(),
            summary: "analyzed Tekton PipelineRun ci/build-app".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            resource_ref_json: None,
            artifact_id: Some("art_test".to_string()),
            data_json: serde_json::json!({
                "analysis": {
                    "summary": {
                        "status": "failed",
                        "argo_sync_status": "OutOfSync",
                        "argo_health_status": "Degraded",
                        "image_alignment": {
                            "status": "registry_mismatch"
                        }
                    },
                    "deployment": {
                        "status": "progressing"
                    }
                }
            }),
            observed_at: "1".to_string(),
        };

        let incident = incident_from_observation(&observation).unwrap();

        assert_eq!(incident.id, "inc_obs_test");
        assert_eq!(incident.status, "candidate");
        assert_eq!(incident.severity, "high");
        assert_eq!(incident.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(incident.resource_kind.as_deref(), Some("PipelineRun"));
        assert_eq!(incident.resource_name.as_deref(), Some("build-app"));
        assert!(
            incident
                .data_json
                .get("reasons")
                .and_then(serde_json::Value::as_array)
                .unwrap()
                .len()
                >= 4
        );
    }

    #[test]
    fn extracts_draft_remediation_plan_from_incident_candidate() {
        let incident = StoredIncident {
            id: "inc_obs_test".to_string(),
            observation_id: "obs_test".to_string(),
            session_id: SessionId::new("ses_test"),
            run_id: Some(RunId::new("run_test")),
            status: "candidate".to_string(),
            severity: "high".to_string(),
            title: "Tekton PipelineRun issue: ci/build-app".to_string(),
            summary: "PipelineRun status is failed".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            data_json: serde_json::json!({
                "reasons": ["PipelineRun status is failed"]
            }),
            created_at: "1".to_string(),
        };

        let plan = remediation_plan_from_incident(&incident).unwrap();

        assert_eq!(plan.id, "rplan_inc_obs_test");
        assert_eq!(plan.incident_id, "inc_obs_test");
        assert_eq!(plan.status, "draft");
        assert_eq!(plan.risk_level, "high");
        assert!(plan.requires_approval);
        assert_eq!(plan.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(
            plan.plan_json["approval_gates"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            4
        );
        assert_eq!(plan.plan_json["mode"], "read_only_draft");

        let stored_plan = StoredRemediationPlan {
            id: plan.id,
            incident_id: plan.incident_id,
            session_id: plan.session_id,
            run_id: plan.run_id,
            status: plan.status,
            title: plan.title,
            summary: plan.summary,
            risk_level: plan.risk_level,
            requires_approval: plan.requires_approval,
            resource_namespace: plan.resource_namespace,
            resource_kind: plan.resource_kind,
            resource_name: plan.resource_name,
            plan_json: plan.plan_json,
            created_at: "1".to_string(),
        };
        let gates = approval_gates_from_remediation_plan(&stored_plan);

        assert_eq!(gates.len(), 4);
        assert_eq!(gates[0].id, "agate_rplan_inc_obs_test_1_file_write");
        assert_eq!(gates[0].gate_kind, "file_write");
        assert_eq!(gates[0].gate_order, 1);
        assert_eq!(gates[0].status, "pending");
        assert_eq!(gates[0].risk_level, "high");
        assert_eq!(gates[0].resource_namespace.as_deref(), Some("ci"));
    }

    fn stored_run(execution_target_json: serde_json::Value) -> StoredRun {
        StoredRun {
            id: RunId::new("run_test"),
            session_id: SessionId::new("ses_test"),
            cwd: ".".to_string(),
            status: "queued".to_string(),
            user_task: "test".to_string(),
            max_turns: 40,
            started_at: "0".to_string(),
            finished_at: None,
            cancel_requested_at: None,
            error: None,
            result_json: None,
            execution_target_json,
        }
    }
}
