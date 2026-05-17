#![forbid(unsafe_code)]

mod app;
mod dto;
mod worker;

use anyhow::Context;
use pharness_store::SqliteStore;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;

    let bind: SocketAddr = std::env::var("PHARNESS_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4777".to_string())
        .parse()
        .context("PHARNESS_BIND must be a socket address")?;
    let db_path = std::env::var("PHARNESS_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".pharness/pharness.db"));

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
    let worker =
        worker::LocalWorker::from_env(store.clone()).context("failed to configure local worker")?;
    let app = app::router(store, worker);
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
