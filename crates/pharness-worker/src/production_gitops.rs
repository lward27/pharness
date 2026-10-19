//! Production GitOps phases. `preview` reads the production Kustomization at the
//! current head with the reader identity so a human can approve the exact diff;
//! `deliver` performs the one admitted write after that decision; `observe`
//! recovers a lost acknowledgement without writing again; `rollback` restores
//! the recorded previous image once a production window has failed.
use anyhow::{Context as _, Result};
use pharness_core::hosted_sdlc::production::{
    ProductionGitOpsPlan, ProductionPreview, ProductionPreviewAuthority, ProductionRollbackPlan,
    PREVIEW_SCHEMA,
};
use serde::Deserialize;
use serde_json::json;

use super::staging_gitops::{self, github, now, Transport};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewContext {
    authority: ProductionPreviewAuthority,
    authority_hash: String,
    preview_recorded: bool,
}

pub(super) async fn execute() -> Result<()> {
    let api = super::required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let deployment = super::required_env("PHARNESS_DEPLOYMENT_INTENT_ID")?;
    let execution = super::required_env("PHARNESS_EXECUTION_ID")?;
    let token = super::required_env("PHARNESS_WORKER_TOKEN")?;
    let phase = super::required_env("PHARNESS_PRODUCTION_PHASE")?;
    let url = format!("{api}/api/internal/deployment-intents/{deployment}/production");
    match phase.as_str() {
        "preview" => {
            let git = github::GitHub::new(
                super::required_env("PHARNESS_GIT_OBSERVER_TOKEN")?,
                "production",
            )?;
            preview(&git, &url, &token, &execution, &deployment).await
        }
        "deliver" | "observe" => {
            let observe_only = phase == "observe";
            let git = github::GitHub::new(
                super::required_env(if observe_only {
                    "PHARNESS_GIT_OBSERVER_TOKEN"
                } else {
                    "PHARNESS_GIT_WRITER_TOKEN"
                })?,
                "production",
            )?;
            let transport = Transport {
                client: git.client.clone(),
                url,
                token,
                execution,
                deployment,
                observe_only,
            };
            staging_gitops::execute_delivery::<ProductionGitOpsPlan>(transport, git).await
        }
        "rollback" => {
            let git = github::GitHub::new(
                super::required_env("PHARNESS_GIT_WRITER_TOKEN")?,
                "production_rollback",
            )?;
            let transport = Transport {
                client: git.client.clone(),
                url: format!("{url}/rollback"),
                token,
                execution,
                deployment,
                observe_only: false,
            };
            staging_gitops::execute_delivery::<ProductionRollbackPlan>(transport, git).await
        }
        _ => anyhow::bail!("production_phase_unsupported"),
    }
}

async fn preview(
    git: &github::GitHub,
    url: &str,
    token: &str,
    execution: &str,
    deployment: &str,
) -> Result<()> {
    let context: PreviewContext = super::fetch_internal_context_with_retry(
        &git.client,
        &format!("{url}/preview-context?execution_id={execution}"),
        token,
    )
    .await?;
    let authority = &context.authority;
    authority
        .validate_for_dispatch(now())
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        authority.execution_id == execution
            && authority.deployment_intent_id == deployment
            && authority.material_hash().map_err(anyhow::Error::msg)? == context.authority_hash
            && !context.preview_recorded,
        "production_preview_authority_mismatch"
    );
    let result = async {
        let base_commit_sha = git.branch().await?;
        let (path, _) = authority.coordinates().map_err(anyhow::Error::msg)?;
        let (original_blob_sha, original_content) = git.file(path, &base_commit_sha).await?;
        let preview = ProductionPreview {
            schema_version: PREVIEW_SCHEMA.into(),
            authority_hash: context.authority_hash.clone(),
            base_commit_sha,
            original_blob_sha,
            original_content,
        };
        preview.validate(authority).map_err(anyhow::Error::msg)?;
        Ok::<_, anyhow::Error>(preview)
    }
    .await;
    let body = match result {
        Ok(preview) => {
            json!({"status":"observed","execution_id":execution,"authority_hash":context.authority_hash,"preview":preview,"checked_at_ms":now()})
        }
        Err(error) => {
            json!({"status":"unavailable","execution_id":execution,"authority_hash":context.authority_hash,"error_code":staging_gitops::error_code("production", &error),"checked_at_ms":now()})
        }
    };
    let response = git
        .client
        .post(format!("{url}/preview"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .context("production_preview_acknowledgement_unknown")?;
    anyhow::ensure!(
        response.status().is_success(),
        "production_preview_not_acknowledged"
    );
    Ok(())
}
