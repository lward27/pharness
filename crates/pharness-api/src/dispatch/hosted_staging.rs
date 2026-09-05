use super::{
    gitops_observer_job_name, gitops_writer_job_name, recovery, GitOpsDeliveryExecutionRequest,
    GitOpsDeliveryObservationRequest, RunDispatcher,
};
use pharness_core::hosted_sdlc::staging::{HostedStagingAuthority, GITOPS_REPOSITORY};
use serde_json::{json, Value};

impl RunDispatcher {
    /// Preserve the configured worker image, isolation, credentials and limits.
    /// Observation uses the existing GitOps reader identity, never the writer.
    pub fn hosted_staging_job_manifest(
        &self,
        authority: &HostedStagingAuthority,
        observe_only: bool,
    ) -> anyhow::Result<Value> {
        authority.validate_identity().map_err(anyhow::Error::msg)?;
        let Self::Kubernetes(dispatcher) = self else {
            anyhow::bail!("hosted staging requires the existing Kubernetes executor");
        };
        let config = &dispatcher.config;
        let mut manifest = if observe_only {
            anyhow::ensure!(
                dispatcher.gitops_observer_available()
                    && config
                        .gitops_observer_allowed_repos
                        .iter()
                        .any(|repo| repo == GITOPS_REPOSITORY)
                    && config.gitops_observer_github_api_url == "https://api.github.com",
                "finite staging GitOps reader is not configured"
            );
            let request = GitOpsDeliveryObservationRequest {
                gitops_change_set_id: authority.gitops_change_set_id.clone(),
                execution_id: authority.execution_id.clone(),
            };
            dispatcher.gitops_observer_job_manifest(
                &request,
                &gitops_observer_job_name(&authority.execution_id),
            )
        } else {
            anyhow::ensure!(
                dispatcher.gitops_writer_available()
                    && config
                        .gitops_writer_allowed_repos
                        .iter()
                        .any(|repo| repo == GITOPS_REPOSITORY)
                    && config.gitops_writer_github_api_url == "https://api.github.com",
                "finite staging GitOps writer is not configured"
            );
            let request = GitOpsDeliveryExecutionRequest {
                gitops_change_set_id: authority.gitops_change_set_id.clone(),
                execution_id: authority.execution_id.clone(),
            };
            dispatcher.gitops_writer_job_manifest(
                &request,
                &gitops_writer_job_name(&authority.execution_id),
            )
        };
        let env = manifest["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array_mut()
            .unwrap();
        for entry in env.iter_mut() {
            if entry["name"] == "PHARNESS_EXECUTION_KIND" {
                entry["value"] = json!("hosted_staging_gitops");
            }
        }
        env.push(
            json!({"name":"PHARNESS_DEPLOYMENT_INTENT_ID","value":authority.deployment_intent_id}),
        );
        env.push(json!({"name":"PHARNESS_EXECUTION_ID","value":authority.execution_id}));
        env.push(json!({"name":"PHARNESS_STAGING_OBSERVE_ONLY","value":observe_only.to_string()}));
        Ok(recovery::bind_manifest(manifest))
    }
}
