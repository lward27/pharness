use super::{
    gitops_observer_job_name, gitops_writer_job_name, recovery, GitOpsDeliveryExecutionRequest,
    GitOpsDeliveryObservationRequest, RunDispatcher,
};
use pharness_core::hosted_sdlc::staging::GITOPS_REPOSITORY;
use serde_json::{json, Value};

/// Identity of one production worker Job. `preview` and `observe` run with the
/// GitOps reader; `deliver` and `rollback` receive the writer credential.
pub struct HostedProductionJob<'a> {
    pub execution_id: &'a str,
    pub deployment_intent_id: &'a str,
    pub gitops_change_set_id: &'a str,
    pub phase: &'a str,
}

impl RunDispatcher {
    /// Same worker image, isolation, credentials and limits as every other
    /// GitOps Job; the production phase only changes which transport runs.
    pub fn hosted_production_job_manifest(
        &self,
        job: &HostedProductionJob<'_>,
    ) -> anyhow::Result<Value> {
        let Self::Kubernetes(dispatcher) = self else {
            anyhow::bail!("hosted production requires the existing Kubernetes executor");
        };
        anyhow::ensure!(
            matches!(job.phase, "preview" | "deliver" | "observe" | "rollback"),
            "unsupported hosted production phase"
        );
        let config = &dispatcher.config;
        let writes = matches!(job.phase, "deliver" | "rollback");
        // The preview reader shares the execution identity with nothing else;
        // the observer recovers the writer's execution and must have its name.
        let job_execution = job.execution_id;
        let mut manifest = if writes {
            anyhow::ensure!(
                dispatcher.gitops_writer_available()
                    && config
                        .gitops_writer_allowed_repos
                        .iter()
                        .any(|repo| repo == GITOPS_REPOSITORY)
                    && config.gitops_writer_github_api_url == "https://api.github.com",
                "finite production GitOps writer is not configured"
            );
            let request = GitOpsDeliveryExecutionRequest {
                gitops_change_set_id: job.gitops_change_set_id.into(),
                execution_id: job_execution.into(),
            };
            dispatcher.gitops_writer_job_manifest(&request, &gitops_writer_job_name(job_execution))
        } else {
            anyhow::ensure!(
                dispatcher.gitops_observer_available()
                    && config
                        .gitops_observer_allowed_repos
                        .iter()
                        .any(|repo| repo == GITOPS_REPOSITORY)
                    && config.gitops_observer_github_api_url == "https://api.github.com",
                "finite production GitOps reader is not configured"
            );
            let request = GitOpsDeliveryObservationRequest {
                gitops_change_set_id: job.gitops_change_set_id.into(),
                execution_id: job_execution.into(),
            };
            dispatcher
                .gitops_observer_job_manifest(&request, &gitops_observer_job_name(job_execution))
        };
        let env = manifest["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array_mut()
            .unwrap();
        for entry in env.iter_mut() {
            if entry["name"] == "PHARNESS_EXECUTION_KIND" {
                entry["value"] = json!("hosted_production_gitops");
            }
        }
        env.push(json!({"name":"PHARNESS_DEPLOYMENT_INTENT_ID","value":job.deployment_intent_id}));
        env.push(json!({"name":"PHARNESS_EXECUTION_ID","value":job.execution_id}));
        env.push(json!({"name":"PHARNESS_PRODUCTION_PHASE","value":job.phase}));
        Ok(recovery::bind_manifest(manifest))
    }
}
