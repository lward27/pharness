#![forbid(unsafe_code)]

mod app;
mod dispatch;
mod dto;
mod worker;

use anyhow::Context;
use pharness_config::ApiRuntimeConfig;
use pharness_store::SqliteStore;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;

    let config = ApiRuntimeConfig::load_from_env()?;
    let bind = config.api.bind;
    let db_path = config.storage.path.clone();
    let cluster_tools = config.cluster_tools();
    let policy = config.policy.clone();

    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let store = Arc::new(
        SqliteStore::connect(&db_path)
            .await
            .with_context(|| format!("failed to open {}", db_path.display()))?,
    );
    let dispatcher = match config.worker.mode {
        pharness_config::WorkerMode::Local => {
            let worker = worker::LocalWorker::from_options(
                store.clone(),
                worker::LocalWorkerOptions {
                    api_key: config.model.api_key.clone(),
                    model: config.model.model.clone(),
                    base_url: config.model.base_url.clone(),
                    cluster_tools: cluster_tools.clone(),
                    default_policy: policy.clone(),
                },
            )
            .context("failed to configure local worker")?;
            match worker {
                Some(worker) => dispatch::RunDispatcher::Local(Box::new(worker)),
                None => dispatch::RunDispatcher::Disabled,
            }
        }
        pharness_config::WorkerMode::KubernetesJob => {
            let worker_env = worker_job_env(&config);
            dispatch::RunDispatcher::Kubernetes(dispatch::KubernetesJobDispatcher::new(
                store.clone(),
                config.cluster.kubectl_bin.clone(),
                config.worker.kubernetes.clone(),
                config.model.model.clone(),
                config.model.base_url.clone(),
                worker_env,
            ))
        }
    };
    tracing::info!(mode = dispatcher.mode(), "run dispatcher configured");
    let worker_token = std::env::var("PHARNESS_WORKER_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if worker_token.is_some() {
        tracing::info!("worker ingest routes enabled");
    }
    let operator_tokens =
        parse_operator_tokens(std::env::var("PHARNESS_OPERATOR_TOKENS").ok().as_deref())?;
    if !operator_tokens.is_empty() {
        tracing::info!(
            operators = operator_tokens.len(),
            "operator token auth enabled"
        );
    }
    let app = app::router(
        store,
        dispatcher,
        cluster_tools,
        policy,
        worker_token,
        operator_tokens,
    );
    tracing::info!(%bind, "starting pharness-api");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "pharness-api listening");

    axum::serve(listener, app).await?;
    Ok(())
}

/// Environment forwarded from the API's effective config into worker Jobs so
/// attempts see the same provider, cluster-tool, and policy settings.
fn worker_job_env(config: &ApiRuntimeConfig) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "PHARNESS_FIREWORKS_MODEL".to_string(),
            config.model.model.clone(),
        ),
        (
            "PHARNESS_FIREWORKS_BASE_URL".to_string(),
            config.model.base_url.clone(),
        ),
        (
            "PHARNESS_ARGOCD_NAMESPACE".to_string(),
            config.cluster.argocd_namespace.clone(),
        ),
        (
            "PHARNESS_CLUSTER_TOOL_TIMEOUT_MS".to_string(),
            config.cluster.timeout_ms.to_string(),
        ),
        (
            "PHARNESS_CLUSTER_TOOL_MAX_OUTPUT_BYTES".to_string(),
            config.cluster.max_output_bytes.to_string(),
        ),
        (
            "PHARNESS_POLICY_SUBJECT".to_string(),
            config.policy.subject.clone(),
        ),
        (
            "PHARNESS_POLICY_ENVIRONMENT".to_string(),
            config.policy.environment.clone(),
        ),
    ];
    if let Some(url) = &config.cluster.prometheus_url {
        env.push(("PHARNESS_PROMETHEUS_URL".to_string(), url.clone()));
    }
    if let Some(url) = &config.cluster.loki_url {
        env.push(("PHARNESS_LOKI_URL".to_string(), url.clone()));
    }
    if !config.cluster.registry_aliases.is_empty() {
        env.push((
            "PHARNESS_REGISTRY_ALIASES".to_string(),
            config.cluster.registry_aliases.join(","),
        ));
    }

    env
}

/// Parse `PHARNESS_OPERATOR_TOKENS` as comma-separated `name=token` pairs.
fn parse_operator_tokens(raw: Option<&str>) -> anyhow::Result<Vec<(String, String)>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let mut tokens = Vec::new();
    for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let Some((name, token)) = entry.split_once('=') else {
            anyhow::bail!("PHARNESS_OPERATOR_TOKENS entries must be name=token pairs");
        };
        let name = name.trim();
        let token = token.trim();
        if name.is_empty() || token.is_empty() {
            anyhow::bail!("PHARNESS_OPERATOR_TOKENS entries must not have blank names or tokens");
        }
        tokens.push((name.to_string(), token.to_string()));
    }

    Ok(tokens)
}

fn init_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("pharness_api=info,tower_http=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))
}
