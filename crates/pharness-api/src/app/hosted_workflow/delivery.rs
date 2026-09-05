use crate::app::ApiError;
use pharness_core::hosted_sdlc::HostedWorkflowPolicySnapshot;

/// The first hosted workflow supports the two reviewed Finance applications.
/// Individually valid contracts must also belong to the authorized source.
pub(in crate::app) fn validate_finance_coordinates(
    policy: &HostedWorkflowPolicySnapshot,
) -> Result<(), ApiError> {
    let binding = &policy.delivery_binding;
    let (workload, image, pipeline, staging_app, staging_path, port, health) = match binding
        .source_repo
        .as_str()
    {
        "https://github.com/lward27/yfinance_wrapper.git" => (
            "yfinance-wrapper",
            "registry.lucas.engineering/yfinance_wrapper",
            "pharness-yfinance-build",
            "yfinance-staging",
            "charts/finance-staging/yfinance/kustomization.yaml",
            8090,
            "/healthz",
        ),
        "https://github.com/lward27/finance-frontend.git" => (
            "finance-frontend",
            "registry.lucas.engineering/finance-frontend",
            "pharness-finance-frontend-build",
            "finance-frontend-staging",
            "charts/finance-staging/frontend/kustomization.yaml",
            8080,
            "/",
        ),
        _ => {
            return Err(ApiError::conflict(
                    "hosted delivery currently supports the registered yfinance and Finance frontend repositories",
                ));
        }
    };
    if binding.gitops_repo != "https://github.com/lward27/lucas_engineering.git"
        || binding.image_name != image
        || binding.staging.kustomization_path != staging_path
        || binding.production.kustomization_path != format!("charts/{workload}/kustomization.yaml")
        || policy.pipeline_contract["pipeline_ref"] != pipeline
        || policy.staging_contract["argo_application"] != staging_app
        || policy.production_contract["argo_application"] != workload
    {
        return Err(ApiError::conflict(
            "hosted source, image, pipeline and GitOps targets must match the same Finance application",
        ));
    }
    for contract in [&policy.staging_contract, &policy.production_contract] {
        let spec = &contract["contract_json"];
        if spec["workload_kind"] != "Deployment"
            || spec["workload_name"] != workload
            || spec["service_name"] != workload
            || spec["service_port"].as_u64() != Some(port)
            || spec["health_path"] != health
            || spec["prune"].as_bool().unwrap_or(false)
            || spec["force"].as_bool().unwrap_or(false)
        {
            return Err(ApiError::conflict(
                "hosted deployment coordinates must match the Finance application and preserve prune=false and force=false",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_finance_coordinates;
    use serde_json::{json, Value};

    fn finance_policy(frontend: bool) -> Value {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../pharness-core/tests/fixtures/hosted-workflow.json"
        ))
        .unwrap();
        let (repo, workload, stage_app, directory, port, health, pipeline) = if frontend {
            (
                "finance-frontend",
                "finance-frontend",
                "finance-frontend-staging",
                "frontend",
                8080,
                "/",
                "pharness-finance-frontend-build",
            )
        } else {
            (
                "yfinance_wrapper",
                "yfinance-wrapper",
                "yfinance-staging",
                "yfinance",
                8090,
                "/healthz",
                "pharness-yfinance-build",
            )
        };
        value["delivery_binding"]["source_repo"] =
            json!(format!("https://github.com/lward27/{repo}.git"));
        value["delivery_binding"]["gitops_repo"] =
            json!("https://github.com/lward27/lucas_engineering.git");
        value["delivery_binding"]["image_name"] =
            json!(format!("registry.lucas.engineering/{repo}"));
        value["delivery_binding"]["staging"]["kustomization_path"] = json!(format!(
            "charts/finance-staging/{directory}/kustomization.yaml"
        ));
        value["delivery_binding"]["production"]["kustomization_path"] =
            json!(format!("charts/{workload}/kustomization.yaml"));
        value["pipeline_contract"]["pipeline_ref"] = json!(pipeline);
        value["staging_contract"]["argo_application"] = json!(stage_app);
        value["production_contract"]["argo_application"] = json!(workload);
        for key in ["staging_contract", "production_contract"] {
            value[key]["contract_json"]["workload_name"] = json!(workload);
            value[key]["contract_json"]["service_name"] = json!(workload);
            value[key]["contract_json"]["service_port"] = json!(port);
            value[key]["contract_json"]["health_path"] = json!(health);
            value[key]["contract_json"]["prune"] = json!(false);
            value[key]["contract_json"]["force"] = json!(false);
        }
        value
    }

    #[test]
    fn both_finance_applications_have_consistent_delivery_coordinates() {
        for frontend in [false, true] {
            validate_finance_coordinates(
                &serde_json::from_value(finance_policy(frontend)).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn valid_contracts_for_the_wrong_application_cannot_authorize_delivery() {
        for (pointer, replacement) in [
            (
                "/delivery_binding/source_repo",
                json!("https://github.com/lward27/unreviewed.git"),
            ),
            (
                "/delivery_binding/gitops_repo",
                json!("https://github.com/lward27/another-cluster.git"),
            ),
            (
                "/delivery_binding/image_name",
                json!("registry.lucas.engineering/finance-frontend"),
            ),
            (
                "/delivery_binding/staging/kustomization_path",
                json!("charts/finance-staging/frontend/kustomization.yaml"),
            ),
            (
                "/delivery_binding/production/kustomization_path",
                json!("charts/finance-frontend/kustomization.yaml"),
            ),
            (
                "/pipeline_contract/pipeline_ref",
                json!("pharness-finance-frontend-build"),
            ),
            (
                "/staging_contract/argo_application",
                json!("finance-frontend-staging"),
            ),
            (
                "/production_contract/argo_application",
                json!("finance-frontend"),
            ),
            (
                "/staging_contract/contract_json/workload_name",
                json!("finance-frontend"),
            ),
            (
                "/production_contract/contract_json/service_name",
                json!("finance-frontend"),
            ),
            ("/staging_contract/contract_json/service_port", json!(8080)),
            ("/production_contract/contract_json/health_path", json!("/")),
            ("/staging_contract/contract_json/prune", json!(true)),
            ("/production_contract/contract_json/force", json!(true)),
        ] {
            let mut value = finance_policy(false);
            *value.pointer_mut(pointer).unwrap() = replacement;
            let policy = serde_json::from_value(value).unwrap();
            assert!(validate_finance_coordinates(&policy).is_err(), "{pointer}");
        }
    }
}
