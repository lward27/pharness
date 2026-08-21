use super::super::work_items::preflight::work_item_target_supported;
use super::super::ApiError;
use pharness_store::{StoredDeploymentIntent, StoredWorkItem};

#[derive(Debug, Clone)]
pub(in crate::app) struct DeploymentTarget {
    pub(in crate::app) environment: String,
    pub(in crate::app) namespace: String,
    pub(in crate::app) application: String,
}

pub(in crate::app) fn deployment_target(
    intent: &StoredDeploymentIntent,
) -> Result<DeploymentTarget, ApiError> {
    Ok(DeploymentTarget {
        environment: intent.target_environment.clone().ok_or_else(|| {
            ApiError::conflict("DeploymentIntent target_environment is required for Argo preflight")
        })?,
        namespace: intent.target_namespace.clone().ok_or_else(|| {
            ApiError::conflict("DeploymentIntent target_namespace is required for Argo preflight")
        })?,
        application: intent.argo_application.clone().ok_or_else(|| {
            ApiError::conflict("DeploymentIntent argo_application is required for Argo preflight")
        })?,
    })
}

pub(in crate::app) fn ensure_supported_deployment_target(
    work_item: &StoredWorkItem,
    target: &DeploymentTarget,
) -> Result<(), ApiError> {
    if !work_item_target_supported(work_item) {
        return Err(ApiError::conflict(
            "Argo trusted envelopes require either a non-production dev WorkItem or the exact protected production target",
        ));
    }
    if target.environment != work_item.target_environment
        || work_item.target_namespace.as_deref() != Some(target.namespace.as_str())
        || work_item.argo_application.as_deref() != Some(target.application.as_str())
    {
        return Err(ApiError::conflict(
            "DeploymentIntent target must exactly match its WorkItem target",
        ));
    }
    Ok(())
}
