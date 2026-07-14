use super::{ToolError, ToolExecutor, ToolResult};
use crate::AgentAction;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, ACCEPT, CONTENT_LENGTH, CONTENT_TYPE};
use reqwest::{Method, StatusCode, Url};
use serde_json::Map;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_PROMETHEUS_RESULTS: usize = 20;
const MAX_PROMETHEUS_INVENTORY_ITEMS: usize = 20;
const DEFAULT_LOKI_SINCE_SECONDS: u64 = 3600;
const MIN_LOKI_SINCE_SECONDS: u64 = 60;
const MAX_LOKI_SINCE_SECONDS: u64 = 86_400;
const DEFAULT_LOKI_LIMIT: u32 = 50;
const MAX_LOKI_LIMIT: u32 = 100;
const MAX_LOKI_STREAMS: usize = 20;
const MAX_LOKI_ENTRIES: usize = 50;
const MAX_LOKI_LINE_BYTES: usize = 512;
const REGISTRY_MANIFEST_ACCEPT: &str = concat!(
    "application/vnd.oci.image.index.v1+json,",
    "application/vnd.oci.image.manifest.v1+json,",
    "application/vnd.docker.distribution.manifest.list.v2+json,",
    "application/vnd.docker.distribution.manifest.v2+json"
);

#[derive(Debug, Clone)]
pub struct ReadOnlyClusterTools {
    kubectl_bin: String,
    argocd_namespace: String,
    prometheus_url: Option<String>,
    loki_url: Option<String>,
    registry_aliases: RegistryAliases,
    include_related_resource_lookups: bool,
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
            loki_url: None,
            registry_aliases: RegistryAliases::default(),
            include_related_resource_lookups: true,
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
            loki_url: std::env::var("PHARNESS_LOKI_URL").ok(),
            registry_aliases: std::env::var("PHARNESS_REGISTRY_ALIASES")
                .map(|value| RegistryAliases::parse(&value))
                .unwrap_or_default(),
            include_related_resource_lookups: true,
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

    pub fn with_prometheus_url_option(mut self, url: Option<String>) -> Self {
        self.prometheus_url = url;
        self
    }

    pub fn with_loki_url_option(mut self, url: Option<String>) -> Self {
        self.loki_url = url;
        self
    }

    pub fn with_kubectl_bin(mut self, bin: impl Into<String>) -> Self {
        self.kubectl_bin = bin.into();
        self
    }

    pub fn with_argocd_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.argocd_namespace = namespace.into();
        self
    }

    pub fn with_registry_aliases(mut self, aliases: impl AsRef<str>) -> Self {
        self.registry_aliases = RegistryAliases::parse(aliases.as_ref());
        self
    }

    /// Restricts Tekton analysis to the PipelineRun and its TaskRuns.
    ///
    /// This is used by the executor, whose service account deliberately lacks
    /// deployment and Argo CD read access.
    pub fn without_related_resource_lookups(mut self) -> Self {
        self.include_related_resource_lookups = false;
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    pub fn kubectl_bin(&self) -> &str {
        &self.kubectl_bin
    }

    pub fn argocd_namespace(&self) -> &str {
        &self.argocd_namespace
    }

    pub fn prometheus_configured(&self) -> bool {
        self.prometheus_url.is_some()
    }

    pub fn loki_configured(&self) -> bool {
        self.loki_url.is_some()
    }

    pub fn registry_alias_count(&self) -> usize {
        self.registry_aliases.len()
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
                "resource": resource,
                "namespace": namespace,
                "name": name,
                "all_namespaces": all_namespaces,
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
                "resource": "applications.argoproj.io",
                "namespace": self.argocd_namespace,
                "name": app,
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
        let deployment_lookup = if self.include_related_resource_lookups {
            self.lookup_pipeline_deployment(&pipeline_run).await
        } else {
            RelatedResourceLookup::skipped("Related deployment lookup is disabled")
        };
        let deployment_command = deployment_lookup
            .command
            .as_ref()
            .map(|args| command_summary(&self.kubectl_bin, args));
        let argo_lookup = if self.include_related_resource_lookups {
            self.lookup_related_argo_application(&deployment_lookup.observation)
                .await
        } else {
            RelatedResourceLookup::skipped("Related Argo CD lookup is disabled")
        };
        let argo_command = argo_lookup
            .command
            .as_ref()
            .map(|args| command_summary(&self.kubectl_bin, args));

        Ok(ToolResult::ok(
            format!("analyzed Tekton PipelineRun {namespace}/{name}"),
            serde_json::json!({
                "source": "tekton",
                "resource": "pipeline_run_analysis",
                "namespace": namespace,
                "name": name,
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
                    &self.registry_aliases,
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
                "namespace": namespace,
                "name": name,
                "all_namespaces": all_namespaces,
                "command": command_summary(&self.kubectl_bin, &args),
                "stdout_truncated": output.stdout_truncated,
                "output": compact_kubernetes_output(&output.stdout),
            }),
        ))
    }

    async fn prometheus_query(&self, query: &str) -> Result<ToolResult, ToolError> {
        reject_secretish("query", query)?;
        let body = self.prometheus_get("query", &[("query", query)]).await?;
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

    async fn prometheus_inventory(&self) -> Result<ToolResult, ToolError> {
        let targets = self.prometheus_get("targets", &[]).await?;
        let rules = self.prometheus_get("rules", &[]).await?;
        let alerts = self.prometheus_get("alerts", &[]).await?;

        Ok(ToolResult::ok(
            "read Prometheus inventory",
            serde_json::json!({
                "source": "prometheus",
                "resource": "inventory",
                "inventory": compact_prometheus_inventory(&targets, &rules, &alerts),
            }),
        ))
    }

    async fn prometheus_get(
        &self,
        api_path: &str,
        query: &[(&str, &str)],
    ) -> Result<Value, ToolError> {
        let Some(base_url) = &self.prometheus_url else {
            return Err(ToolError::InvalidArguments {
                message: "PHARNESS_PROMETHEUS_URL is not configured".to_string(),
            });
        };

        let url = format!(
            "{}/api/v1/{}",
            base_url.trim_end_matches('/'),
            api_path.trim_start_matches('/')
        );
        let response = self
            .http
            .get(&url)
            .query(query)
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
        Ok(body)
    }

    async fn loki_log_summary(
        &self,
        query: &str,
        since_seconds: Option<u64>,
        limit: Option<u32>,
    ) -> Result<ToolResult, ToolError> {
        reject_secretish("query", query)?;
        let since_seconds = normalize_loki_since_seconds(since_seconds);
        let limit = normalize_loki_limit(limit);
        let end_ns = now_unix_nanos();
        let start_ns = end_ns.saturating_sub(u128::from(since_seconds) * 1_000_000_000);
        let body = self
            .loki_get(
                "query_range",
                &[
                    ("query", query.to_string()),
                    ("start", start_ns.to_string()),
                    ("end", end_ns.to_string()),
                    ("limit", limit.to_string()),
                    ("direction", "backward".to_string()),
                ],
            )
            .await?;

        Ok(ToolResult::ok(
            "read Loki log summary",
            serde_json::json!({
                "source": "loki",
                "resource": "log_summary",
                "query": query,
                "since_seconds": since_seconds,
                "limit": limit,
                "response": compact_loki_response(&body),
            }),
        ))
    }

    async fn registry_inspect_image(
        &self,
        image_ref: &str,
        registry_base_url: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        reject_secretish("image_ref", image_ref)?;
        if let Some(registry_base_url) = registry_base_url {
            reject_secretish("registry_base_url", registry_base_url)?;
        }
        let parsed = ImageRef::parse(image_ref).ok_or_else(|| ToolError::InvalidArguments {
            message: "image_ref must be a valid image reference".to_string(),
        })?;
        let registry_base = registry_base_url
            .map(parse_registry_base_url)
            .transpose()?
            .or_else(|| registry_base_url_from_image(&parsed));
        let reference = parsed
            .digest
            .as_deref()
            .or(parsed.tag.as_deref())
            .unwrap_or("latest");
        let probe = if let Some(base) = registry_base {
            Some(
                self.registry_probe_manifest(&base, &parsed.repository, reference)
                    .await?,
            )
        } else {
            None
        };
        let verification_status = registry_verification_status(&parsed, probe.as_ref());
        let summary = registry_inspect_summary(&parsed, probe.as_ref(), verification_status);

        Ok(ToolResult::ok(
            summary,
            serde_json::json!({
                "source": "registry",
                "image": parsed.to_json(),
                "requested_image_ref": image_ref,
                "reference": reference,
                "registry_base_url": probe.as_ref().map(|probe| probe.registry_base_url.as_str()),
                "verification_status": verification_status,
                "probe": probe.as_ref().map(RegistryProbe::to_json),
            }),
        ))
    }

    async fn registry_probe_manifest(
        &self,
        registry_base_url: &Url,
        repository: &str,
        reference: &str,
    ) -> Result<RegistryProbe, ToolError> {
        let manifest_url = registry_manifest_url(registry_base_url, repository, reference)?;
        let head_response = self
            .registry_manifest_request(Method::HEAD, manifest_url.clone())
            .await?;
        let response = if head_response.status == StatusCode::METHOD_NOT_ALLOWED.as_u16() {
            self.registry_manifest_request(Method::GET, manifest_url)
                .await?
        } else {
            head_response
        };

        Ok(response)
    }

    async fn registry_manifest_request(
        &self,
        method: Method,
        url: Url,
    ) -> Result<RegistryProbe, ToolError> {
        let registry_base_url = registry_origin(&url);
        let response = self
            .http
            .request(method.clone(), url.clone())
            .header(ACCEPT, REGISTRY_MANIFEST_ACCEPT)
            .send()
            .await
            .map_err(|error| ToolError::Network {
                message: format!("registry manifest request failed: {error}"),
            })?;
        let status = response.status();
        let headers = response.headers().clone();

        Ok(RegistryProbe {
            registry_base_url,
            manifest_url: sanitized_manifest_url(&url),
            method: method.as_str().to_string(),
            status: status.as_u16(),
            accessible: status.is_success(),
            digest: header_str(&headers, "docker-content-digest"),
            content_type: header_str(&headers, CONTENT_TYPE.as_str()),
            content_length: header_str(&headers, CONTENT_LENGTH.as_str()),
        })
    }

    async fn loki_get(&self, api_path: &str, query: &[(&str, String)]) -> Result<Value, ToolError> {
        let Some(base_url) = &self.loki_url else {
            return Err(ToolError::InvalidArguments {
                message: "PHARNESS_LOKI_URL is not configured".to_string(),
            });
        };

        let url = format!(
            "{}/loki/api/v1/{}",
            base_url.trim_end_matches('/'),
            api_path.trim_start_matches('/')
        );
        let response = self
            .http
            .get(&url)
            .query(query)
            .send()
            .await
            .map_err(|error| ToolError::Network {
                message: error.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Network {
                message: format!("Loki returned HTTP {status}: {}", truncate(&body, 4096).0),
            });
        }

        let mut body: Value = response.json().await.map_err(|error| ToolError::Network {
            message: error.to_string(),
        })?;
        redact_json(&mut body);
        Ok(body)
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
            AgentAction::PrometheusInventory { .. } => self.prometheus_inventory().await,
            AgentAction::LokiLogSummary {
                query,
                since_seconds,
                limit,
                ..
            } => self.loki_log_summary(query, *since_seconds, *limit).await,
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
            AgentAction::RegistryInspectImage {
                image_ref,
                registry_base_url,
                ..
            } => {
                self.registry_inspect_image(image_ref, registry_base_url.as_deref())
                    .await
            }
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RegistryAliases {
    pairs: Vec<(String, String)>,
}

impl RegistryAliases {
    fn parse(value: &str) -> Self {
        let pairs = value
            .split(',')
            .filter_map(|entry| {
                let (left, right) = entry.split_once('=')?;
                let left = normalize_registry(left)?;
                let right = normalize_registry(right)?;
                Some((left, right))
            })
            .collect();

        Self { pairs }
    }

    fn equivalent(&self, left: Option<&str>, right: Option<&str>) -> bool {
        if left == right {
            return true;
        }

        let Some(left) = left.and_then(normalize_registry) else {
            return false;
        };
        let Some(right) = right.and_then(normalize_registry) else {
            return false;
        };

        self.pairs
            .iter()
            .any(|(a, b)| (a == &left && b == &right) || (a == &right && b == &left))
    }

    fn len(&self) -> usize {
        self.pairs.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageRef {
    registry: Option<String>,
    repository: String,
    tag: Option<String>,
    digest: Option<String>,
}

impl ImageRef {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }

        let (without_digest, digest) = value
            .split_once('@')
            .map_or((value, None), |(image, digest)| (image, Some(digest)));
        let last_slash = without_digest.rfind('/');
        let tag_split = without_digest
            .rfind(':')
            .filter(|colon| last_slash.map_or(true, |slash| *colon > slash));
        let (without_tag, tag) = tag_split.map_or((without_digest, None), |colon| {
            (&without_digest[..colon], Some(&without_digest[colon + 1..]))
        });

        let (registry, repository) =
            without_tag
                .split_once('/')
                .map_or((None, without_tag), |(first, rest)| {
                    if looks_like_registry(first) {
                        (normalize_registry(first), rest)
                    } else {
                        (None, without_tag)
                    }
                });

        if repository.is_empty() {
            return None;
        }

        Some(Self {
            registry,
            repository: repository.to_string(),
            tag: tag.filter(|value| !value.is_empty()).map(str::to_string),
            digest: digest.filter(|value| !value.is_empty()).map(str::to_string),
        })
    }

    fn identity_matches(&self, other: &Self) -> bool {
        self.repository == other.repository && self.version_identity() == other.version_identity()
    }

    fn version_identity(&self) -> ImageVersion<'_> {
        if let Some(digest) = &self.digest {
            return ImageVersion::Digest(digest);
        }

        ImageVersion::Tag(self.tag.as_deref().unwrap_or("latest"))
    }

    fn to_json(&self) -> Value {
        serde_json::json!({
            "registry": self.registry,
            "repository": self.repository,
            "tag": self.tag,
            "digest": self.digest,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageVersion<'a> {
    Tag(&'a str),
    Digest(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryProbe {
    registry_base_url: String,
    manifest_url: String,
    method: String,
    status: u16,
    accessible: bool,
    digest: Option<String>,
    content_type: Option<String>,
    content_length: Option<String>,
}

impl RegistryProbe {
    fn to_json(&self) -> Value {
        serde_json::json!({
            "registry_base_url": self.registry_base_url.as_str(),
            "manifest_url": self.manifest_url.as_str(),
            "method": self.method.as_str(),
            "status": self.status,
            "accessible": self.accessible,
            "digest": self.digest.as_deref(),
            "content_type": self.content_type.as_deref(),
            "content_length": self.content_length.as_deref(),
        })
    }
}

fn registry_verification_status(image: &ImageRef, probe: Option<&RegistryProbe>) -> &'static str {
    let Some(probe) = probe else {
        return "unknown";
    };
    if !probe.accessible {
        return "unknown";
    }
    let Some(expected_digest) = image.digest.as_deref() else {
        return "verified";
    };
    match probe.digest.as_deref() {
        Some(actual) if actual == expected_digest => "verified",
        Some(_) => "mismatch",
        None => "unknown",
    }
}

fn registry_inspect_summary(
    image: &ImageRef,
    probe: Option<&RegistryProbe>,
    verification_status: &str,
) -> String {
    match probe {
        Some(probe) if probe.accessible => {
            format!(
                "registry manifest reachable for {} with verification status {}",
                image.repository, verification_status
            )
        }
        Some(probe) => format!(
            "registry manifest probe returned HTTP {} for {}",
            probe.status, image.repository
        ),
        None => format!(
            "parsed image {} but no registry base URL was available",
            image.repository
        ),
    }
}

fn parse_registry_base_url(value: &str) -> Result<Url, ToolError> {
    let url = Url::parse(value).map_err(|error| ToolError::InvalidArguments {
        message: format!("registry_base_url must be a valid URL: {error}"),
    })?;
    validate_registry_base_url(&url)?;
    Ok(url)
}

fn registry_base_url_from_image(image: &ImageRef) -> Option<Url> {
    let registry = image.registry.as_deref()?;
    let scheme = if registry == "localhost" || registry.starts_with("localhost:") {
        "http"
    } else {
        "https"
    };
    let url = Url::parse(&format!("{scheme}://{registry}")).ok()?;
    validate_registry_base_url(&url).ok()?;
    Some(url)
}

fn validate_registry_base_url(url: &Url) -> Result<(), ToolError> {
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(ToolError::InvalidArguments {
            message: "registry_base_url must use http or https".to_string(),
        });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ToolError::InvalidArguments {
            message: "registry_base_url must not include credentials".to_string(),
        });
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ToolError::InvalidArguments {
            message: "registry_base_url must not include query or fragment".to_string(),
        });
    }
    Ok(())
}

fn registry_manifest_url(
    registry_base_url: &Url,
    repository: &str,
    reference: &str,
) -> Result<Url, ToolError> {
    let mut url = registry_base_url.clone();
    let base_path = url.path().trim_end_matches('/');
    let manifest_path = format!("{base_path}/v2/{repository}/manifests/{reference}");
    url.set_path(&manifest_path);
    Ok(url)
}

fn registry_origin(url: &Url) -> String {
    let Some(host) = url.host_str() else {
        return url.as_str().trim_end_matches('/').to_string();
    };
    match url.port() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    }
}

fn sanitized_manifest_url(url: &Url) -> String {
    let mut url = url.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn header_str(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn normalize_registry(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn looks_like_registry(value: &str) -> bool {
    value == "localhost" || value.contains('.') || value.contains(':')
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
    registry_aliases: &RegistryAliases,
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
    let image_alignment = image_alignment(&expected_image, &deployment, registry_aliases);

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

fn image_alignment(
    expected_image: &Value,
    deployment: &Value,
    registry_aliases: &RegistryAliases,
) -> Value {
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

    if let Some(matched_image) = deployment_images
        .iter()
        .filter_map(Value::as_str)
        .find(|image| *image == expected_image)
    {
        return image_alignment_result(
            "exact_match",
            expected_image,
            &deployment_images,
            Some(matched_image),
            None,
            None,
            None,
        );
    }

    let Some(expected_ref) = ImageRef::parse(expected_image) else {
        return image_alignment_result(
            "unknown",
            expected_image,
            &deployment_images,
            None,
            None,
            None,
            Some("expected image could not be parsed"),
        );
    };

    for deployment_image in deployment_images.iter().filter_map(Value::as_str) {
        let Some(deployment_ref) = ImageRef::parse(deployment_image) else {
            continue;
        };
        if !expected_ref.identity_matches(&deployment_ref) {
            continue;
        }
        if registry_aliases.equivalent(
            expected_ref.registry.as_deref(),
            deployment_ref.registry.as_deref(),
        ) {
            return image_alignment_result(
                "registry_alias_match",
                expected_image,
                &deployment_images,
                Some(deployment_image),
                Some(&expected_ref),
                Some(&deployment_ref),
                None,
            );
        }

        return image_alignment_result(
            "registry_mismatch",
            expected_image,
            &deployment_images,
            Some(deployment_image),
            Some(&expected_ref),
            Some(&deployment_ref),
            Some(
                "image repository and version match, but registry is not configured as equivalent",
            ),
        );
    }

    image_alignment_result(
        "mismatch",
        expected_image,
        &deployment_images,
        None,
        Some(&expected_ref),
        None,
        None,
    )
}

fn image_alignment_result(
    status: &str,
    expected_image: &str,
    deployment_images: &[Value],
    matched_deployment_image: Option<&str>,
    expected_ref: Option<&ImageRef>,
    deployment_ref: Option<&ImageRef>,
    reason: Option<&str>,
) -> Value {
    serde_json::json!({
        "status": status,
        "expected_image": expected_image,
        "deployment_images": deployment_images,
        "matched_deployment_image": matched_deployment_image,
        "expected_ref": expected_ref.map(ImageRef::to_json),
        "matched_deployment_ref": deployment_ref.map(ImageRef::to_json),
        "reason": reason,
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

fn compact_prometheus_inventory(targets: &Value, rules: &Value, alerts: &Value) -> Value {
    serde_json::json!({
        "targets": compact_prometheus_targets(targets),
        "rules": compact_prometheus_rules(rules),
        "alerts": compact_prometheus_alerts(alerts),
    })
}

fn compact_prometheus_targets(value: &Value) -> Value {
    let active = value
        .pointer("/data/activeTargets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let dropped_count = value
        .pointer("/data/droppedTargets")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let unhealthy = active
        .iter()
        .filter(|target| target.get("health").and_then(Value::as_str) != Some("up"))
        .collect::<Vec<_>>();

    serde_json::json!({
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "active_count": active.len(),
        "dropped_count": dropped_count,
        "health": count_string_field(active.iter(), "health"),
        "unhealthy_count": unhealthy.len(),
        "unhealthy_truncated": unhealthy.len() > MAX_PROMETHEUS_INVENTORY_ITEMS,
        "unhealthy_targets": unhealthy
            .into_iter()
            .take(MAX_PROMETHEUS_INVENTORY_ITEMS)
            .map(compact_prometheus_target)
            .collect::<Vec<_>>(),
    })
}

fn compact_prometheus_target(value: &Value) -> Value {
    let mut target = Map::new();
    copy_non_empty_string(value, &mut target, "scrape_pool", "/scrapePool");
    copy_non_empty_string(value, &mut target, "scrape_url", "/scrapeUrl");
    copy_non_empty_string(value, &mut target, "health", "/health");
    copy_non_empty_string(value, &mut target, "last_scrape", "/lastScrape");
    copy_non_empty_string(value, &mut target, "last_error", "/lastError");
    if let Some(labels) = safe_prometheus_labels(value.get("labels")) {
        target.insert("labels".to_string(), labels);
    }

    Value::Object(target)
}

fn compact_prometheus_rules(value: &Value) -> Value {
    let groups = value
        .pointer("/data/groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rule_count = 0usize;
    let mut alerting_count = 0usize;
    let mut recording_count = 0usize;
    let mut active_alert_count = 0usize;
    let mut problem_rule_count = 0usize;
    let mut health = Map::new();
    let mut problem_rules = Vec::new();

    for group in &groups {
        let group_name = group.get("name").and_then(Value::as_str);
        let file = group.get("file").and_then(Value::as_str);
        let rules = group
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for rule in rules {
            rule_count += 1;
            match rule.get("type").and_then(Value::as_str) {
                Some("alerting") => alerting_count += 1,
                Some("recording") => recording_count += 1,
                _ => {}
            }
            let rule_health = rule
                .get("health")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            increment_count(&mut health, rule_health);
            let rule_alerts = rule
                .get("alerts")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            active_alert_count += rule_alerts;

            let state = rule.get("state").and_then(Value::as_str);
            let is_problem = rule_health != "ok"
                || matches!(state, Some("firing" | "pending"))
                || rule_alerts > 0;
            if is_problem {
                problem_rule_count += 1;
                if problem_rules.len() < MAX_PROMETHEUS_INVENTORY_ITEMS {
                    problem_rules.push(compact_prometheus_rule(
                        &rule,
                        group_name,
                        file,
                        rule_alerts,
                    ));
                }
            }
        }
    }

    serde_json::json!({
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "group_count": groups.len(),
        "rule_count": rule_count,
        "alerting_count": alerting_count,
        "recording_count": recording_count,
        "active_alert_count": active_alert_count,
        "health": Value::Object(health),
        "problem_rule_count": problem_rule_count,
        "problem_rules_truncated": problem_rule_count > problem_rules.len(),
        "problem_rules": problem_rules,
    })
}

fn compact_prometheus_rule(
    rule: &Value,
    group_name: Option<&str>,
    file: Option<&str>,
    active_alert_count: usize,
) -> Value {
    let mut summary = Map::new();
    if let Some(group_name) = group_name {
        summary.insert("group".to_string(), Value::String(group_name.to_string()));
    }
    if let Some(file) = file {
        summary.insert("file".to_string(), Value::String(file.to_string()));
    }
    copy_non_empty_string(rule, &mut summary, "name", "/name");
    copy_non_empty_string(rule, &mut summary, "type", "/type");
    copy_non_empty_string(rule, &mut summary, "state", "/state");
    copy_non_empty_string(rule, &mut summary, "health", "/health");
    copy_non_empty_string(rule, &mut summary, "last_evaluation", "/lastEvaluation");
    if let Some(evaluation_time) = rule.get("evaluationTime").and_then(Value::as_f64) {
        summary.insert(
            "evaluation_time_seconds".to_string(),
            Value::from(evaluation_time),
        );
    }
    summary.insert(
        "active_alert_count".to_string(),
        Value::from(active_alert_count),
    );

    Value::Object(summary)
}

fn compact_prometheus_alerts(value: &Value) -> Value {
    let alerts = value
        .pointer("/data/alerts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    serde_json::json!({
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "alert_count": alerts.len(),
        "states": count_string_field(alerts.iter(), "state"),
        "alerts_truncated": alerts.len() > MAX_PROMETHEUS_INVENTORY_ITEMS,
        "alerts": alerts
            .iter()
            .take(MAX_PROMETHEUS_INVENTORY_ITEMS)
            .map(compact_prometheus_alert)
            .collect::<Vec<_>>(),
    })
}

fn compact_prometheus_alert(value: &Value) -> Value {
    let mut alert = Map::new();
    copy_non_empty_string(value, &mut alert, "state", "/state");
    copy_non_empty_string(value, &mut alert, "active_at", "/activeAt");
    copy_non_empty_string(value, &mut alert, "value", "/value");
    if let Some(labels) = safe_prometheus_labels(value.get("labels")) {
        alert.insert("labels".to_string(), labels);
    }

    Value::Object(alert)
}

fn count_string_field<'a>(values: impl Iterator<Item = &'a Value>, field: &str) -> Value {
    let mut counts = Map::new();
    for value in values {
        let key = value
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        increment_count(&mut counts, key);
    }

    Value::Object(counts)
}

fn increment_count(counts: &mut Map<String, Value>, key: &str) {
    let next = counts.get(key).and_then(Value::as_u64).unwrap_or_default() + 1;
    counts.insert(key.to_string(), Value::from(next));
}

fn copy_non_empty_string(
    source: &Value,
    target: &mut Map<String, Value>,
    key: &str,
    pointer: &str,
) {
    if let Some(value) = source.pointer(pointer).and_then(Value::as_str) {
        if !value.trim().is_empty() {
            target.insert(key.to_string(), Value::String(truncate(value, 512).0));
        }
    }
}

fn safe_prometheus_labels(labels: Option<&Value>) -> Option<Value> {
    let labels = labels?.as_object()?;
    let mut safe = Map::new();
    for key in [
        "alertname",
        "severity",
        "namespace",
        "pod",
        "container",
        "service",
        "job",
        "instance",
        "endpoint",
        "app",
        "name",
        "deployment",
        "statefulset",
        "daemonset",
        "prometheus",
        "pipelinerun",
        "taskrun",
        "task",
        "pipeline",
        "app_kubernetes_io_name",
    ] {
        if let Some(value) = labels.get(key).and_then(Value::as_str) {
            safe.insert(key.to_string(), Value::String(truncate(value, 256).0));
        }
    }

    non_empty_object(safe)
}

fn compact_loki_response(value: &Value) -> Value {
    let streams = value
        .pointer("/data/result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total_entries = streams
        .iter()
        .map(|stream| {
            stream
                .get("values")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default()
        })
        .sum::<usize>();
    let mut remaining_entries = MAX_LOKI_ENTRIES;
    let compact_streams = streams
        .iter()
        .take(MAX_LOKI_STREAMS)
        .filter_map(|stream| {
            if remaining_entries == 0 {
                return None;
            }
            let compact = compact_loki_stream(stream, &mut remaining_entries);
            Some(compact)
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "data": {
            "resultType": value.pointer("/data/resultType").cloned().unwrap_or(Value::Null),
            "stream_count": streams.len(),
            "streams_truncated": streams.len() > MAX_LOKI_STREAMS,
            "entry_count": total_entries,
            "entries_truncated": total_entries > MAX_LOKI_ENTRIES,
            "streams": compact_streams,
        },
        "warnings": value.get("warnings").cloned().unwrap_or(Value::Null),
    })
}

fn compact_loki_stream(value: &Value, remaining_entries: &mut usize) -> Value {
    let mut stream = Map::new();
    if let Some(labels) = safe_prometheus_labels(value.get("stream")) {
        stream.insert("labels".to_string(), labels);
    }
    let entries = value
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(*remaining_entries)
        .filter_map(compact_loki_entry)
        .collect::<Vec<_>>();
    *remaining_entries = remaining_entries.saturating_sub(entries.len());
    stream.insert("entry_count".to_string(), Value::from(entries.len()));
    stream.insert("entries".to_string(), Value::Array(entries));

    Value::Object(stream)
}

fn compact_loki_entry(value: &Value) -> Option<Value> {
    let values = value.as_array()?;
    let timestamp = values.first().and_then(Value::as_str)?;
    let line = values.get(1).and_then(Value::as_str).unwrap_or_default();
    let redacted = redact_text(line);
    let (line, line_truncated) = truncate(&redacted, MAX_LOKI_LINE_BYTES);

    Some(serde_json::json!({
        "timestamp": timestamp,
        "line": line,
        "line_truncated": line_truncated,
    }))
}

fn normalize_loki_since_seconds(value: Option<u64>) -> u64 {
    value
        .unwrap_or(DEFAULT_LOKI_SINCE_SECONDS)
        .clamp(MIN_LOKI_SINCE_SECONDS, MAX_LOKI_SINCE_SECONDS)
}

fn normalize_loki_limit(value: Option<u32>) -> u32 {
    value.unwrap_or(DEFAULT_LOKI_LIMIT).clamp(1, MAX_LOKI_LIMIT)
}

fn now_unix_nanos() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
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
        compact_kubernetes_output, compact_loki_response, compact_prometheus_inventory,
        compact_prometheus_response, image_alignment, normalize_loki_limit,
        normalize_loki_since_seconds, redact_json, redact_text, validate_kubernetes_name, ImageRef,
        ReadOnlyClusterTools, RegistryAliases, DEFAULT_LOKI_LIMIT, DEFAULT_LOKI_SINCE_SECONDS,
        MAX_LOKI_LIMIT, MAX_LOKI_SINCE_SECONDS, MIN_LOKI_SINCE_SECONDS,
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
        let analysis = build_pipeline_run_analysis(
            &pipeline_run,
            &task_runs,
            deployment,
            argo_application,
            &RegistryAliases::default(),
        );

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
    fn parses_registry_alias_configuration() {
        let aliases = RegistryAliases::parse(
            "docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering",
        );

        assert!(aliases.equivalent(
            Some("docker-registry.registry.svc.cluster.local:5000"),
            Some("registry.lucas.engineering")
        ));
        assert!(!aliases.equivalent(Some("example.com"), Some("registry.lucas.engineering")));
    }

    #[test]
    fn parses_image_references_without_losing_repository_identity() {
        let image = ImageRef::parse(
            "docker-registry.registry.svc.cluster.local:5000/team/app:latest@sha256:abc",
        )
        .expect("image should parse");

        assert_eq!(
            image.registry.as_deref(),
            Some("docker-registry.registry.svc.cluster.local:5000")
        );
        assert_eq!(image.repository, "team/app");
        assert_eq!(image.tag.as_deref(), Some("latest"));
        assert_eq!(image.digest.as_deref(), Some("sha256:abc"));
    }

    #[test]
    fn image_alignment_uses_configured_registry_aliases() {
        let deployment = serde_json::json!({
            "containers": [
                {
                    "name": "app",
                    "image": "registry.lucas.engineering/app:latest"
                }
            ]
        });
        let aliases = RegistryAliases::parse(
            "docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering",
        );

        let alignment = image_alignment(
            &serde_json::json!("docker-registry.registry.svc.cluster.local:5000/app:latest"),
            &deployment,
            &aliases,
        );

        assert_eq!(alignment["status"], "registry_alias_match");
        assert_eq!(
            alignment["matched_deployment_image"],
            "registry.lucas.engineering/app:latest"
        );
        assert_eq!(
            alignment["expected_ref"]["repository"],
            alignment["matched_deployment_ref"]["repository"]
        );
    }

    #[test]
    fn image_alignment_preserves_unconfigured_registry_mismatch() {
        let deployment = serde_json::json!({
            "containers": [
                {
                    "name": "app",
                    "image": "registry.lucas.engineering/app:latest"
                }
            ]
        });

        let alignment = image_alignment(
            &serde_json::json!("docker-registry.registry.svc.cluster.local:5000/app:latest"),
            &deployment,
            &RegistryAliases::default(),
        );

        assert_eq!(alignment["status"], "registry_mismatch");
        assert_eq!(
            alignment["reason"],
            "image repository and version match, but registry is not configured as equivalent"
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

    #[test]
    fn compacts_prometheus_inventory_without_annotations_or_queries() {
        let inventory = compact_prometheus_inventory(
            &serde_json::json!({
                "status": "success",
                "data": {
                    "activeTargets": [
                        {
                            "scrapePool": "kubernetes-pods",
                            "scrapeUrl": "http://app:8080/metrics",
                            "health": "up",
                            "labels": {
                                "job": "app",
                                "namespace": "apps-dev",
                                "pod": "app-123",
                                "token": "should-not-appear"
                            }
                        },
                        {
                            "scrapePool": "kubernetes-pods",
                            "scrapeUrl": "http://bad:8080/metrics",
                            "health": "down",
                            "lastError": "connection refused",
                            "labels": {"job": "bad", "namespace": "apps-dev"}
                        }
                    ],
                    "droppedTargets": [{}]
                }
            }),
            &serde_json::json!({
                "status": "success",
                "data": {
                    "groups": [
                        {
                            "name": "apps.rules",
                            "file": "apps.yaml",
                            "rules": [
                                {
                                    "name": "HighErrorRate",
                                    "type": "alerting",
                                    "state": "firing",
                                    "health": "ok",
                                    "query": "rate(http_requests_total[5m])",
                                    "alerts": [{}]
                                }
                            ]
                        }
                    ]
                }
            }),
            &serde_json::json!({
                "status": "success",
                "data": {
                    "alerts": [
                        {
                            "state": "firing",
                            "activeAt": "2026-05-21T00:00:00Z",
                            "labels": {
                                "alertname": "HighErrorRate",
                                "severity": "warning",
                                "namespace": "apps-dev",
                                "password": "should-not-appear"
                            },
                            "annotations": {
                                "summary": "intentionally omitted"
                            }
                        }
                    ]
                }
            }),
        );

        assert_eq!(inventory["targets"]["active_count"], 2);
        assert_eq!(inventory["targets"]["unhealthy_count"], 1);
        assert_eq!(
            inventory["targets"]["unhealthy_targets"][0]["labels"]["job"],
            "bad"
        );
        assert!(inventory["targets"]["unhealthy_targets"][0]["labels"]
            .get("token")
            .is_none());
        assert_eq!(inventory["rules"]["problem_rule_count"], 1);
        assert_eq!(
            inventory["rules"]["problem_rules"][0]["name"],
            "HighErrorRate"
        );
        assert!(inventory["rules"]["problem_rules"][0]
            .get("query")
            .is_none());
        assert_eq!(inventory["alerts"]["alert_count"], 1);
        assert_eq!(
            inventory["alerts"]["alerts"][0]["labels"]["alertname"],
            "HighErrorRate"
        );
        assert!(inventory["alerts"]["alerts"][0]["labels"]
            .get("password")
            .is_none());
        assert!(inventory["alerts"]["alerts"][0]
            .get("annotations")
            .is_none());
    }

    #[test]
    fn compacts_loki_response_with_bounded_redacted_entries() {
        let summary = compact_loki_response(&serde_json::json!({
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [
                    {
                        "stream": {
                            "namespace": "apps-dev",
                            "pod": "api-123",
                            "token": "should-not-appear"
                        },
                        "values": [
                            ["1778880000000000000", "started request"],
                            ["1778880001000000000", "password=abc"]
                        ]
                    }
                ]
            }
        }));

        assert_eq!(summary["status"], "success");
        assert_eq!(summary["data"]["stream_count"], 1);
        assert_eq!(summary["data"]["entry_count"], 2);
        assert_eq!(
            summary["data"]["streams"][0]["labels"]["namespace"],
            "apps-dev"
        );
        assert!(summary["data"]["streams"][0]["labels"]
            .get("token")
            .is_none());
        assert_eq!(
            summary["data"]["streams"][0]["entries"][0]["line"],
            "started request"
        );
        assert_eq!(
            summary["data"]["streams"][0]["entries"][1]["line"],
            "[redacted]"
        );
    }

    #[test]
    fn normalizes_loki_window_and_limit() {
        assert_eq!(
            normalize_loki_since_seconds(None),
            DEFAULT_LOKI_SINCE_SECONDS
        );
        assert_eq!(
            normalize_loki_since_seconds(Some(1)),
            MIN_LOKI_SINCE_SECONDS
        );
        assert_eq!(
            normalize_loki_since_seconds(Some(MAX_LOKI_SINCE_SECONDS + 1)),
            MAX_LOKI_SINCE_SECONDS
        );
        assert_eq!(normalize_loki_limit(None), DEFAULT_LOKI_LIMIT);
        assert_eq!(normalize_loki_limit(Some(0)), 1);
        assert_eq!(
            normalize_loki_limit(Some(MAX_LOKI_LIMIT + 1)),
            MAX_LOKI_LIMIT
        );
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
    async fn rejects_prometheus_inventory_when_url_is_missing() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::PrometheusInventory {
                id: "act_prom_inventory".into(),
                reason: "inventory".to_string(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn rejects_loki_log_summary_when_url_is_missing() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::LokiLogSummary {
                id: "act_loki".into(),
                reason: "logs".to_string(),
                query: r#"{namespace="apps-dev"}"#.to_string(),
                since_seconds: Some(900),
                limit: Some(25),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn inspects_registry_image_identity_without_registry_probe() {
        let tools = ReadOnlyClusterTools::default();
        let result = tools
            .execute(&AgentAction::RegistryInspectImage {
                id: "act_registry".into(),
                reason: "inspect image identity".to_string(),
                image_ref: "team/checkout-api:v1".to_string(),
                registry_base_url: None,
            })
            .await
            .unwrap();

        assert_eq!(result.content["source"], "registry");
        assert_eq!(result.content["image"]["repository"], "team/checkout-api");
        assert_eq!(result.content["image"]["tag"], "v1");
        assert_eq!(result.content["verification_status"], "unknown");
        assert!(result.content["probe"].is_null());
    }

    #[tokio::test]
    async fn rejects_registry_base_urls_with_credentials() {
        let tools = ReadOnlyClusterTools::default();
        let error = tools
            .execute(&AgentAction::RegistryInspectImage {
                id: "act_registry".into(),
                reason: "inspect image identity".to_string(),
                image_ref: "registry.example.test/team/checkout-api:v1".to_string(),
                registry_base_url: Some("https://user:pass@registry.example.test".to_string()),
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
