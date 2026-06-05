#![forbid(unsafe_code)]

mod app;
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
    let worker = worker::LocalWorker::from_options(
        store.clone(),
        worker::LocalWorkerOptions {
            api_key: config.model.api_key,
            model: config.model.model,
            base_url: config.model.base_url,
            cluster_tools: cluster_tools.clone(),
            default_policy: policy.clone(),
        },
    )
    .context("failed to configure local worker")?;
    let app = app::router(store, worker, cluster_tools, policy);
    tracing::info!(%bind, "starting pharness-api");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "pharness-api listening");

    axum::serve(listener, app).await?;
    Ok(())
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
