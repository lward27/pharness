use super::{ToolError, ToolExecutor, ToolResult};
use crate::AgentAction;
use async_trait::async_trait;
use serde_json::Map;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_PROMETHEUS_RESULTS: usize = 20;

#[derive(Debug, Clone)]
pub struct ReadOnlyClusterTools {
    kubectl_bin: String,
    argocd_namespace: String,
    prometheus_url: Option<String>,
    timeout_ms: u64,
    max_output_bytes: usize,
    http: reqwest::Client,
}

impl Default for ReadOnlyClusterTools {
    fn default() -> Self {
        Self {
            kubectl_bin: "kubectl".to_string(),
            argocd_namespace: "argocd".to_string(),
            prometheus_url: None,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            http: reqwest::Client::new(),
        }
    }
}

impl ReadOnlyClusterTools {
    pub fn from_env() -> Self {
        Self {
            kubectl_bin: std::env::var("PHARNESS_KUBECTL_BIN")
                .unwrap_or_else(|_| "kubectl".to_string()),
            argocd_namespace: std::env::var("PHARNESS_ARGOCD_NAMESPACE")
                .unwrap_or_else(|_| "argocd".to_string()),
            prometheus_url: std::env::var("PHARNESS_PROMETHEUS_URL").ok(),
            timeout_ms: std::env::var("PHARNESS_CLUSTER_TOOL_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            max_output_bytes: std::env::var("PHARNESS_CLUSTER_TOOL_MAX_OUTPUT_BYTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES),
            http: reqwest::Client::new(),
        }
    }

    pub fn with_prometheus_url(mut self, url: impl Into<String>) -> Self {
        self.prometheus_url = Some(url.into());
        self
    }

    async fn kubernetes_get(
        &self,
        resource: &str,
        namespace: Option<&str>,
        name: Option<&str>,
        all_namespaces: bool,
        label_selector: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        reject_secretish("resource", resource)?;
        validate_kubernetes_name("resource", resource, true)?;
        if let Some(namespace) = namespace {
            reject_secretish("namespace", namespace)?;
            validate_kubernetes_name("namespace", namespace, false)?;
        }
        if let Some(name) = name {
            reject_secretish("name", name)?;
            validate_kubernetes_name("name", name, true)?;
        }
        if let Some(label_selector) = label_selector {
            reject_secretish("label_selector", label_selector)?;
            validate_label_selector(label_selector)?;
        }

        let mut args = vec!["get".to_string(), resource.to_string()];
        if let Some(name) = name {
            args.push(name.to_string());
        }
        if all_namespaces {
            args.push("--all-namespaces".to_string());
        } else if let Some(namespace) = namespace {
            args.push("-n".to_string());
            args.push(namespace.to_string());
        }
        if let Some(label_selector) = label_selector {
            args.push("-l".to_string());
            args.push(label_selector.to_string());
        }
        args.push("-o".to_string());
        args.push("json".to_string());

        let output = self.run_command(&self.kubectl_bin, &args).await?;
        Ok(ToolResult::ok(
            format!("read kubernetes {resource}"),
            serde_json::json!({
                "source": "kubernetes",
                "command": command_summary(&self.kubectl_bin, &args),
                "stdout_truncated": output.stdout_truncated,
                "output": compact_kubernetes_output(&output.stdout),
            }),
        ))
    }

    async fn argo_get_app(&self, app: &str) -> Result<ToolResult, ToolError> {
        reject_secretish("app", app)?;
        validate_kubernetes_name("app", app, true)?;
        reject_secretish("argocd_namespace", &self.argocd_namespace)?;
        validate_kubernetes_name("argocd_namespace", &self.argocd_namespace, false)?;

        let args = vec![
            "get".to_string(),
            "applications.argoproj.io".to_string(),
            app.to_string(),
            "-n".to_string(),
            self.argocd_namespace.clone(),
            "-o".to_string(),
            "json".to_string(),
        ];

        let output = self.run_command(&self.kubectl_bin, &args).await?;
        Ok(ToolResult::ok(
            format!("read Argo CD app {app}"),
            serde_json::json!({
                "source": "argocd",
                "command": command_summary(&self.kubectl_bin, &args),
                "stdout_truncated": output.stdout_truncated,
                "output": compact_kubernetes_output(&output.stdout),
            }),
        ))
    }

    async fn tekton_get_pipeline_runs(
        &self,
        namespace: Option<&str>,
        name: Option<&str>,
        all_namespaces: bool,
        label_selector: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        self.tekton_get_runs(
            TektonRunResource::PipelineRun,
            namespace,
            name,
            all_namespaces,
            label_selector,
        )
        .await
    }

    async fn tekton_get_task_runs(
        &self,
        namespace: Option<&str>,
        name: Option<&str>,
        all_namespaces: bool,
        label_selector: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        self.tekton_get_runs(
            TektonRunResource::TaskRun,
            namespace,
            name,
            all_namespaces,
            label_selector,
        )
        .await
    }

    async fn tekton_analyze_pipeline_run(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<ToolResult, ToolError> {
        reject_secretish("namespace", namespace)?;
        validate_kubernetes_name("namespace", namespace, false)?;
        reject_secretish("name", name)?;
        validate_kubernetes_name("name", name, true)?;

        let pipeline_args = vec![
            "get".to_string(),
            TektonRunResource::PipelineRun.crd().to_string(),
            name.to_string(),
            "-n".to_string(),
            namespace.to_string(),
            "-o".to_string(),
            "json".to_string(),
        ];
        let pipeline_output = self.run_command(&self.kubectl_bin, &pipeline_args).await?;
        let mut pipeline_run = parse_json_output(&pipeline_output.stdout)?;
        redact_json(&mut pipeline_run);

        let task_selector = format!("tekton.dev/pipelineRun={name}");
        let task_args = vec![
            "get".to_string(),
            TektonRunResource::TaskRun.crd().to_string(),
            "-n".to_string(),
            namespace.to_string(),
            "-l".to_string(),
            task_selector,
            "-o".to_string(),
            "json".to_string(),
        ];
        let task_output = self.run_command(&self.kubectl_bin, &task_args).await?;
        let mut task_runs = parse_json_output(&task_output.stdout)?;
        redact_json(&mut task_runs);
        let deployment_lookup = self.lookup_pipeline_deployment(&pipeline_run).await;
        let deployment_command = deployment_lookup
            .command
            .as_ref()
            .map(|args| command_summary(&self.kubectl_bin, args));
        let argo_lookup = self
            .lookup_related_argo_application(&deployment_lookup.observation)
            .await;
        let argo_command = argo_lookup
            .command
            .as_ref()
            .map(|args| command_summary(&self.kubectl_bin, args));

        Ok(ToolResult::ok(
            format!("analyzed Tekton PipelineRun {namespace}/{name}"),
            serde_json::json!({
                "source": "tekton",
                "resource": "pipeline_run_analysis",
                "commands": {
                    "pipeline_run": command_summary(&self.kubectl_bin, &pipeline_args),
                    "task_runs": command_summary(&self.kubectl_bin, &task_args),
                    "deployment": deployment_command,
                    "argo_application": argo_command,
                },
                "stdout_truncated": pipeline_output.stdout_truncated || task_output.stdout_truncated,
                "analysis": build_pipeline_run_analysis(
                    &pipeline_run,
                    &task_runs,
                    deployment_lookup.observation,
                    argo_lookup.observation,
                ),
            }),
        ))
    }

    async fn lookup_pipeline_deployment(&self, pipeline_run: &Value) -> RelatedResourceLookup {
        let params = extract_pipeline_params(pipeline_run);
        let Some(Value::String(name)) = params.get("deployment") else {
            return RelatedResourceLookup::skipped(
                "PipelineRun did not declare a deployment target",
            );
        };
        let Some(Value::String(namespace)) = params.get("deployment-namespace") else {
            return RelatedResourceLookup::skipped(
                "PipelineRun did not declare a deployment namespace",
            );
        };
        if name.is_empty() {
            return RelatedResourceLookup::skipped("PipelineRun deployment target is empty");
        }
        if reject_secretish("deployment", name)
            .and_then(|_| validate_kubernetes_name("deployment", name, true))
            .is_err()
        {
            return RelatedResourceLookup::skipped("PipelineRun deployment target is not allowed");
        }
        if reject_secretish("deployment_namespace", namespace)
            .and_then(|_| validate_kubernetes_name("deployment_namespace", namespace, false))
            .is_err()
        {
            return RelatedResourceLookup::skipped(
                "PipelineRun deployment namespace is not allowed",
            );
        }

        let args = vec![
            "get".to_string(),
            "deployment.apps".to_string(),
            name.clone(),
            "-n".to_string(),
            namespace.clone(),
            "-o".to_string(),
            "json".to_string(),
        ];

        match self.run_command(&self.kubectl_bin, &args).await {
            Ok(output) => match parse_json_output(&output.stdout) {
                Ok(mut deployment) => {
                    redact_json(&mut deployment);
                    RelatedResourceLookup {
                        command: Some(args),
                        observation: analyze_deployment(&deployment),
                    }
                }
                Err(error) => RelatedResourceLookup {
                    command: Some(args),
                    observation: serde_json::json!({
                        "status": "error",
                        "error": error.to_string(),
                    }),
                },
            },
            Err(error) => RelatedResourceLookup {
                command: Some(args),
                observation: serde_json::json!({
                    "status": "error",
                    "error": error.to_string(),
                }),
            },
        }
    }

    async fn lookup_related_argo_application(&self, deployment: &Value) -> RelatedResourceLookup {
        let Some(app) = deployment.get("argo_application").and_then(Value::as_str) else {
            return RelatedResourceLookup::skipped(
                "Deployment is not linked to an Argo CD Application",
            );
        };
        if app.is_empty() {
            return RelatedResourceLookup::skipped("Deployment Argo CD Application is empty");
        }
        if reject_secretish("argo_application", app)
            .and_then(|_| validate_kubernetes_name("argo_application", app, true))
            .is_err()
        {
            return RelatedResourceLookup::skipped("Deployment Argo CD Application is not allowed");
        }
        if reject_secretish("argocd_namespace", &self.argocd_namespace)
            .and_then(|_| {
                validate_kubernetes_name("argocd_namespace", &self.argocd_namespace, false)
            })
            .is_err()
        {
            return RelatedResourceLookup::skipped("Configured Argo CD namespace is not allowed");
        }

        let args = vec![
            "get".to_string(),
            "applications.argoproj.io".to_string(),
            app.to_string(),
            "-n".to_string(),
            self.argocd_namespace.clone(),
            "-o".to_string(),
            "json".to_string(),
        ];

        match self.run_command(&self.kubectl_bin, &args).await {
            Ok(output) => match parse_json_output(&output.stdout) {
                Ok(mut application) => {
                    redact_json(&mut application);
                    RelatedResourceLookup {
                        command: Some(args),
                        observation: analyze_argo_application(&application),
                    }
                }
                Err(error) => RelatedResourceLookup {
                    command: Some(args),
                    observation: serde_json::json!({
                        "status": "error",
                        "error": error.to_string(),
                    }),
                },
            },
            Err(error) => RelatedResourceLookup {
                command: Some(args),
                observation: serde_json::json!({
                    "status": "error",
                    "error": error.to_string(),
                }),
            },
        }
    }

    async fn tekton_get_runs(
        &self,
        resource: TektonRunResource,
        namespace: Option<&str>,
        name: Option<&str>,
        all_namespaces: bool,
        label_selector: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        if let Some(namespace) = namespace {
            reject_secretish("namespace", namespace)?;
            validate_kubernetes_name("namespace", namespace, false)?;
        }
        if let Some(name) = name {
            reject_secretish("name", name)?;
            validate_kubernetes_name("name", name, true)?;
        }
        if let Some(label_selector) = label_selector {
            reject_secretish("label_selector", label_selector)?;
            validate_label_selector(label_selector)?;
        }

        let mut args = vec!["get".to_string(), resource.crd().to_string()];
        if let Some(name) = name {
            args.push(name.to_string());
        }
        if all_namespaces {
            args.push("--all-namespaces".to_string());
        } else if let Some(namespace) = namespace {
            args.push("-n".to_string());
            args.push(namespace.to_string());
        }
        if let Some(label_selector) = label_selector {
            args.push("-l".to_string());
            args.push(label_selector.to_string());
        }
        args.push("-o".to_string());
        args.push("json".to_string());

        let output = self.run_command(&self.kubectl_bin, &args).await?;
        Ok(ToolResult::ok(
            resource.summary(),
            serde_json::json!({
                "source": "tekton",
                "resource": resource.crd(),
                "command": command_summary(&self.kubectl_bin, &args),
                "stdout_truncated": output.stdout_truncated,
                "output": compact_kubernetes_output(&output.stdout),
            }),
        ))
    }

    async fn prometheus_query(&self, query: &str) -> Result<ToolResult, ToolError> {
        reject_secretish("query", query)?;
        let Some(base_url) = &self.prometheus_url else {
            return Err(ToolError::InvalidArguments {
                message: "PHARNESS_PROMETHEUS_URL is not configured".to_string(),
            });
        };

        let url = format!("{}/api/v1/query", base_url.trim_end_matches('/'));
        let response = self
            .http
            .get(&url)
            .query(&[("query", query)])
            .send()
            .await
            .map_err(|error| ToolError::Network {
                message: error.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Network {
                message: format!(
                    "Prometheus returned HTTP {status}: {}",
                    truncate(&body, 4096).0
                ),
            });
        }

        let mut body: Value = response.json().await.map_err(|error| ToolError::Network {
            message: error.to_string(),
        })?;
        redact_json(&mut body);
        let body = compact_prometheus_response(&body);

        Ok(ToolResult::ok(
            "read Prometheus instant query",
            serde_json::json!({
                "source": "prometheus",
                "query": query,
                "response": body,
            }),
        ))
    }

    async fn run_command(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<CommandOutput, ToolError> {
        let command = command_summary(program, args);
        let mut process = Command::new(program);
        process.args(args).kill_on_drop(true);

        let output = timeout(Duration::from_millis(self.timeout_ms), process.output())
            .await
            .map_err(|_| ToolError::TimedOut {
                command: command.clone(),
                timeout_ms: self.timeout_ms,
            })?
            .map_err(|error| ToolError::Io {
                message: format!("failed to run {command}: {error}"),
            })?;

        let (stdout, stdout_truncated) = truncate(
            &String::from_utf8_lossy(&output.stdout),
            self.max_output_bytes,
        );
        let (stderr, _) = truncate(&String::from_utf8_lossy(&output.stderr), 32 * 1024);

        if !output.status.success() {
            return Err(ToolError::CommandFailed {
                command,
                status: output.status.to_string(),
                stderr: redact_text(&stderr),
            });
        }

        Ok(CommandOutput {
            stdout,
            stdout_truncated,
        })
    }
}

#[async_trait]
impl ToolExecutor for ReadOnlyClusterTools {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        match action {
            AgentAction::KubernetesGet {
                resource,
                namespace,
                name,
                all_namespaces,
                label_selector,
                ..
            } => {
                self.kubernetes_get(
                    resource,
                    namespace.as_deref(),
                    name.as_deref(),
                    *all_namespaces,
                    label_selector.as_deref(),
                )
                .await
            }
            AgentAction::ArgoGetApp { app, .. } => self.argo_get_app(app).await,
            AgentAction::PrometheusQuery { query, .. } => self.prometheus_query(query).await,
            AgentAction::TektonGetPipelineRuns {
                namespace,
                name,
                all_namespaces,
                label_selector,
                ..
            } => {
                self.tekton_get_pipeline_runs(
                    namespace.as_deref(),
                    name.as_deref(),
                    *all_namespaces,
                    label_selector.as_deref(),
                )
                .await
            }
            AgentAction::TektonGetTaskRuns {
                namespace,
                name,
                all_namespaces,
                label_selector,
                ..
            } => {
                self.tekton_get_task_runs(
                    namespace.as_deref(),
                    name.as_deref(),
                    *all_namespaces,
                    label_selector.as_deref(),
                )
                .await
            }
            AgentAction::TektonAnalyzePipelineRun {
                namespace, name, ..
            } => self.tekton_analyze_pipeline_run(namespace, name).await,
            other => Err(ToolError::UnsupportedAction {
                action: other.kind_name().to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TektonRunResource {
    PipelineRun,
    TaskRun,
}

impl TektonRunResource {
    fn crd(self) -> &'static str {
        match self {
            Self::PipelineRun => "pipelineruns.tekton.dev",
            Self::TaskRun => "taskruns.tekton.dev",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::PipelineRun => "read Tekton PipelineRuns",
            Self::TaskRun => "read Tekton TaskRuns",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RelatedResourceLookup {
    command: Option<Vec<String>>,
    observation: Value,
}

impl RelatedResourceLookup {
    fn skipped(reason: impl Into<String>) -> Self {
        Self {
            command: None,
            observation: serde_json::json!({
                "status": "skipped",
                "reason": reason.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutput {
    stdout: String,
    stdout_truncated: bool,
}

fn command_summary(program: &str, args: &[String]) -> String {
    let program = Path::new(program)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or(program);

    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_kubernetes_output(stdout: &str) -> Value {
    match serde_json::from_str::<Value>(stdout) {
        Ok(mut value) => {
            redact_json(&mut value);
            compact_kubernetes_value(&value)
        }
        Err(_) => serde_json::json!({ "text": redact_text(stdout) }),
    }
}

fn parse_json_output(stdout: &str) -> Result<Value, ToolError> {
    serde_json::from_str(stdout).map_err(|error| ToolError::InvalidArguments {
        message: format!("kubectl returned invalid JSON: {error}"),
    })
}

fn compact_kubernetes_value(value: &Value) -> Value {
    if value.get("kind").and_then(Value::as_str) == Some("List") {
        let items = value
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        return serde_json::json!({
            "apiVersion": value.get("apiVersion").cloned().unwrap_or(Value::Null),
            "kind": "List",
            "item_count": items.len(),
            "items": items.iter().map(compact_kubernetes_resource).collect::<Vec<_>>(),
        });
    }

    compact_kubernetes_resource(value)
}

fn compact_kubernetes_resource(value: &Value) -> Value {
    let mut resource = Map::new();
    copy_string(value, &mut resource, "apiVersion", "/apiVersion");
    copy_string(value, &mut resource, "kind", "/kind");

    if let Some(metadata) = compact_metadata(value) {
        resource.insert("metadata".to_string(), metadata);
    }
    if let Some(status) = compact_status(value) {
        resource.insert("status".to_string(), status);
    }
    if let Some(spec) = compact_spec(value) {
        resource.insert("spec".to_string(), spec);
    }

    Value::Object(resource)
}

fn compact_metadata(value: &Value) -> Option<Value> {
    let mut metadata = Map::new();
    copy_string(value, &mut metadata, "name", "/metadata/name");
    copy_string(value, &mut metadata, "namespace", "/metadata/namespace");
    copy_string(
        value,
        &mut metadata,
        "creationTimestamp",
        "/metadata/creationTimestamp",
    );

    non_empty_object(metadata)
}

fn compact_status(value: &Value) -> Option<Value> {
    match value.get("kind").and_then(Value::as_str) {
        Some("Pod") => compact_pod_status(value),
        Some("Deployment" | "StatefulSet" | "ReplicaSet" | "DaemonSet") => {
            compact_workload_status(value)
        }
        Some("PipelineRun" | "TaskRun") => compact_tekton_run_status(value),
        Some("Service") => None,
        Some("Ingress") => compact_ingress_status(value),
        _ => compact_generic_status(value),
    }
}

fn compact_pod_status(value: &Value) -> Option<Value> {
    let mut status = Map::new();
    copy_string(value, &mut status, "phase", "/status/phase");

    if let Some(ready) = pod_ready(value) {
        status.insert("ready".to_string(), Value::Bool(ready));
    }

    if let Some(container_statuses) = value
        .pointer("/status/containerStatuses")
        .and_then(Value::as_array)
    {
        let ready_count = container_statuses
            .iter()
            .filter(|container| {
                container
                    .get("ready")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        let restart_count = container_statuses
            .iter()
            .filter_map(|container| container.get("restartCount").and_then(Value::as_u64))
            .sum::<u64>();
        let containers = container_statuses
            .iter()
            .map(compact_container_status)
            .collect::<Vec<_>>();

        status.insert(
            "containers_ready".to_string(),
            serde_json::json!({
                "ready": ready_count,
                "total": container_statuses.len(),
            }),
        );
        status.insert("restart_count".to_string(), Value::from(restart_count));
        status.insert("containers".to_string(), Value::Array(containers));
    }

    non_empty_object(status)
}

fn compact_container_status(value: &Value) -> Value {
    let mut container = Map::new();
    copy_string(value, &mut container, "name", "/name");
    copy_bool(value, &mut container, "ready", "/ready");
    copy_u64(value, &mut container, "restartCount", "/restartCount");

    if let Some(state) = value.get("state").and_then(Value::as_object) {
        if let Some(state_name) = state.keys().next() {
            container.insert("state".to_string(), Value::String(state_name.clone()));
        }
    }

    Value::Object(container)
}

fn compact_workload_status(value: &Value) -> Option<Value> {
    let mut status = Map::new();
    copy_u64(value, &mut status, "replicas", "/status/replicas");
    copy_u64(value, &mut status, "readyReplicas", "/status/readyReplicas");
    copy_u64(
        value,
        &mut status,
        "availableReplicas",
        "/status/availableReplicas",
    );
    copy_u64(
        value,
        &mut status,
        "updatedReplicas",
        "/status/updatedReplicas",
    );

    non_empty_object(status)
}

fn compact_tekton_run_status(value: &Value) -> Option<Value> {
    let mut status = Map::new();
    copy_string(value, &mut status, "startTime", "/status/startTime");
    copy_string(
        value,
        &mut status,
        "completionTime",
        "/status/completionTime",
    );
    if let Some(conditions) = compact_conditions(value) {
        status.insert("conditions".to_string(), conditions);
    }

    non_empty_object(status)
}

fn compact_ingress_status(value: &Value) -> Option<Value> {
    let load_balancer = value.pointer("/status/loadBalancer")?;
    Some(serde_json::json!({ "loadBalancer": load_balancer }))
}

fn compact_generic_status(value: &Value) -> Option<Value> {
    let mut status = Map::new();
    copy_string(value, &mut status, "phase", "/status/phase");

    if let Some(health) = value.pointer("/status/health") {
        status.insert("health".to_string(), health.clone());
    }
    if let Some(sync) = value.pointer("/status/sync") {
        status.insert("sync".to_string(), sync.clone());
    }
    if let Some(conditions) = compact_conditions(value) {
        status.insert("conditions".to_string(), conditions);
    }

    non_empty_object(status)
}

fn compact_spec(value: &Value) -> Option<Value> {
    match value.get("kind").and_then(Value::as_str) {
        Some("Service") => compact_service_spec(value),
        Some("Ingress") => compact_ingress_spec(value),
        _ => None,
    }
}

fn compact_service_spec(value: &Value) -> Option<Value> {
    let mut spec = Map::new();
    copy_string(value, &mut spec, "type", "/spec/type");
    copy_string(value, &mut spec, "clusterIP", "/spec/clusterIP");
    if let Some(ports) = value.pointer("/spec/ports").and_then(Value::as_array) {
        spec.insert(
            "ports".to_string(),
            Value::Array(ports.iter().map(compact_service_port).collect()),
        );
    }

    non_empty_object(spec)
}

fn compact_service_port(value: &Value) -> Value {
    let mut port = Map::new();
    copy_string(value, &mut port, "name", "/name");
    copy_string(value, &mut port, "protocol", "/protocol");
    copy_u64(value, &mut port, "port", "/port");
    copy_u64(value, &mut port, "targetPort", "/targetPort");

    Value::Object(port)
}

fn compact_ingress_spec(value: &Value) -> Option<Value> {
    let rules = value.pointer("/spec/rules").and_then(Value::as_array)?;
    let hosts = rules
        .iter()
        .filter_map(|rule| rule.get("host").and_then(Value::as_str))
        .map(Value::from)
        .collect::<Vec<_>>();

    Some(serde_json::json!({ "hosts": hosts }))
}

fn compact_conditions(value: &Value) -> Option<Value> {
    let conditions = value.pointer("/status/conditions")?.as_array()?;
    let compacted = conditions
        .iter()
        .map(|condition| {
            let mut output = Map::new();
            copy_string(condition, &mut output, "type", "/type");
            copy_string(condition, &mut output, "status", "/status");
            copy_string(condition, &mut output, "reason", "/reason");
            Value::Object(output)
        })
        .collect::<Vec<_>>();

    Some(Value::Array(compacted))
}

fn build_pipeline_run_analysis(
    pipeline_run: &Value,
    task_runs: &Value,
    deployment: Value,
    argo_application: Value,
) -> Value {
    let pipeline = analyze_tekton_run(pipeline_run);
    let pipeline_params = extract_pipeline_params(pipeline_run);
    let pipeline_results = collect_named_results(
        task_runs
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten(),
    );
    let task_items = task_runs
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tasks = task_items
        .iter()
        .map(analyze_tekton_run)
        .collect::<Vec<_>>();
    let failed_tasks = tasks
        .iter()
        .filter(|task| task.get("status").and_then(Value::as_str) == Some("failed"))
        .count();
    let succeeded_tasks = tasks
        .iter()
        .filter(|task| task.get("status").and_then(Value::as_str) == Some("succeeded"))
        .count();
    let running_tasks = tasks
        .iter()
        .filter(|task| task.get("status").and_then(Value::as_str) == Some("running"))
        .count();
    let expected_image = pipeline_results
        .get("IMAGE_URL")
        .or_else(|| pipeline_params.get("image-reference"))
        .cloned()
        .unwrap_or(Value::Null);
    let image_alignment = image_alignment(&expected_image, &deployment);

    serde_json::json!({
        "kind": "PipelineRunAnalysis",
        "pipeline_run": pipeline,
        "inputs": {
            "repo_url": pipeline_params.get("repo-url").cloned().unwrap_or(Value::Null),
            "image_reference": pipeline_params.get("image-reference").cloned().unwrap_or(Value::Null),
            "deployment": pipeline_params.get("deployment").cloned().unwrap_or(Value::Null),
            "deployment_namespace": pipeline_params.get("deployment-namespace").cloned().unwrap_or(Value::Null),
        },
        "outputs": {
            "commit": pipeline_results.get("commit").cloned().unwrap_or(Value::Null),
            "repo_url": pipeline_results.get("url").cloned().unwrap_or(Value::Null),
            "image_digest": pipeline_results.get("IMAGE_DIGEST").cloned().unwrap_or(Value::Null),
            "image_url": pipeline_results.get("IMAGE_URL").cloned().unwrap_or(Value::Null),
        },
        "deployment": deployment,
        "argo_application": argo_application.clone(),
        "task_runs": tasks,
        "summary": {
            "status": pipeline.get("status").cloned().unwrap_or_else(|| Value::String("unknown".to_string())),
            "reason": pipeline.get("reason").cloned().unwrap_or(Value::Null),
            "task_run_count": task_items.len(),
            "failed_task_run_count": failed_tasks,
            "succeeded_task_run_count": succeeded_tasks,
            "running_task_run_count": running_tasks,
            "image_alignment": image_alignment,
            "argo_sync_status": argo_application.get("sync_status").cloned().unwrap_or(Value::Null),
            "argo_health_status": argo_application.get("health_status").cloned().unwrap_or(Value::Null),
        }
    })
}

fn analyze_tekton_run(value: &Value) -> Value {
    let condition = tekton_succeeded_condition(value);
    serde_json::json!({
        "kind": value.get("kind").cloned().unwrap_or(Value::Null),
        "name": value.pointer("/metadata/name").cloned().unwrap_or(Value::Null),
        "namespace": value.pointer("/metadata/namespace").cloned().unwrap_or(Value::Null),
        "pipeline": value.pointer("/metadata/labels/tekton.dev~1pipeline").cloned().unwrap_or(Value::Null),
        "pipeline_task": value.pointer("/metadata/labels/tekton.dev~1pipelineTask").cloned().unwrap_or(Value::Null),
        "task": value.pointer("/metadata/labels/tekton.dev~1task").cloned().unwrap_or(Value::Null),
        "component": value.pointer("/metadata/labels/app.kubernetes.io~1component").cloned().unwrap_or(Value::Null),
        "status": tekton_condition_status(condition),
        "reason": condition.and_then(|condition| condition.get("reason")).cloned().unwrap_or(Value::Null),
        "message": condition.and_then(|condition| condition.get("message")).cloned().unwrap_or(Value::Null),
        "start_time": value.pointer("/status/startTime").cloned().unwrap_or(Value::Null),
        "completion_time": value.pointer("/status/completionTime").cloned().unwrap_or(Value::Null),
        "pod_name": value.pointer("/status/podName").cloned().unwrap_or(Value::Null),
        "results": compact_tekton_results(value),
    })
}

fn analyze_deployment(value: &Value) -> Value {
    let desired = value.pointer("/spec/replicas").and_then(Value::as_u64);
    let updated = value
        .pointer("/status/updatedReplicas")
        .and_then(Value::as_u64);
    let available = value
        .pointer("/status/availableReplicas")
        .and_then(Value::as_u64);
    let ready = value
        .pointer("/status/readyReplicas")
        .and_then(Value::as_u64);
    let generation = value
        .pointer("/metadata/generation")
        .and_then(Value::as_u64);
    let observed_generation = value
        .pointer("/status/observedGeneration")
        .and_then(Value::as_u64);

    serde_json::json!({
        "status": deployment_rollout_status(desired, updated, available, ready, generation, observed_generation),
        "kind": value.get("kind").cloned().unwrap_or(Value::Null),
        "name": value.pointer("/metadata/name").cloned().unwrap_or(Value::Null),
        "namespace": value.pointer("/metadata/namespace").cloned().unwrap_or(Value::Null),
        "argo_tracking_id": value.pointer("/metadata/annotations/argocd.argoproj.io~1tracking-id").cloned().unwrap_or(Value::Null),
        "argo_application": argo_application_from_tracking_id(value),
        "revision": value.pointer("/metadata/annotations/deployment.kubernetes.io~1revision").cloned().unwrap_or(Value::Null),
        "restarted_at": value.pointer("/spec/template/metadata/annotations/kubectl.kubernetes.io~1restartedAt").cloned().unwrap_or(Value::Null),
        "replicas": {
            "desired": desired,
            "updated": updated,
            "available": available,
            "ready": ready,
        },
        "generation": generation,
        "observed_generation": observed_generation,
        "containers": deployment_containers(value),
        "conditions": compact_conditions(value).unwrap_or(Value::Array(Vec::new())),
    })
}

fn analyze_argo_application(value: &Value) -> Value {
    serde_json::json!({
        "status": "ok",
        "kind": value.get("kind").cloned().unwrap_or(Value::Null),
        "name": value.pointer("/metadata/name").cloned().unwrap_or(Value::Null),
        "namespace": value.pointer("/metadata/namespace").cloned().unwrap_or(Value::Null),
        "health_status": value.pointer("/status/health/status").cloned().unwrap_or(Value::Null),
        "sync_status": value.pointer("/status/sync/status").cloned().unwrap_or(Value::Null),
        "revision": value.pointer("/status/sync/revision").cloned().unwrap_or(Value::Null),
        "reconciled_at": value.pointer("/status/reconciledAt").cloned().unwrap_or(Value::Null),
        "conditions": compact_conditions(value).unwrap_or(Value::Array(Vec::new())),
    })
}

fn deployment_rollout_status(
    desired: Option<u64>,
    updated: Option<u64>,
    available: Option<u64>,
    ready: Option<u64>,
    generation: Option<u64>,
    observed_generation: Option<u64>,
) -> &'static str {
    let desired = desired.unwrap_or(1);
    if generation
        .zip(observed_generation)
        .is_some_and(|(generation, observed)| observed < generation)
    {
        return "progressing";
    }
    if updated.unwrap_or(0) < desired
        || available.unwrap_or(0) < desired
        || ready.unwrap_or(0) < desired
    {
        return "progressing";
    }

    "healthy"
}

fn argo_application_from_tracking_id(value: &Value) -> Value {
    value
        .pointer("/metadata/annotations/argocd.argoproj.io~1tracking-id")
        .and_then(Value::as_str)
        .and_then(|tracking_id| tracking_id.split(':').next())
        .filter(|app| !app.is_empty())
        .map(Value::from)
        .unwrap_or(Value::Null)
}

fn deployment_containers(value: &Value) -> Value {
    Value::Array(
        value
            .pointer("/spec/template/spec/containers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|container| {
                serde_json::json!({
                    "name": container.get("name").cloned().unwrap_or(Value::Null),
                    "image": container.get("image").cloned().unwrap_or(Value::Null),
                })
            })
            .collect(),
    )
}

fn image_alignment(expected_image: &Value, deployment: &Value) -> Value {
    let deployment_images = deployment
        .get("containers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|container| container.get("image").and_then(Value::as_str))
        .map(Value::from)
        .collect::<Vec<_>>();

    let Some(expected_image) = expected_image.as_str().filter(|value| !value.is_empty()) else {
        return serde_json::json!({
            "status": "unknown",
            "expected_image": Value::Null,
            "deployment_images": deployment_images,
        });
    };
    let exact_match = deployment_images
        .iter()
        .any(|image| image.as_str() == Some(expected_image));

    serde_json::json!({
        "status": if exact_match { "exact_match" } else { "mismatch" },
        "expected_image": expected_image,
        "deployment_images": deployment_images,
    })
}

fn extract_pipeline_params(pipeline_run: &Value) -> Map<String, Value> {
    pipeline_run
        .pointer("/spec/params")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(named_value_pair)
        .collect()
}

fn collect_named_results<'a>(runs: impl Iterator<Item = &'a Value>) -> Map<String, Value> {
    runs.flat_map(|run| {
        run.pointer("/status/results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(named_value_pair)
    })
    .collect()
}

fn compact_tekton_results(value: &Value) -> Value {
    Value::Object(
        value
            .pointer("/status/results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(named_value_pair)
            .collect(),
    )
}

fn named_value_pair(value: &Value) -> Option<(String, Value)> {
    let name = value.get("name")?.as_str()?.to_string();
    let raw_value = value.get("value").cloned().unwrap_or(Value::Null);
    let safe_value = if looks_secretish(&name) {
        Value::String("[redacted]".to_string())
    } else {
        redact_named_value(raw_value)
    };

    Some((name, safe_value))
}

fn redact_named_value(mut value: Value) -> Value {
    redact_json(&mut value);
    value
}

fn tekton_succeeded_condition(value: &Value) -> Option<&Value> {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)?
        .iter()
        .find(|condition| condition.get("type").and_then(Value::as_str) == Some("Succeeded"))
}

fn tekton_condition_status(condition: Option<&Value>) -> &'static str {
    match condition.and_then(|condition| condition.get("status").and_then(Value::as_str)) {
        Some("True") => "succeeded",
        Some("False") => "failed",
        Some("Unknown") => "running",
        _ => "unknown",
    }
}

fn pod_ready(value: &Value) -> Option<bool> {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)?
        .iter()
        .find(|condition| condition.get("type").and_then(Value::as_str) == Some("Ready"))
        .map(|condition| condition.get("status").and_then(Value::as_str) == Some("True"))
}

fn copy_string(source: &Value, target: &mut Map<String, Value>, key: &str, pointer: &str) {
    if let Some(value) = source.pointer(pointer).and_then(Value::as_str) {
        target.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn copy_bool(source: &Value, target: &mut Map<String, Value>, key: &str, pointer: &str) {
    if let Some(value) = source.pointer(pointer).and_then(Value::as_bool) {
        target.insert(key.to_string(), Value::Bool(value));
    }
}

fn copy_u64(source: &Value, target: &mut Map<String, Value>, key: &str, pointer: &str) {
    if let Some(value) = source.pointer(pointer).and_then(Value::as_u64) {
        target.insert(key.to_string(), Value::from(value));
    }
}

fn non_empty_object(object: Map<String, Value>) -> Option<Value> {
    if object.is_empty() {
        None
    } else {
        Some(Value::Object(object))
    }
}

fn compact_prometheus_response(value: &Value) -> Value {
    let result = value
        .pointer("/data/result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let samples = result
        .iter()
        .take(MAX_PROMETHEUS_RESULTS)
        .map(compact_prometheus_result)
        .collect::<Vec<_>>();

    serde_json::json!({
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "data": {
            "resultType": value.pointer("/data/resultType").cloned().unwrap_or(Value::Null),
            "result_count": result.len(),
            "results_truncated": result.len() > MAX_PROMETHEUS_RESULTS,
            "results": samples,
        },
        "warnings": value.get("warnings").cloned().unwrap_or(Value::Null),
        "infos": value.get("infos").cloned().unwrap_or(Value::Null),
    })
}

fn compact_prometheus_result(value: &Value) -> Value {
    let mut result = Map::new();
    if let Some(metric) = value.get("metric") {
        result.insert("metric".to_string(), metric.clone());
    }
    if let Some(value) = value.get("value") {
        result.insert("value".to_string(), value.clone());
    }
    if let Some(values) = value.get("values") {
        result.insert("values".to_string(), values.clone());
    }

    Value::Object(result)
}

fn redact_json(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if looks_secretish(key) {
                    *value = Value::String("[redacted]".to_string());
                } else {
                    redact_json(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json(value);
            }
        }
        Value::String(value) if looks_secretish(value) => {
            *value = "[redacted]".to_string();
        }
        _ => {}
    }
}

fn redact_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if looks_secretish(line) {
                "[redacted]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reject_secretish(label: &str, value: &str) -> Result<(), ToolError> {
    if looks_secretish(value) {
        return Err(ToolError::InvalidArguments {
            message: format!("{label} appears to request secret data"),
        });
    }
    Ok(())
}

fn looks_secretish(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "secret",
        "token",
        "password",
        "credential",
        "kubeconfig",
        "private_key",
        "authorization",
    ]
    .into_iter()
    .any(|needle| value.contains(needle))
}

fn validate_kubernetes_name(label: &str, value: &str, allow_slash: bool) -> Result<(), ToolError> {
    if value.is_empty()
        || value.chars().any(|ch| {
            !(ch.is_ascii_alphanumeric()
                || ch == '-'
                || ch == '.'
                || ch == '_'
                || (allow_slash && ch == '/'))
        })
    {
        return Err(ToolError::InvalidArguments {
            message: format!("{label} contains unsupported characters"),
        });
    }

    Ok(())
}

fn validate_label_selector(value: &str) -> Result<(), ToolError> {
    if value.chars().any(|ch| {
        !(ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '.' | '_' | '/' | '=' | '!' | ',' | '(' | ')'))
    }) {
        return Err(ToolError::InvalidArguments {
            message: "label_selector contains unsupported characters".to_string(),
        });
    }

    Ok(())
}

fn truncate(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    (format!("{}...[truncated]", &value[..end]), true)
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_argo_application, analyze_deployment, build_pipeline_run_analysis, command_summary,
        compact_kubernetes_output, compact_prometheus_response, redact_json, redact_text,
        validate_kubernetes_name, ReadOnlyClusterTools,
    };
    use crate::{AgentAction, ToolError, ToolExecutor};

    #[test]
    fn redacts_secret_shaped_json_fields() {
        let mut redacted =
            serde_json::json!({"metadata":{"name":"app"},"data":{"token":"abc","safe":"ok"}});
        redact_json(&mut redacted);

        assert_eq!(redacted["data"]["token"], "[redacted]");
        assert_eq!(redacted["data"]["safe"], "ok");
    }

    #[test]
    fn redacts_secret_shaped_text_lines() {
        let redacted = redact_text("status ok\npassword: abc\nready");
        assert_eq!(redacted, "status ok\n[redacted]\nready");
    }

    #[test]
    fn validates_kubernetes_names_without_shell_parsing() {
        assert!(validate_kubernetes_name("resource", "deployments.apps", true).is_ok());
        assert!(validate_kubernetes_name("resource", "pods --raw", true).is_err());
    }

    #[test]
    fn command_summary_uses_program_name_without_local_path() {
        let command = command_summary("/opt/tools/kubectl", &["get".into(), "pods".into()]);
        assert_eq!(command, "kubectl get pods");
    }

    #[test]
    fn compacts_kubernetes_lists_without_breaking_json_redaction() {
        let output = compact_kubernetes_output(
            r#"{
                "apiVersion": "v1",
                "kind": "List",
                "items": [
                    {
                        "apiVersion": "v1",
                        "kind": "Pod",
                        "metadata": {
                            "name": "app-0",
                            "namespace": "apps",
                            "creationTimestamp": "2026-05-16T00:00:00Z"
                        },
                        "spec": {
                            "volumes": [
                                {
                                    "name": "kube-api-access",
                                    "projected": {
                                        "sources": [
                                            {
                                                "serviceAccountToken": {
                                                    "path": "token"
                                                }
                                            }
                                        ]
                                    }
                                }
                            ]
                        },
                        "status": {
                            "phase": "Running",
                            "conditions": [
                                {"type": "Ready", "status": "True"}
                            ],
                            "containerStatuses": [
                                {
                                    "name": "app",
                                    "ready": true,
                                    "restartCount": 2,
                                    "state": {"running": {}}
                                }
                            ]
                        }
                    }
                ]
            }"#,
        );

        assert_eq!(output["kind"], "List");
        assert_eq!(output["item_count"], 1);
        assert_eq!(output["items"][0]["metadata"]["name"], "app-0");
        assert_eq!(output["items"][0]["status"]["ready"], true);
        assert_eq!(output["items"][0]["status"]["restart_count"], 2);
        assert!(output.to_string().len() < 600);
        assert!(!output.to_string().contains("serviceAccountToken"));
    }

    #[test]
    fn compacts_argocd_application_status() {
        let output = compact_kubernetes_output(
            r#"{
                "apiVersion": "argoproj.io/v1alpha1",
                "kind": "Application",
                "metadata": {
                    "name": "ghost",
                    "namespace": "argocd"
                },
                "status": {
                    "health": {"status": "Healthy"},
                    "sync": {"status": "Synced", "revision": "abc123"},
                    "operationState": {"message": "large details intentionally omitted"}
                }
            }"#,
        );

        assert_eq!(output["kind"], "Application");
        assert_eq!(output["metadata"]["name"], "ghost");
        assert_eq!(output["status"]["health"]["status"], "Healthy");
        assert_eq!(output["status"]["sync"]["status"], "Synced");
        assert!(output["status"].get("operationState").is_none());
    }

    #[test]
    fn compacts_tekton_pipeline_run_status() {
        let output = compact_kubernetes_output(
            r#"{
                "apiVersion": "tekton.dev/v1",
                "kind": "PipelineRun",
                "metadata": {
                    "name": "build-app",
                    "namespace": "ci"
                },
                "spec": {
                    "params": [
                        {"name": "token", "value": "should-not-leak"}
                    ]
                },
                "status": {
                    "startTime": "2026-05-16T00:00:00Z",
                    "completionTime": "2026-05-16T00:02:00Z",
                    "conditions": [
                        {"type": "Succeeded", "status": "True", "reason": "Succeeded"}
                    ],
                    "childReferences": [
                        {"name": "large-details-intentionally-omitted"}
                    ]
                }
            }"#,
        );

        assert_eq!(output["kind"], "PipelineRun");
        assert_eq!(output["metadata"]["name"], "build-app");
        assert_eq!(output["status"]["startTime"], "2026-05-16T00:00:00Z");
        assert_eq!(output["status"]["conditions"][0]["type"], "Succeeded");
        assert!(output["spec"].is_null());
        assert!(output["status"].get("childReferences").is_none());
        assert!(!output.to_string().contains("should-not-leak"));
    }

    #[test]
    fn compacts_tekton_task_run_status() {
        let output = compact_kubernetes_output(
            r#"{
                "apiVersion": "tekton.dev/v1",
                "kind": "TaskRun",
                "metadata": {
                    "name": "build-app-task",
                    "namespace": "ci"
                },
                "status": {
                    "startTime": "2026-05-16T00:00:00Z",
                    "completionTime": "2026-05-16T00:01:00Z",
                    "conditions": [
                        {"type": "Succeeded", "status": "False", "reason": "Failed"}
                    ],
                    "steps": [
                        {"name": "large-details-intentionally-omitted"}
                    ]
                }
            }"#,
        );

        assert_eq!(output["kind"], "TaskRun");
        assert_eq!(output["metadata"]["name"], "build-app-task");
        assert_eq!(output["status"]["completionTime"], "2026-05-16T00:01:00Z");
        assert_eq!(output["status"]["conditions"][0]["reason"], "Failed");
        assert!(output["status"].get("steps").is_none());
    }

    #[test]
    fn builds_pipeline_run_analysis_from_pipeline_and_task_runs() {
        let pipeline_run = serde_json::json!({
            "apiVersion": "tekton.dev/v1",
            "kind": "PipelineRun",
            "metadata": {
                "name": "build-app",
                "namespace": "ci",
                "labels": {
                    "app.kubernetes.io/component": "app",
                    "tekton.dev/pipeline": "clone-build-push"
                }
            },
            "spec": {
                "params": [
                    {"name": "repo-url", "value": "https://example.com/app.git"},
                    {"name": "image-reference", "value": "registry.local/app:latest"},
                    {"name": "deployment", "value": "app"},
                    {"name": "deployment-namespace", "value": "apps-prod"},
                    {"name": "token-output", "value": "unsafe"}
                ]
            },
            "status": {
                "startTime": "2026-05-16T00:00:00Z",
                "completionTime": "2026-05-16T00:03:00Z",
                "conditions": [
                    {"type": "Succeeded", "status": "False", "reason": "Failed"}
                ]
            }
        });
        let task_runs = serde_json::json!({
            "kind": "List",
            "items": [
                {
                    "kind": "TaskRun",
                    "metadata": {
                        "name": "build-app-test",
                        "namespace": "ci",
                        "labels": {
                            "tekton.dev/pipelineTask": "test",
                            "tekton.dev/task": "unit-test"
                        }
                    },
                    "status": {
                        "conditions": [
                            {"type": "Succeeded", "status": "True", "reason": "Succeeded"}
                        ],
                        "results": [
                            {"name": "commit", "value": "abc123"},
                            {"name": "token-result", "value": "unsafe"}
                        ]
                    }
                },
                {
                    "kind": "TaskRun",
                    "metadata": {
                        "name": "build-app-build",
                        "namespace": "ci",
                        "labels": {
                            "tekton.dev/pipelineTask": "build",
                            "tekton.dev/task": "kaniko"
                        }
                    },
                    "status": {
                        "conditions": [
                            {"type": "Succeeded", "status": "False", "reason": "Failed"}
                        ],
                        "podName": "build-app-build-pod",
                        "results": [
                            {"name": "IMAGE_DIGEST", "value": "sha256:abc"},
                            {"name": "IMAGE_URL", "value": "registry.local/app:latest"}
                        ]
                    }
                }
            ]
        });

        let deployment = serde_json::json!({
            "status": "healthy",
            "name": "app",
            "namespace": "apps-prod",
            "argo_application": "app",
            "containers": [
                {
                    "name": "app",
                    "image": "registry.local/app:latest"
                }
            ],
        });
        let argo_application = serde_json::json!({
            "status": "ok",
            "name": "app",
            "namespace": "argocd",
            "health_status": "Healthy",
            "sync_status": "Synced",
            "revision": "abc123",
        });
        let analysis =
            build_pipeline_run_analysis(&pipeline_run, &task_runs, deployment, argo_application);

        assert_eq!(analysis["kind"], "PipelineRunAnalysis");
        assert_eq!(analysis["pipeline_run"]["status"], "failed");
        assert_eq!(analysis["summary"]["status"], "failed");
        assert_eq!(analysis["summary"]["task_run_count"], 2);
        assert_eq!(analysis["summary"]["failed_task_run_count"], 1);
        assert_eq!(analysis["summary"]["succeeded_task_run_count"], 1);
        assert_eq!(analysis["task_runs"][1]["reason"], "Failed");
        assert_eq!(
            analysis["inputs"]["repo_url"],
            "https://example.com/app.git"
        );
        assert_eq!(
            analysis["inputs"]["image_reference"],
            "registry.local/app:latest"
        );
        assert_eq!(analysis["inputs"]["deployment"], "app");
        assert_eq!(analysis["inputs"]["deployment_namespace"], "apps-prod");
        assert_eq!(analysis["outputs"]["commit"], "abc123");
        assert_eq!(analysis["outputs"]["image_digest"], "sha256:abc");
        assert_eq!(
            analysis["outputs"]["image_url"],
            "registry.local/app:latest"
        );
        assert_eq!(analysis["deployment"]["status"], "healthy");
        assert_eq!(analysis["deployment"]["name"], "app");
        assert_eq!(analysis["argo_application"]["sync_status"], "Synced");
        assert_eq!(analysis["summary"]["argo_sync_status"], "Synced");
        assert_eq!(analysis["summary"]["argo_health_status"], "Healthy");
        assert_eq!(
            analysis["summary"]["image_alignment"]["status"],
            "exact_match"
        );
        assert_eq!(analysis["task_runs"][1]["pipeline_task"], "build");
        assert_eq!(analysis["task_runs"][1]["task"], "kaniko");
        assert_eq!(analysis["task_runs"][1]["pod_name"], "build-app-build-pod");
        assert_eq!(
            analysis["task_runs"][0]["results"]["token-result"],
            "[redacted]"
        );
    }

    #[test]
    fn analyzes_deployment_rollout_status_without_env_or_secrets() {
        let deployment = serde_json::json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {
                "name": "app",
                "namespace": "apps-prod",
                "generation": 7,
                "annotations": {
                    "argocd.argoproj.io/tracking-id": "app:apps/Deployment:apps-prod/app",
                    "deployment.kubernetes.io/revision": "3"
                }
            },
            "spec": {
                "replicas": 2,
                "template": {
                    "metadata": {
                        "annotations": {
                            "kubectl.kubernetes.io/restartedAt": "2026-05-16T23:03:43Z"
                        }
                    },
                    "spec": {
                        "containers": [
                            {
                                "name": "app",
                                "image": "registry.local/app:latest",
                                "env": [
                                    {
                                        "name": "PASSWORD",
                                        "value": "unsafe"
                                    }
                                ]
                            }
                        ]
                    }
                }
            },
            "status": {
                "observedGeneration": 7,
                "replicas": 2,
                "updatedReplicas": 2,
                "availableReplicas": 2,
                "readyReplicas": 2,
                "conditions": [
                    {
                        "type": "Available",
                        "status": "True",
                        "reason": "MinimumReplicasAvailable"
                    }
                ]
            }
        });

        let analysis = analyze_deployment(&deployment);

        assert_eq!(analysis["status"], "healthy");
        assert_eq!(analysis["argo_application"], "app");
        assert_eq!(analysis["replicas"]["desired"], 2);
        assert_eq!(analysis["replicas"]["ready"], 2);
        assert_eq!(
            analysis["containers"][0]["image"],
            "registry.local/app:latest"
        );
        assert!(!analysis.to_string().contains("unsafe"));
    }

    #[test]
    fn analyzes_argo_application_without_operation_details() {
        let application = serde_json::json!({
            "apiVersion": "argoproj.io/v1alpha1",
            "kind": "Application",
            "metadata": {
                "name": "app",
                "namespace": "argocd"
            },
            "status": {
                "health": {"status": "Healthy"},
                "sync": {"status": "Synced", "revision": "abc123"},
                "reconciledAt": "2026-05-16T23:03:43Z",
                "operationState": {"message": "large details intentionally omitted"},
                "conditions": [
                    {
                        "type": "ComparisonError",
                        "status": "False",
                        "reason": "None"
                    }
                ]
            }
        });

        let analysis = analyze_argo_application(&application);

        assert_eq!(analysis["status"], "ok");
        assert_eq!(analysis["name"], "app");
        assert_eq!(analysis["health_status"], "Healthy");
        assert_eq!(analysis["sync_status"], "Synced");
        assert_eq!(analysis["revision"], "abc123");
        assert_eq!(analysis["conditions"][0]["type"], "ComparisonError");
        assert!(analysis.get("operationState").is_none());
    }

    #[test]
    fn compacts_prometheus_responses() {
        let output = compact_prometheus_response(&serde_json::json!({
            "status": "success",
            "data": {
                "resultType": "vector",
                "result": [
                    {
                        "metric": {"__name__": "up", "job": "one"},
                        "value": [1234.0, "1"]
                    },
                    {
                        "metric": {"__name__": "up", "job": "two"},
                        "value": [1234.0, "0"]
                    }
                ]
            }
        }));

        assert_eq!(output["status"], "success");
        assert_eq!(output["data"]["resultType"], "vector");
        assert_eq!(output["data"]["result_count"], 2);
        assert_eq!(output["data"]["results_truncated"], false);
        assert_eq!(output["data"]["results"][0]["metric"]["job"], "one");
    }

    #[tokio::test]
    async fn rejects_prometheus_query_when_url_is_missing() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::PrometheusQuery {
                id: "act_prom".into(),
                reason: "query".to_string(),
                query: "up".to_string(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn rejects_secret_shaped_kubernetes_request_before_command() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::KubernetesGet {
                id: "act_secret".into(),
                reason: "read secret".to_string(),
                resource: "secrets".to_string(),
                namespace: None,
                name: None,
                all_namespaces: false,
                label_selector: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn rejects_secret_shaped_tekton_request_before_command() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::TektonGetPipelineRuns {
                id: "act_secret".into(),
                reason: "read secret-shaped Tekton namespace".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn rejects_secret_shaped_tekton_task_request_before_command() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::TektonGetTaskRuns {
                id: "act_secret".into(),
                reason: "read secret-shaped Tekton task namespace".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn rejects_secret_shaped_tekton_analysis_before_command() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::TektonAnalyzePipelineRun {
                id: "act_secret".into(),
                reason: "analyze secret-shaped Tekton run".to_string(),
                namespace: "ci".to_string(),
                name: "token-build".to_string(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }
}
