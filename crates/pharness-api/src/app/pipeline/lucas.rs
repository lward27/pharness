//! The finite Finance pipelines publish artifacts without deployment authority.
use crate::app::identifiers::is_git_sha;
use crate::app::ApiError;
use serde_json::{json, Value};

pub(super) fn constrain_finance_build(
    manifest: &mut Value,
    production_impacting: bool,
) -> Result<(), ApiError> {
    if !matches!(
        manifest
            .pointer("/spec/pipelineRef/name")
            .and_then(Value::as_str),
        Some("pharness-yfinance-build" | "pharness-finance-frontend-build")
    ) {
        return Ok(());
    }
    if manifest
        .pointer("/metadata/namespace")
        .and_then(Value::as_str)
        != Some("tekton-pipelines")
        || production_impacting
    {
        return Err(ApiError::conflict(
            "Finance builds must use the non-deploying tekton-pipelines binding",
        ));
    }
    let params = manifest["spec"]["params"]
        .as_array()
        .ok_or_else(|| ApiError::conflict("Finance build parameters are unavailable"))?;
    let revision = params
        .iter()
        .find(|p| p["name"] == "revision")
        .and_then(|p| p["value"].as_str())
        .filter(|sha| is_git_sha(sha) && sha.bytes().all(|c| !c.is_ascii_uppercase()))
        .ok_or_else(|| {
            ApiError::conflict("Finance builds require a full lowercase source commit")
        })?;
    if let Some(merged) = manifest
        .pointer("/metadata/annotations/pharness.lucas.engineering~1source-commit")
        .and_then(Value::as_str)
    {
        if revision != merged {
            return Err(ApiError::conflict(
                "Finance build revision differs from the observed merged source",
            ));
        }
    }
    for param in params {
        let allowed = match param["name"].as_str() {
            Some("revision") => true,
            Some("dockerfile") => param["value"] == "./Dockerfile",
            Some("context") => param["value"] == "./",
            _ => false,
        };
        if !allowed {
            return Err(ApiError::conflict(
                "Finance builds use the committed root Dockerfile and complete application context",
            ));
        }
    }
    let workspaces = manifest["spec"]["workspaces"]
        .as_array_mut()
        .ok_or_else(|| ApiError::conflict("Finance build workspace is unavailable"))?;
    if workspaces.len() != 1
        || workspaces[0]["name"] != "shared-data"
        || workspaces[0].get("persistentVolumeClaim").is_some()
        || workspaces[0].pointer("/volumeClaimTemplate/spec/accessModes")
            != Some(&json!(["ReadWriteOnce"]))
        || workspaces[0].pointer("/volumeClaimTemplate/spec/resources/requests/storage")
            != Some(&json!("1Gi"))
    {
        return Err(ApiError::conflict(
            "Finance builds require their own 1Gi ReadWriteOnce workspace",
        ));
    }
    workspaces[0]["volumeClaimTemplate"]["spec"]["storageClassName"] = json!("local-path");
    manifest["spec"]["taskRunTemplate"] = json!({
        "serviceAccountName":"pharness-finance-build",
        "podTemplate":{
            "nodeSelector":{"kubernetes.io/arch":"amd64"},
            "securityContext":{"fsGroup":65532}
        }
    });
    // Preserve the existing Tekton default rather than extending execution limits.
    manifest["spec"]["timeouts"] = json!({"pipeline":"1h0m0s"});
    Ok(())
}
