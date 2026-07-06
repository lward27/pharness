//! Worker system prompt and tool schema shared by every attempt host.

use pharness_core::{CapabilityKind, ToolSpec};

pub fn system_prompt() -> &'static str {
    r#"You are the pharness local SDLC agent worker for lucas_engineering.
Use exactly one tool call per turn. Do not answer with prose unless you call the respond tool.
Available action tools are: respond, finish, list_dir, read_file, search_files, write_file, patch_file, run_shell, git_diff, git_status, kubernetes_get, argo_get_app, prometheus_query, prometheus_inventory, loki_log_summary, tekton_get_pipeline_runs, tekton_get_task_runs, tekton_analyze_pipeline_run.
Prefer read-only repo inspection first. Never read secrets, .env files, private keys, kubeconfigs, tokens, or credential files.
File writes, destructive commands, network commands, and production mutations are policy-gated and may pause for approval.
For available policy-gated actions, call the concrete tool. The runtime will pause for approval before execution.
Use patch_file for small existing-file text edits when an exact find/replace patch is safer than rewriting the whole file.
Use typed read-only actions for Kubernetes, Argo CD, and Prometheus inspection:
- kubernetes_get fields: resource, namespace, name, all_namespaces, label_selector.
- argo_get_app fields: app.
- prometheus_query fields: query.
- prometheus_inventory fields: none beyond reason.
- loki_log_summary fields: query, since_seconds, limit.
- tekton_get_pipeline_runs fields: namespace, name, all_namespaces, label_selector.
- tekton_get_task_runs fields: namespace, name, all_namespaces, label_selector.
- tekton_analyze_pipeline_run fields: namespace, name.
Never request Kubernetes Secret resources or secret-shaped names, labels, or metric queries.
For registry, database, or any unavailable cluster mutation, use respond to explain that the capability is not exposed yet.
When done, use finish with success and a concise summary."#
}

pub fn worker_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec::new(
            "respond",
            "Return a non-final message to the operator when more information is needed.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "message"],
                "properties": {
                    "reason": { "type": "string" },
                    "message": { "type": "string" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "finish",
            "Finish the run with a concise machine-readable summary.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "summary", "success"],
                "properties": {
                    "reason": { "type": "string" },
                    "summary": { "type": "string" },
                    "success": { "type": "boolean" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "list_dir",
            "List files and directories under a workspace path.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "depth"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 0, "maximum": 3 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "read_file",
            "Read a UTF-8 file inside the workspace. Do not read secrets or credential files.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "max_bytes": { "type": ["integer", "null"], "minimum": 1, "maximum": 262144 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "search_files",
            "Search UTF-8 files inside the workspace for a string.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" },
                    "path": { "type": ["string", "null"] },
                    "glob": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "write_file",
            "Write a UTF-8 file inside the workspace. This is policy-gated and requires approval in default mode.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "content"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "patch_file",
            "Apply an exact UTF-8 find/replace patch to an existing workspace file. This is policy-gated and requires approval in default mode.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "patch"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["find", "replace"],
                        "properties": {
                            "find": { "type": "string", "minLength": 1 },
                            "replace": { "type": "string" },
                            "replace_all": { "type": "boolean" }
                        }
                    }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "run_shell",
            "Run a policy-gated local shell command inside the workspace. Non-zero exit is returned as structured output.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "cmd", "dry_run"],
                "properties": {
                    "reason": { "type": "string" },
                    "cmd": { "type": "string" },
                    "cwd": { "type": ["string", "null"] },
                    "timeout_ms": { "type": ["integer", "null"] },
                    "dry_run": { "type": "boolean" }
                }
            }),
            CapabilityKind::Shell,
        ),
        ToolSpec::new(
            "git_status",
            "Read git status for the workspace.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" }
                }
            }),
            CapabilityKind::Git,
        ),
        ToolSpec::new(
            "git_diff",
            "Read git diff for the workspace, optionally scoped by pathspec.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" },
                    "pathspec": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::Git,
        ),
        ToolSpec::new(
            "kubernetes_get",
            "Read Kubernetes resources with kubectl get -o json. Secret-shaped resources are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "resource", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "resource": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::KubernetesRead,
        ),
        ToolSpec::new(
            "argo_get_app",
            "Read an Argo CD Application CRD from the configured Argo CD namespace.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "app"],
                "properties": {
                    "reason": { "type": "string" },
                    "app": { "type": "string" }
                }
            }),
            CapabilityKind::ArgoRead,
        ),
        ToolSpec::new(
            "prometheus_query",
            "Run a read-only Prometheus instant query against PHARNESS_PROMETHEUS_URL.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "prometheus_inventory",
            "Read bounded Prometheus targets, rules, and active alerts from PHARNESS_PROMETHEUS_URL.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "loki_log_summary",
            "Read bounded Loki log lines from PHARNESS_LOKI_URL with compacted, redacted output.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" },
                    "since_seconds": {
                        "type": ["integer", "null"],
                        "minimum": 60,
                        "maximum": 86400
                    },
                    "limit": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "maximum": 100
                    }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "tekton_get_pipeline_runs",
            "Read Tekton PipelineRuns through the Kubernetes API. Secret-shaped names and labels are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::TektonRead,
        ),
        ToolSpec::new(
            "tekton_get_task_runs",
            "Read Tekton TaskRuns through the Kubernetes API. Secret-shaped names and labels are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::TektonRead,
        ),
        ToolSpec::new(
            "tekton_analyze_pipeline_run",
            "Read one Tekton PipelineRun and its related TaskRuns, then return a normalized PipelineRunAnalysis summary.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "namespace", "name"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": "string" },
                    "name": { "type": "string" }
                }
            }),
            CapabilityKind::TektonRead,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::worker_tool_specs;
    use std::collections::HashSet;

    #[test]
    fn worker_tool_schema_contains_terminal_and_read_only_actions() {
        let names = worker_tool_specs()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        for expected in [
            "respond",
            "finish",
            "list_dir",
            "read_file",
            "search_files",
            "write_file",
            "patch_file",
            "run_shell",
            "git_status",
            "git_diff",
            "kubernetes_get",
            "argo_get_app",
            "prometheus_query",
            "prometheus_inventory",
            "loki_log_summary",
            "tekton_get_pipeline_runs",
            "tekton_get_task_runs",
            "tekton_analyze_pipeline_run",
        ] {
            assert!(names.contains(expected), "missing tool spec for {expected}");
        }
    }

    #[test]
    fn worker_tool_schema_does_not_expose_non_resumable_approval_by_default() {
        let names = worker_tool_specs()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        assert!(!names.contains("request_approval"));
    }
}
