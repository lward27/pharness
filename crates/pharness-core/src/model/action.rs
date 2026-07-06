use crate::ActionId;
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentAction {
    Respond {
        id: ActionId,
        reason: String,
        message: String,
    },
    ReadFile {
        id: ActionId,
        reason: String,
        path: Utf8PathBuf,
        max_bytes: Option<u64>,
    },
    WriteFile {
        id: ActionId,
        reason: String,
        path: Utf8PathBuf,
        content: String,
    },
    PatchFile {
        id: ActionId,
        reason: String,
        path: Utf8PathBuf,
        patch: TextPatch,
    },
    ListDir {
        id: ActionId,
        reason: String,
        path: Utf8PathBuf,
        depth: u8,
    },
    SearchFiles {
        id: ActionId,
        reason: String,
        query: String,
        path: Option<Utf8PathBuf>,
        glob: Option<String>,
    },
    RunShell {
        id: ActionId,
        reason: String,
        cmd: String,
        cwd: Option<Utf8PathBuf>,
        timeout_ms: Option<u64>,
        dry_run: bool,
    },
    GitDiff {
        id: ActionId,
        reason: String,
        pathspec: Option<String>,
    },
    GitStatus {
        id: ActionId,
        reason: String,
    },
    KubernetesGet {
        id: ActionId,
        reason: String,
        resource: String,
        namespace: Option<String>,
        name: Option<String>,
        all_namespaces: bool,
        label_selector: Option<String>,
    },
    ArgoGetApp {
        id: ActionId,
        reason: String,
        app: String,
    },
    PrometheusQuery {
        id: ActionId,
        reason: String,
        query: String,
    },
    PrometheusInventory {
        id: ActionId,
        reason: String,
    },
    LokiLogSummary {
        id: ActionId,
        reason: String,
        query: String,
        since_seconds: Option<u64>,
        limit: Option<u32>,
    },
    TektonGetPipelineRuns {
        id: ActionId,
        reason: String,
        namespace: Option<String>,
        name: Option<String>,
        all_namespaces: bool,
        label_selector: Option<String>,
    },
    TektonGetTaskRuns {
        id: ActionId,
        reason: String,
        namespace: Option<String>,
        name: Option<String>,
        all_namespaces: bool,
        label_selector: Option<String>,
    },
    TektonAnalyzePipelineRun {
        id: ActionId,
        reason: String,
        namespace: String,
        name: String,
    },
    RegistryInspectImage {
        id: ActionId,
        reason: String,
        image_ref: String,
        registry_base_url: Option<String>,
    },
    RequestApproval {
        id: ActionId,
        reason: String,
        approval_kind: ApprovalKind,
        summary: String,
    },
    Finish {
        id: ActionId,
        reason: String,
        summary: String,
        success: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPatch {
    pub find: String,
    pub replace: String,
    #[serde(default)]
    pub replace_all: bool,
}

impl AgentAction {
    pub fn id(&self) -> &ActionId {
        match self {
            Self::Respond { id, .. }
            | Self::ReadFile { id, .. }
            | Self::WriteFile { id, .. }
            | Self::PatchFile { id, .. }
            | Self::ListDir { id, .. }
            | Self::SearchFiles { id, .. }
            | Self::RunShell { id, .. }
            | Self::GitDiff { id, .. }
            | Self::GitStatus { id, .. }
            | Self::KubernetesGet { id, .. }
            | Self::ArgoGetApp { id, .. }
            | Self::PrometheusQuery { id, .. }
            | Self::PrometheusInventory { id, .. }
            | Self::LokiLogSummary { id, .. }
            | Self::TektonGetPipelineRuns { id, .. }
            | Self::TektonGetTaskRuns { id, .. }
            | Self::TektonAnalyzePipelineRun { id, .. }
            | Self::RegistryInspectImage { id, .. }
            | Self::RequestApproval { id, .. }
            | Self::Finish { id, .. } => id,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Respond { .. } => "respond",
            Self::ReadFile { .. } => "read_file",
            Self::WriteFile { .. } => "write_file",
            Self::PatchFile { .. } => "patch_file",
            Self::ListDir { .. } => "list_dir",
            Self::SearchFiles { .. } => "search_files",
            Self::RunShell { .. } => "run_shell",
            Self::GitDiff { .. } => "git_diff",
            Self::GitStatus { .. } => "git_status",
            Self::KubernetesGet { .. } => "kubernetes_get",
            Self::ArgoGetApp { .. } => "argo_get_app",
            Self::PrometheusQuery { .. } => "prometheus_query",
            Self::PrometheusInventory { .. } => "prometheus_inventory",
            Self::LokiLogSummary { .. } => "loki_log_summary",
            Self::TektonGetPipelineRuns { .. } => "tekton_get_pipeline_runs",
            Self::TektonGetTaskRuns { .. } => "tekton_get_task_runs",
            Self::TektonAnalyzePipelineRun { .. } => "tekton_analyze_pipeline_run",
            Self::RegistryInspectImage { .. } => "registry_inspect_image",
            Self::RequestApproval { .. } => "request_approval",
            Self::Finish { .. } => "finish",
        }
    }

    pub fn from_tool_call(
        function_name: &str,
        fallback_id: impl Into<String>,
        arguments: &str,
    ) -> Result<Self, ActionParseError> {
        let mut value: serde_json::Value =
            serde_json::from_str(arguments).map_err(|error| ActionParseError::InvalidJson {
                message: error.to_string(),
            })?;

        let serde_json::Value::Object(ref mut object) = value else {
            return Err(ActionParseError::ExpectedObject);
        };

        object
            .entry("action".to_string())
            .or_insert_with(|| serde_json::Value::String(function_name.to_string()));
        object
            .entry("id".to_string())
            .or_insert_with(|| serde_json::Value::String(fallback_id.into()));

        serde_json::from_value(value).map_err(|error| ActionParseError::InvalidAction {
            message: error.to_string(),
        })
    }

    pub fn from_json_text(text: &str) -> Result<Self, ActionParseError> {
        let normalized = normalize_json_text(text);
        serde_json::from_str(normalized).map_err(|error| ActionParseError::InvalidAction {
            message: error.to_string(),
        })
    }

    pub fn provider_respond(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Respond {
            id: ActionId::new(id),
            reason: "Provider returned assistant content without a tool call".to_string(),
            message: message.into(),
        }
    }
}

fn normalize_json_text(text: &str) -> &str {
    let trimmed = text.trim();

    if let Some(stripped) = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
    {
        return stripped.trim();
    }

    if let Some(stripped) = trimmed
        .strip_prefix("```")
        .and_then(|value| value.strip_suffix("```"))
    {
        return stripped.trim();
    }

    trimmed
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ActionParseError {
    #[error("action arguments must be a JSON object")]
    ExpectedObject,
    #[error("invalid action JSON: {message}")]
    InvalidJson { message: String },
    #[error("invalid action payload: {message}")]
    InvalidAction { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    FileWrite,
    ShellCommand,
    Network,
    Destructive,
    Privileged,
    SecretAccess,
    RegistryWrite,
    TektonRun,
    ArgoSync,
    DatabaseChange,
    ProductionChange,
}

#[cfg(test)]
mod tests {
    use super::{ActionParseError, AgentAction};

    #[test]
    fn deserializes_structured_run_shell_action() {
        let action: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "run_shell",
            "id": "act_test",
            "reason": "Run tests",
            "cmd": "cargo test --workspace",
            "cwd": ".",
            "timeout_ms": 120000,
            "dry_run": false
        }))
        .unwrap();

        match action {
            AgentAction::RunShell {
                id,
                cmd,
                timeout_ms,
                ..
            } => {
                assert_eq!(id.as_str(), "act_test");
                assert_eq!(cmd, "cargo test --workspace");
                assert_eq!(timeout_ms, Some(120000));
            }
            other => panic!("expected run_shell action, got {other:?}"),
        }
    }

    #[test]
    fn builds_action_from_native_tool_call_arguments() {
        let action = AgentAction::from_tool_call(
            "read_file",
            "call_abc",
            r#"{"reason":"Inspect manifest","path":"Cargo.toml","max_bytes":1000}"#,
        )
        .unwrap();

        match action {
            AgentAction::ReadFile {
                id,
                reason,
                path,
                max_bytes,
            } => {
                assert_eq!(id.as_str(), "call_abc");
                assert_eq!(reason, "Inspect manifest");
                assert_eq!(path.as_str(), "Cargo.toml");
                assert_eq!(max_bytes, Some(1000));
            }
            other => panic!("expected read_file action, got {other:?}"),
        }
    }

    #[test]
    fn parses_fenced_json_action_text() {
        let action = AgentAction::from_json_text(
            r#"```json
{"action":"finish","id":"act_done","reason":"Done","summary":"Complete","success":true}
```"#,
        )
        .unwrap();

        match action {
            AgentAction::Finish { success, .. } => assert!(success),
            other => panic!("expected finish action, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_object_tool_arguments() {
        let error = AgentAction::from_tool_call("read_file", "call_abc", "[]").unwrap_err();
        assert_eq!(error, ActionParseError::ExpectedObject);
    }

    #[test]
    fn deserializes_typed_cluster_read_actions() {
        let kube: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "kubernetes_get",
            "id": "act_kube",
            "reason": "inspect pods",
            "resource": "pods",
            "namespace": "argocd",
            "name": null,
            "all_namespaces": false,
            "label_selector": "app.kubernetes.io/name=argocd-server"
        }))
        .unwrap();
        assert_eq!(kube.kind_name(), "kubernetes_get");

        let argo: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "argo_get_app",
            "id": "act_argo",
            "reason": "inspect app",
            "app": "openclaw"
        }))
        .unwrap();
        assert_eq!(argo.kind_name(), "argo_get_app");

        let prom: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "prometheus_query",
            "id": "act_prom",
            "reason": "check cpu",
            "query": "up"
        }))
        .unwrap();
        assert_eq!(prom.kind_name(), "prometheus_query");

        let prom_inventory: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "prometheus_inventory",
            "id": "act_prom_inventory",
            "reason": "check observability health"
        }))
        .unwrap();
        assert_eq!(prom_inventory.kind_name(), "prometheus_inventory");

        let loki: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "loki_log_summary",
            "id": "act_loki",
            "reason": "check recent app logs",
            "query": "{namespace=\"apps-dev\"}",
            "since_seconds": 900,
            "limit": 25
        }))
        .unwrap();
        assert_eq!(loki.kind_name(), "loki_log_summary");

        let tekton: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "tekton_get_pipeline_runs",
            "id": "act_tekton",
            "reason": "inspect recent pipeline runs",
            "namespace": "ci",
            "name": null,
            "all_namespaces": false,
            "label_selector": null
        }))
        .unwrap();
        assert_eq!(tekton.kind_name(), "tekton_get_pipeline_runs");

        let task_runs: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "tekton_get_task_runs",
            "id": "act_tekton_tasks",
            "reason": "inspect recent task runs",
            "namespace": "ci",
            "name": null,
            "all_namespaces": false,
            "label_selector": null
        }))
        .unwrap();
        assert_eq!(task_runs.kind_name(), "tekton_get_task_runs");

        let analysis: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "tekton_analyze_pipeline_run",
            "id": "act_tekton_analysis",
            "reason": "analyze pipeline result",
            "namespace": "ci",
            "name": "build-app"
        }))
        .unwrap();
        assert_eq!(analysis.kind_name(), "tekton_analyze_pipeline_run");

        let registry: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "registry_inspect_image",
            "id": "act_registry",
            "reason": "inspect image evidence",
            "image_ref": "registry.example.test/team/checkout-api:v1",
            "registry_base_url": "https://registry.example.test"
        }))
        .unwrap();
        assert_eq!(registry.kind_name(), "registry_inspect_image");
    }

    #[test]
    fn deserializes_structured_patch_action() {
        let action: AgentAction = serde_json::from_value(serde_json::json!({
            "action": "patch_file",
            "id": "act_patch",
            "reason": "update text",
            "path": "README.md",
            "patch": {
                "find": "old",
                "replace": "new"
            }
        }))
        .unwrap();

        match action {
            AgentAction::PatchFile { patch, .. } => {
                assert_eq!(patch.find, "old");
                assert_eq!(patch.replace, "new");
                assert!(!patch.replace_all);
            }
            other => panic!("expected patch_file action, got {other:?}"),
        }
    }
}
