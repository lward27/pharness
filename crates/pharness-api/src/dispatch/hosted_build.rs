use super::{
    executor_job_terminal_state, recovery, tekton_executor_job_name, ExecutorJobTerminalState,
    RunDispatcher, TektonExecutionRequest,
};
use serde_json::{json, Value};

impl RunDispatcher {
    /// Render once, before dispatch. Store this complete manifest with the
    /// operation so later releases cannot silently replace the worker image.
    pub fn hosted_build_job_manifest(
        &self,
        request: &TektonExecutionRequest,
        observe_only: bool,
    ) -> anyhow::Result<Value> {
        let Self::Kubernetes(dispatcher) = self else {
            anyhow::bail!("hosted builds require the existing Kubernetes executor");
        };
        anyhow::ensure!(
            dispatcher
                .config
                .tekton_allowed_namespaces
                .contains(&request.target_namespace),
            "hosted build namespace is not allowlisted"
        );
        pharness_core::hosted_sdlc::build::validate_manifest(&request.pipeline_run_manifest)
            .map_err(anyhow::Error::msg)?;
        let job_id = if observe_only {
            format!("{}-observe", request.execution_id)
        } else {
            request.execution_id.clone()
        };
        let mut manifest =
            dispatcher.tekton_executor_job_manifest(request, &tekton_executor_job_name(&job_id));
        if observe_only {
            // Reuse the existing read-only Kubernetes identity. Recovery does
            // not inherit the executor's PipelineRun create permission.
            manifest["spec"]["template"]["spec"]["serviceAccountName"] =
                json!(dispatcher.config.service_account);
        }
        let env = manifest["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array_mut()
            .unwrap();
        env.push(json!({"name":"PHARNESS_HOSTED_BUILD","value":"true"}));
        env.push(
            json!({"name":"PHARNESS_HOSTED_BUILD_OBSERVE_ONLY","value":observe_only.to_string()}),
        );
        Ok(recovery::bind_manifest(manifest))
    }

    /// Observation never deletes a completed Job. Recovery creates the original
    /// manifest only when the caller still holds the corresponding authority.
    pub async fn reconcile_hosted_build_job(
        &self,
        manifest: &Value,
        recover_missing: bool,
    ) -> anyhow::Result<&'static str> {
        let Self::Kubernetes(dispatcher) = self else {
            anyhow::bail!("hosted build recovery requires the existing Kubernetes executor");
        };
        anyhow::ensure!(
            manifest["kind"] == "Job"
                && manifest["metadata"]["namespace"] == dispatcher.config.namespace,
            "hosted build Job namespace differs from its executor"
        );
        let mut observed = recovery::find_exact_job(
            &dispatcher.kubectl_bin,
            &dispatcher.config.namespace,
            manifest,
        )
        .await?;
        if observed.is_none() && recover_missing {
            recovery::create_or_reconcile_job(
                &dispatcher.kubectl_bin,
                &dispatcher.config.namespace,
                manifest,
            )
            .await?;
            observed = recovery::find_exact_job(
                &dispatcher.kubectl_bin,
                &dispatcher.config.namespace,
                manifest,
            )
            .await?;
        }
        Ok(match observed.as_ref().map(executor_job_terminal_state) {
            None => "missing",
            Some(ExecutorJobTerminalState::Active) => "active",
            Some(ExecutorJobTerminalState::Succeeded) => "succeeded",
            Some(ExecutorJobTerminalState::Failed) => "failed",
        })
    }
}
