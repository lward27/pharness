use super::{
    bind_source_delivery_manifest, create_job_from_manifest, executor_job_terminal_state,
    git_observer_job_name, git_writer_job_name, recovery, ExecutorJobTerminalState,
    GitDeliveryExecutionReceipt, GitDeliveryExecutionRequest, GitDeliveryObservationReceipt,
    GitDeliveryObservationRequest, KubernetesJobDispatcher, RunDispatcher,
    SourceDeliveryExecutionRequest, SourceDeliveryObservationRequest,
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceJobKind {
    Writer,
    Observer,
}

pub struct SourceJobObservation {
    pub job_name: String,
    pub status: &'static str,
}

impl RunDispatcher {
    /// Observe the hash-bound operation before recovering a missing Job. Pauses
    /// can observe writers without authorizing their creation.
    pub async fn reconcile_source_delivery_job(
        &self,
        intent_id: &str,
        execution_id: &str,
        kind: SourceJobKind,
        recover_missing: bool,
    ) -> anyhow::Result<SourceJobObservation> {
        let Self::Kubernetes(dispatcher) = self else {
            anyhow::bail!("source delivery recovery requires kubernetes_job worker mode");
        };
        let (manifest, hosted) = dispatcher
            .source_delivery_manifest(intent_id, execution_id, kind)
            .await?;
        anyhow::ensure!(
            hosted,
            "source-only work cannot enter hosted source recovery"
        );
        let manifest = recovery::bind_manifest(manifest);
        let mut job = recovery::find_exact_job(
            &dispatcher.kubectl_bin,
            &dispatcher.config.namespace,
            &manifest,
        )
        .await?;
        if job.is_none() && recover_missing {
            recovery::create_or_reconcile_job(
                &dispatcher.kubectl_bin,
                &dispatcher.config.namespace,
                &manifest,
            )
            .await?;
            job = recovery::find_exact_job(
                &dispatcher.kubectl_bin,
                &dispatcher.config.namespace,
                &manifest,
            )
            .await?;
        }
        let status = match job.as_ref().map(executor_job_terminal_state) {
            None => "missing",
            Some(ExecutorJobTerminalState::Active) => "active",
            Some(ExecutorJobTerminalState::Succeeded) => "succeeded",
            Some(ExecutorJobTerminalState::Failed) => "failed",
        };
        Ok(SourceJobObservation {
            job_name: manifest["metadata"]["name"].as_str().unwrap().into(),
            status,
        })
    }
}

impl KubernetesJobDispatcher {
    async fn source_delivery_manifest(
        &self,
        intent_id: &str,
        execution_id: &str,
        kind: SourceJobKind,
    ) -> anyhow::Result<(Value, bool)> {
        anyhow::ensure!(
            match kind {
                SourceJobKind::Writer => self.git_writer_available(),
                SourceJobKind::Observer => self.git_observer_available(),
            },
            "source delivery executor is not configured"
        );
        let intent = self
            .store
            .get_source_delivery_intent(intent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("source delivery intent is unavailable"))?;
        let hosted = intent.authorization["workflow_policy_hash"].is_string();
        let recorded = match kind {
            SourceJobKind::Writer => &intent.writer_execution_id,
            SourceJobKind::Observer => &intent.observer_execution_id,
        };
        anyhow::ensure!(
            !hosted || recorded.as_deref() == Some(execution_id),
            "source delivery execution does not match the persisted operation"
        );
        let mut manifest = match kind {
            SourceJobKind::Writer => self.git_writer_job_manifest(
                &GitDeliveryExecutionRequest {
                    change_set_id: intent_id.into(),
                    execution_id: execution_id.into(),
                },
                &git_writer_job_name(execution_id),
            ),
            SourceJobKind::Observer => self.git_observer_job_manifest(
                &GitDeliveryObservationRequest {
                    change_set_id: intent_id.into(),
                    execution_id: execution_id.into(),
                },
                &git_observer_job_name(execution_id),
            ),
        };
        bind_source_delivery_manifest(
            &mut manifest,
            intent_id,
            "PHARNESS_SOURCE_DELIVERY_INTENT_ID",
        )?;
        Ok((manifest, hosted))
    }

    async fn dispatch_source_job(
        &self,
        intent_id: &str,
        execution_id: &str,
        kind: SourceJobKind,
    ) -> anyhow::Result<String> {
        let (manifest, hosted) = self
            .source_delivery_manifest(intent_id, execution_id, kind)
            .await?;
        if hosted {
            recovery::create_or_reconcile_job(
                &self.kubectl_bin,
                &self.config.namespace,
                &recovery::bind_manifest(manifest.clone()),
            )
            .await?;
        } else {
            create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        }
        let job_name = manifest["metadata"]["name"].as_str().unwrap().to_owned();
        tracing::info!(source_delivery_intent_id=%intent_id, execution_id=%execution_id, ?kind, job=%job_name,
            "source delivery job created or reconciled");
        Ok(job_name)
    }

    pub(super) async fn create_source_delivery_writer_job(
        &self,
        request: &SourceDeliveryExecutionRequest,
    ) -> anyhow::Result<GitDeliveryExecutionReceipt> {
        Ok(GitDeliveryExecutionReceipt {
            job_name: self
                .dispatch_source_job(
                    &request.source_delivery_intent_id,
                    &request.execution_id,
                    SourceJobKind::Writer,
                )
                .await?,
        })
    }

    pub(super) async fn create_source_delivery_observer_job(
        &self,
        request: &SourceDeliveryObservationRequest,
    ) -> anyhow::Result<GitDeliveryObservationReceipt> {
        Ok(GitDeliveryObservationReceipt {
            job_name: self
                .dispatch_source_job(
                    &request.source_delivery_intent_id,
                    &request.execution_id,
                    SourceJobKind::Observer,
                )
                .await?,
        })
    }
}
