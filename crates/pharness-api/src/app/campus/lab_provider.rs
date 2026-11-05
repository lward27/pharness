//! Building the lab runtime and its execution provider at startup.

use super::lab_catalog::{CampusLabProviderKind, CampusLabsConfig};
use super::labs::{CampusLabs, RECONCILER_ACTOR};
use super::state::CampusRuntime;
use anyhow::Context;
use pharness_core::campus_lab_session::kubernetes::{
    KubectlCampusKubeApi, KubernetesCampusProvider,
};
use pharness_core::{CampusExecutionProvider, FakeCampusProvider};
use pharness_store::{JournalCampusAuthoritySource, SqliteStore};
use std::sync::Arc;

/// The lab runtime for `config`, with the execution provider it selects.
pub(crate) fn build_campus_labs(
    runtime: &CampusRuntime,
    store: &SqliteStore,
    config: CampusLabsConfig,
) -> anyhow::Result<CampusLabs> {
    let catalog = Arc::new(config.catalog);
    let provider: Option<Arc<dyn CampusExecutionProvider>> = match config.provider {
        CampusLabProviderKind::None => None,
        CampusLabProviderKind::Fake => {
            tracing::warn!("campus labs use the in-memory fake provider; nothing real is launched");
            Some(Arc::new(FakeCampusProvider::new()))
        }
        CampusLabProviderKind::Kubernetes => {
            let file = config
                .kubernetes
                .context("the Kubernetes lab provider has no configuration")?;
            let clock = runtime.broker_clock();
            let authority = Arc::new(JournalCampusAuthoritySource::new(
                store.clone(),
                catalog.clone(),
                clock.clone(),
            ));
            let api = Arc::new(KubectlCampusKubeApi::new(file.kubectl, file.global_args));
            let slots = file.provider.slots.len();
            let provider = KubernetesCampusProvider::new(
                file.provider,
                catalog.profiles.iter().cloned(),
                authority,
                runtime.broker_token_issuer(store, RECONCILER_ACTOR),
                api,
                clock,
            )?;
            tracing::info!(slots, "campus labs launch on the Kubernetes provider");
            Some(Arc::new(provider))
        }
    };
    Ok(CampusLabs::with_shared_catalog(catalog, provider))
}
