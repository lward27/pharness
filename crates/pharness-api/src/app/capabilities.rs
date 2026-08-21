use super::*;

pub(in crate::app) async fn execute_capability(
    State(state): State<AppState>,
    Json(request): Json<ExecuteCapabilityRequest>,
) -> Result<Json<ExecuteCapabilityResponse>, ApiError> {
    execute_direct_capability(&state, request.action, request.timeout_ms)
        .await
        .map(Json)
}

pub(in crate::app) async fn execute_direct_capability(
    state: &AppState,
    action: AgentAction,
    requested_timeout_ms: Option<u64>,
) -> Result<ExecuteCapabilityResponse, ApiError> {
    let timeout_ms = direct_capability_timeout_ms(requested_timeout_ms);
    if !is_direct_capability_action(&action) {
        return Err(ApiError::bad_request(format!(
            "{} is not exposed through direct capability execution",
            action.kind_name()
        )));
    }

    let decision = state.policy.evaluate_action(&action);
    let response = match &decision {
        PolicyDecision::Allow { .. } => {
            let action_name = action.kind_name().to_string();
            match timeout(
                Duration::from_millis(timeout_ms),
                state.cluster_tools.execute(&action),
            )
            .await
            {
                Ok(Ok(result)) => {
                    let evidence =
                        persist_direct_capability_evidence(&state.store, &action_name, &result)
                            .await?;
                    append_direct_capability_audit_event(
                        &state.store,
                        DirectCapabilityAuditInput {
                            kind: "direct_capability.executed",
                            action: &action,
                            decision: &decision,
                            executed: true,
                            cancelled: false,
                            timeout_ms,
                            result: Some(&result),
                            error: None,
                        },
                    )
                    .await?;
                    ExecuteCapabilityResponse {
                        status: "ok".to_string(),
                        action: action_name,
                        decision: decision.clone(),
                        executed: true,
                        cancelled: false,
                        timeout_ms,
                        artifact_id: evidence.artifact_id,
                        observation_id: evidence.observation_id,
                        result: Some(result),
                        error: None,
                    }
                }
                Ok(Err(error)) => {
                    let error = error.to_string();
                    append_direct_capability_audit_event(
                        &state.store,
                        DirectCapabilityAuditInput {
                            kind: "direct_capability.failed",
                            action: &action,
                            decision: &decision,
                            executed: true,
                            cancelled: false,
                            timeout_ms,
                            result: None,
                            error: Some(&error),
                        },
                    )
                    .await?;
                    ExecuteCapabilityResponse {
                        status: "tool_error".to_string(),
                        action: action_name,
                        decision: decision.clone(),
                        executed: true,
                        cancelled: false,
                        timeout_ms,
                        artifact_id: None,
                        observation_id: None,
                        result: None,
                        error: Some(error),
                    }
                }
                Err(_) => {
                    let error = format!("capability execution cancelled after {timeout_ms} ms");
                    append_direct_capability_audit_event(
                        &state.store,
                        DirectCapabilityAuditInput {
                            kind: "direct_capability.cancelled",
                            action: &action,
                            decision: &decision,
                            executed: true,
                            cancelled: true,
                            timeout_ms,
                            result: None,
                            error: Some(&error),
                        },
                    )
                    .await?;
                    ExecuteCapabilityResponse {
                        status: "cancelled".to_string(),
                        action: action_name,
                        decision: decision.clone(),
                        executed: true,
                        cancelled: true,
                        timeout_ms,
                        artifact_id: None,
                        observation_id: None,
                        result: None,
                        error: Some(error),
                    }
                }
            }
        }
        PolicyDecision::Ask { .. } => ExecuteCapabilityResponse {
            status: "approval_required".to_string(),
            action: action.kind_name().to_string(),
            decision: decision.clone(),
            executed: false,
            cancelled: false,
            timeout_ms,
            artifact_id: None,
            observation_id: None,
            result: None,
            error: None,
        },
        PolicyDecision::Deny { summary, .. } => ExecuteCapabilityResponse {
            status: "denied".to_string(),
            action: action.kind_name().to_string(),
            decision: decision.clone(),
            executed: false,
            cancelled: false,
            timeout_ms,
            artifact_id: None,
            observation_id: None,
            result: None,
            error: Some(summary.clone()),
        },
    };
    if matches!(decision, PolicyDecision::Deny { .. }) {
        append_direct_capability_audit_event(
            &state.store,
            DirectCapabilityAuditInput {
                kind: "direct_capability.denied",
                action: &action,
                decision: &decision,
                executed: false,
                cancelled: false,
                timeout_ms,
                result: None,
                error: None,
            },
        )
        .await?;
    }

    Ok(response)
}

#[derive(Debug, Default)]
pub(in crate::app) struct DirectCapabilityEvidence {
    artifact_id: Option<String>,
    observation_id: Option<String>,
}

pub(in crate::app) async fn persist_direct_capability_evidence(
    store: &SqliteStore,
    action_name: &str,
    result: &ToolResult,
) -> Result<DirectCapabilityEvidence, ApiError> {
    let Some(source) = direct_evidence_source(result) else {
        return Ok(DirectCapabilityEvidence::default());
    };

    let (session_id, run_id) =
        root_session_for_request(store, None, None, "direct capability evidence").await?;
    let artifact_kind = direct_artifact_kind(&result.content, source);
    let artifact_id = format!("art_direct_{}_{}", action_name, unique_suffix());
    let artifact = store
        .create_artifact(CreateArtifact {
            id: artifact_id.clone(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            kind: artifact_kind,
            label: result.summary.clone(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(result.content.clone()),
        })
        .await?;

    let kind = direct_observation_kind(&result.content, source);
    let subject = direct_observation_subject(&result.content, source, &kind);
    let observation = store
        .create_observation(CreateObservation {
            id: format!("obs_direct_{}_{}", action_name, unique_suffix()),
            session_id,
            run_id,
            source: source.to_string(),
            kind: kind.clone(),
            subject: subject.clone(),
            summary: result.summary.clone(),
            resource_namespace: direct_observation_namespace(&result.content),
            resource_kind: direct_observation_resource_kind(&result.content, source, &kind),
            resource_name: direct_observation_resource_name(
                &result.content,
                source,
                &kind,
                &subject,
            ),
            resource_ref_json: Some(direct_observation_resource_ref(
                action_name,
                source,
                &kind,
                &subject,
            )),
            artifact_id: Some(artifact.id.clone()),
            data_json: direct_observation_data(&result.content),
        })
        .await?;
    append_observation_audit_event(
        store,
        &observation,
        "observation.created",
        Some("api".to_string()),
        Some(format!("direct capability {action_name}")),
    )
    .await?;

    Ok(DirectCapabilityEvidence {
        artifact_id: Some(artifact.id),
        observation_id: Some(observation.id),
    })
}

pub(in crate::app) fn direct_evidence_source(result: &ToolResult) -> Option<&str> {
    let source = result.content.get("source")?.as_str()?;
    matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    )
    .then_some(source)
}

pub(in crate::app) fn direct_artifact_kind(content: &Value, source: &str) -> String {
    if source == "tekton"
        && content.get("resource").and_then(Value::as_str) == Some("pipeline_run_analysis")
    {
        "pipeline_run_analysis".to_string()
    } else {
        format!("{source}_tool_result")
    }
}

pub(in crate::app) fn direct_observation_kind(content: &Value, source: &str) -> String {
    content
        .get("resource")
        .and_then(Value::as_str)
        .or_else(|| content.get("action").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{source}_read"))
}

pub(in crate::app) fn direct_observation_subject(
    content: &Value,
    source: &str,
    kind: &str,
) -> String {
    if source == "tekton" && kind == "pipeline_run_analysis" {
        if let (Some(namespace), Some(name)) = (
            content
                .pointer("/analysis/pipeline_run/namespace")
                .and_then(Value::as_str),
            content
                .pointer("/analysis/pipeline_run/name")
                .and_then(Value::as_str),
        ) {
            return format!("{namespace}/{name}");
        }
    }
    if let Some(query) = content.get("query").and_then(Value::as_str) {
        return query.to_string();
    }
    if let Some(name) = content.get("name").and_then(Value::as_str) {
        return name.to_string();
    }
    if let Some(namespace) = content.get("namespace").and_then(Value::as_str) {
        return format!("{namespace}/{kind}");
    }
    format!("{source}/{kind}")
}

pub(in crate::app) fn direct_observation_namespace(content: &Value) -> Option<String> {
    first_direct_string(&[
        content.pointer("/namespace"),
        content.pointer("/output/metadata/namespace"),
        content.pointer("/analysis/pipeline_run/namespace"),
    ])
}

pub(in crate::app) fn direct_observation_resource_kind(
    content: &Value,
    source: &str,
    kind: &str,
) -> Option<String> {
    let output_kind = content.pointer("/output/kind").and_then(Value::as_str);
    if output_kind.is_some_and(|value| value != "List") {
        return output_kind.map(str::to_string);
    }
    if source == "tekton" && kind == "pipeline_run_analysis" {
        return Some("PipelineRun".to_string());
    }

    first_direct_string(&[
        content.pointer("/analysis/pipeline_run/kind"),
        content.pointer("/resource"),
    ])
    .or_else(|| match (source, kind) {
        ("argocd", _) => Some("Application".to_string()),
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("prometheus", _) => Some("query".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        (_, value) if !value.trim().is_empty() => Some(value.to_string()),
        _ => None,
    })
}

pub(in crate::app) fn direct_observation_resource_name(
    content: &Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> Option<String> {
    first_direct_string(&[
        content.pointer("/name"),
        content.pointer("/output/metadata/name"),
        content.pointer("/analysis/pipeline_run/name"),
    ])
    .or_else(|| match (source, kind) {
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        _ if !subject.trim().is_empty() && !subject.contains('/') => Some(subject.to_string()),
        _ => None,
    })
}

pub(in crate::app) fn first_direct_string(values: &[Option<&Value>]) -> Option<String> {
    values
        .iter()
        .filter_map(|value| value.and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

pub(in crate::app) fn direct_observation_resource_ref(
    action_name: &str,
    source: &str,
    kind: &str,
    subject: &str,
) -> Value {
    json!({
        "source": source,
        "kind": kind,
        "name": subject,
        "metadata": {
            "capability": action_name,
            "direct": true,
        },
    })
}

pub(in crate::app) fn direct_observation_data(content: &Value) -> Value {
    let mut data = Map::new();
    for key in [
        "source",
        "resource",
        "namespace",
        "name",
        "query",
        "output",
        "response",
        "inventory",
        "analysis",
    ] {
        if let Some(value) = content.get(key) {
            data.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(data)
}

pub(in crate::app) fn direct_capability_timeout_ms(requested: Option<u64>) -> u64 {
    requested
        .unwrap_or(DEFAULT_DIRECT_CAPABILITY_TIMEOUT_MS)
        .clamp(1, MAX_DIRECT_CAPABILITY_TIMEOUT_MS)
}

pub(in crate::app) fn is_direct_capability_action(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::KubernetesGet { .. }
            | AgentAction::ArgoGetApp { .. }
            | AgentAction::PrometheusQuery { .. }
            | AgentAction::PrometheusInventory { .. }
            | AgentAction::LokiLogSummary { .. }
            | AgentAction::TektonGetPipelineRuns { .. }
            | AgentAction::TektonGetTaskRuns { .. }
            | AgentAction::TektonAnalyzePipelineRun { .. }
            | AgentAction::RegistryInspectImage { .. }
    )
}

pub(in crate::app) struct DirectCapabilityAuditInput<'a> {
    kind: &'a str,
    action: &'a AgentAction,
    decision: &'a PolicyDecision,
    executed: bool,
    cancelled: bool,
    timeout_ms: u64,
    result: Option<&'a ToolResult>,
    error: Option<&'a str>,
}

pub(in crate::app) async fn append_direct_capability_audit_event(
    store: &SqliteStore,
    input: DirectCapabilityAuditInput<'_>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!(
                "aud_direct_{}_{}",
                input.action.id().as_str(),
                unique_suffix()
            ),
            kind: input.kind.to_string(),
            actor: Some("api".to_string()),
            resource_kind: "capability".to_string(),
            resource_id: input.action.kind_name().to_string(),
            run_id: None,
            payload_json: json!({
                "action": input.action.kind_name(),
                "action_id": input.action.id().as_str(),
                "decision": input.decision,
                "executed": input.executed,
                "cancelled": input.cancelled,
                "timeout_ms": input.timeout_ms,
                "result": input.result.map(direct_capability_result_summary),
                "error": input.error.map(|value| truncate_audit_text(value, 512)),
            }),
        })
        .await
        .map(|_| ())
}

pub(in crate::app) fn direct_capability_result_summary(result: &ToolResult) -> Value {
    let mut summary = Map::new();
    summary.insert("tool_status".to_string(), json!(result.status));
    summary.insert(
        "summary".to_string(),
        Value::String(truncate_audit_text(&result.summary, 256)),
    );
    insert_cloned(&mut summary, "source", result.content.get("source"));
    insert_cloned(&mut summary, "resource", result.content.get("resource"));
    insert_cloned(
        &mut summary,
        "stdout_truncated",
        result.content.get("stdout_truncated"),
    );
    insert_object_if_not_empty(
        &mut summary,
        "output",
        select_json_paths(
            &result.content,
            &[
                ("kind", "/output/kind"),
                ("name", "/output/metadata/name"),
                ("namespace", "/output/metadata/namespace"),
                ("item_count", "/output/item_count"),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "response",
        select_json_paths(
            &result.content,
            &[
                ("result_count", "/response/data/result_count"),
                ("results_truncated", "/response/data/results_truncated"),
                ("stream_count", "/response/data/stream_count"),
                ("streams_truncated", "/response/data/streams_truncated"),
                ("entry_count", "/response/data/entry_count"),
                ("entries_truncated", "/response/data/entries_truncated"),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "inventory",
        select_json_paths(
            &result.content,
            &[
                ("active_targets", "/inventory/targets/active_count"),
                ("unhealthy_targets", "/inventory/targets/unhealthy_count"),
                ("rules", "/inventory/rules/rule_count"),
                ("problem_rules", "/inventory/rules/problem_rule_count"),
                ("alerts", "/inventory/alerts/alert_count"),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "analysis",
        select_json_paths(
            &result.content,
            &[
                ("status", "/analysis/summary/status"),
                ("task_run_count", "/analysis/summary/task_run_count"),
                (
                    "succeeded_task_runs",
                    "/analysis/summary/succeeded_task_runs",
                ),
                ("failed_task_runs", "/analysis/summary/failed_task_runs"),
                ("deployment_status", "/analysis/deployment/status"),
                ("argo_sync_status", "/analysis/argo_application/sync_status"),
                (
                    "argo_health_status",
                    "/analysis/argo_application/health_status",
                ),
                (
                    "image_alignment_status",
                    "/analysis/summary/image_alignment/status",
                ),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "image",
        select_json_paths(
            &result.content,
            &[
                ("registry", "/image/registry"),
                ("repository", "/image/repository"),
                ("tag", "/image/tag"),
                ("digest", "/image/digest"),
                ("verification_status", "/verification_status"),
                ("probe_status", "/probe/status"),
                ("probe_accessible", "/probe/accessible"),
                ("probe_digest", "/probe/digest"),
            ],
        ),
    );

    Value::Object(summary)
}

pub(in crate::app) fn select_json_paths(
    source: &Value,
    paths: &[(&str, &str)],
) -> Map<String, Value> {
    let mut selected = Map::new();
    for (key, pointer) in paths {
        insert_cloned(&mut selected, key, source.pointer(pointer));
    }
    selected
}

pub(in crate::app) fn insert_cloned(
    target: &mut Map<String, Value>,
    key: &str,
    value: Option<&Value>,
) {
    if let Some(value) = value {
        target.insert(key.to_string(), value.clone());
    }
}

pub(in crate::app) fn insert_object_if_not_empty(
    target: &mut Map<String, Value>,
    key: &str,
    value: Map<String, Value>,
) {
    if !value.is_empty() {
        target.insert(key.to_string(), Value::Object(value));
    }
}

pub(in crate::app) fn truncate_audit_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...[truncated]", &value[..end])
}
