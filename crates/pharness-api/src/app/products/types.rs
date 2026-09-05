use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(super) struct OrganizationResponse {
    pub(super) id: String,
    pub(super) organization_key: String,
    pub(super) display_name: String,
    pub(super) repo_mode_v1_enabled: bool,
    pub(super) repo_mode_v1_ui_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateProductRequest {
    pub(super) display_name: String,
    pub(super) description: String,
    pub(super) owner_principal: String,
    pub(super) actor: String,
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdateProductRequest {
    pub(super) display_name: Option<String>,
    pub(super) description: Option<String>,
    pub(super) owner_principal: Option<String>,
    pub(super) actor: String,
    pub(super) reason: String,
    pub(super) state_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ProductModelServiceInput {
    #[serde(default)]
    pub(super) id: Option<String>,
    pub(super) service_key: String,
    pub(super) display_name: String,
    pub(super) description: String,
    #[serde(default = "default_active_status")]
    pub(super) status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ProductModelScopeInput {
    pub(super) path_glob: String,
    pub(super) role: String,
    #[serde(default)]
    pub(super) service_id: Option<String>,
    #[serde(default)]
    pub(super) service_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ProductModelBindingInput {
    pub(super) repository_id: String,
    #[serde(default = "default_active_status")]
    pub(super) status: String,
    pub(super) scopes: Vec<ProductModelScopeInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ProductModelChangePreflightRequest {
    pub(super) services: Vec<ProductModelServiceInput>,
    pub(super) bindings: Vec<ProductModelBindingInput>,
    pub(super) actor: String,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct NormalizedProductModelService {
    pub(super) id: String,
    pub(super) service_key: String,
    pub(super) display_name: String,
    pub(super) description: String,
    pub(super) status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct NormalizedProductModelScope {
    pub(super) id: String,
    pub(super) path_glob: String,
    pub(super) role: String,
    pub(super) service_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct NormalizedProductModelBinding {
    pub(super) binding_id: String,
    pub(super) repository_id: String,
    pub(super) revision_id: String,
    pub(super) status: String,
    pub(super) scopes: Vec<NormalizedProductModelScope>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct NormalizedProductModelChange {
    pub(super) services: Vec<NormalizedProductModelService>,
    pub(super) bindings: Vec<NormalizedProductModelBinding>,
}

#[derive(Debug, Serialize)]
pub(super) struct ProductModelChangePreflightResponse {
    pub(super) product_id: String,
    pub(super) state_hash: String,
    pub(super) normalized_change: NormalizedProductModelChange,
    pub(super) resulting_snapshot: Value,
    pub(super) resulting_snapshot_hash: String,
    pub(super) preflight_hash: String,
    pub(super) predicted_mutations: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ApplyProductModelChangeRequest {
    pub(super) normalized_change: NormalizedProductModelChange,
    pub(super) state_hash: String,
    pub(super) preflight_hash: String,
    pub(super) actor: String,
    pub(super) reason: String,
}

fn default_active_status() -> String {
    "active".into()
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RepositoryRegistrationPreflightRequest {
    pub(super) repository_url: String,
    pub(super) source_commit: String,
    #[serde(default)]
    pub(super) proposer_inference_policy: Option<pharness_core::InferencePolicyRef>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RegisterRepositoryRequest {
    pub(super) repository_url: String,
    pub(super) source_commit: String,
    pub(super) preflight_hash: String,
    #[serde(default)]
    pub(super) proposer_inference_policy: Option<pharness_core::InferencePolicyRef>,
    pub(super) actor: String,
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateRepositoryOnboardingRequest {
    pub(super) product_id: String,
    pub(super) source_commit: String,
    #[serde(default)]
    pub(super) proposer_inference_policy: Option<pharness_core::InferencePolicyRef>,
    pub(super) actor: String,
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RepositoryReadinessQuery {
    pub(super) source_commit: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateRepositoryReadinessRequest {
    pub(super) source_commit: String,
    pub(super) actor: String,
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExecuteRepositoryOnboardingActionRequest {
    pub(super) actor: String,
    pub(super) reason: String,
    pub(super) state_hash: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct PutRepositoryOnboardingProposalRequest {
    pub(super) proposal: pharness_core::RepositoryOnboardingProposal,
    pub(super) actor: String,
    pub(super) reason: String,
    pub(super) state_hash: String,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct RepositoryDiscoveryContextResponse {
    pub(super) discovery_id: String,
    pub(super) onboarding_id: String,
    pub(super) repository_id: String,
    pub(super) provider: String,
    pub(super) canonical_url: String,
    pub(super) default_branch: String,
    pub(super) source_commit: String,
    pub(super) limits: pharness_core::RepositoryDiscoveryLimits,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct RepositoryDiscoveryOutcomeRequest {
    pub(super) status: String,
    #[serde(default)]
    pub(super) discovery: Option<pharness_core::RepositoryDiscovery>,
    #[serde(default)]
    pub(super) error_code: Option<String>,
    #[serde(default)]
    pub(super) error_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct InternalOnboardingPatchQuery {
    pub(super) execution_id: String,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct OnboardingPatchContextResponse {
    pub(super) onboarding_id: String,
    pub(super) execution_id: String,
    pub(super) repository_id: String,
    pub(super) provider: String,
    pub(super) canonical_url: String,
    pub(super) default_branch: String,
    pub(super) source_commit: String,
    pub(super) proposal_id: String,
    pub(super) proposal_hash: String,
    pub(super) candidate_contract: Value,
    pub(super) instructions: String,
    pub(super) remove_alias: bool,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct OnboardingPatchOutcomeRequest {
    pub(in crate::app) status: String,
    #[serde(default)]
    pub(in crate::app) patch: Option<String>,
    #[serde(default)]
    pub(in crate::app) patch_hash: Option<String>,
    #[serde(default)]
    pub(in crate::app) changed_paths: Vec<String>,
    #[serde(default)]
    pub(in crate::app) error_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct InternalOnboardingContractValidationQuery {
    pub(in crate::app) execution_id: String,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct OnboardingContractValidationContextResponse {
    pub(super) onboarding_id: String,
    pub(super) execution_id: String,
    pub(super) repository_id: String,
    pub(super) provider: String,
    pub(super) canonical_url: String,
    pub(super) source_commit: String,
    pub(super) proposal_id: String,
    pub(super) proposal_hash: String,
    pub(super) expected_contract: Value,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct OnboardingContractValidationOutcomeRequest {
    pub(in crate::app) status: String,
    #[serde(default)]
    pub(in crate::app) contract: Option<Value>,
    #[serde(default)]
    pub(in crate::app) contract_content_hash: Option<String>,
    #[serde(default)]
    pub(in crate::app) contract_source: Option<String>,
    #[serde(default)]
    pub(in crate::app) warnings: Vec<String>,
    #[serde(default)]
    pub(in crate::app) error_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct RepositoryReadinessPreparationContextResponse {
    pub(super) preparation_id: String,
    pub(super) workspace_id: String,
    pub(super) repository_id: String,
    pub(super) provider: String,
    pub(super) canonical_url: String,
    pub(super) default_branch: String,
    pub(super) source_commit: String,
    pub(super) contract_version_id: String,
    pub(super) contract_content_hash: String,
    pub(super) contract: Value,
    pub(super) environment_profile_id: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct RepositoryReadinessPreparationOutcomeRequest {
    pub(super) status: String,
    #[serde(default)]
    pub(super) resolved_commit: Option<String>,
    #[serde(default)]
    pub(super) repository_contract: Option<Value>,
    #[serde(default)]
    pub(super) repository_contract_hash: Option<String>,
    #[serde(default)]
    pub(super) environment_snapshot: Option<Value>,
    #[serde(default)]
    pub(super) snapshot_signature: Option<String>,
    #[serde(default)]
    pub(super) acceptance_results: Value,
    #[serde(default)]
    pub(super) logs: Value,
    #[serde(default)]
    pub(super) error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct RepositoryRegistrationPreflightResponse {
    pub(super) product_id: String,
    pub(super) provider: String,
    pub(super) provider_repository_id: String,
    pub(super) external_id: String,
    pub(super) canonical_url: String,
    pub(super) default_branch: String,
    pub(super) source_commit: String,
    pub(super) commit_verified: bool,
    pub(super) proposer_inference: Option<Value>,
    pub(super) already_registered_globally: bool,
    pub(super) already_bound_to_product: bool,
    pub(super) predicted_mutations: Vec<String>,
    pub(super) blockers: Vec<String>,
    pub(super) preflight_hash: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ProductResponse {
    pub(super) id: String,
    pub(super) organization_id: String,
    pub(super) product_key: String,
    pub(super) display_name: String,
    pub(super) description: String,
    pub(super) owner_principal: String,
    pub(super) state_version: u64,
    pub(super) state_hash: String,
    pub(super) current_model_snapshot_id: String,
    pub(super) current_model_snapshot_hash: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ProductsResponse {
    pub(super) products: Vec<ProductResponse>,
    pub(super) count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ProductModelSnapshotResponse {
    pub(super) id: String,
    pub(super) product_id: String,
    pub(super) version: u64,
    pub(super) model: Value,
    pub(super) content_hash: String,
    pub(super) created_by: String,
    pub(super) creation_reason: String,
    pub(super) created_at: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ServiceResponse {
    pub(super) id: String,
    pub(super) product_id: String,
    pub(super) service_key: String,
    pub(super) display_name: String,
    pub(super) description: String,
    pub(super) status: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct RepositoryResponse {
    pub(super) id: String,
    pub(super) provider: String,
    pub(super) provider_repository_id: String,
    pub(super) external_id: String,
    pub(super) canonical_url: String,
    pub(super) default_branch: String,
    pub(super) registered_commit: String,
    pub(super) state_version: u64,
    pub(super) binding_id: Option<String>,
    pub(super) binding_revision_id: Option<String>,
    pub(super) onboarding_id: Option<String>,
    pub(super) onboarding_status: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Serialize)]
pub(super) struct RepositoriesResponse {
    pub(super) repositories: Vec<RepositoryResponse>,
    pub(super) count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct RepositoryOnboardingResponse {
    pub(super) id: String,
    pub(super) product_id: String,
    pub(super) repository_id: String,
    pub(super) binding_id: String,
    pub(super) onboarding_kind: String,
    pub(super) status: String,
    pub(super) registered_commit: String,
    pub(super) resolved_commit: Option<String>,
    pub(super) current_discovery_id: Option<String>,
    pub(super) current_proposal_revision: u64,
    pub(super) approved_proposal_hash: Option<String>,
    pub(super) source_delivery_intent_id: Option<String>,
    pub(super) contract_version_id: Option<String>,
    pub(super) readiness_assessment_id: Option<String>,
    pub(super) proposer_run_id: Option<String>,
    pub(super) proposer_profile_hash: Option<String>,
    pub(super) proposer_stop_reason: Option<String>,
    pub(super) patch_execution_id: Option<String>,
    pub(super) patch_artifact_id: Option<String>,
    pub(super) patch_hash: Option<String>,
    pub(super) validation_execution_id: Option<String>,
    pub(super) validation_stop_reason: Option<String>,
    pub(super) state_version: u64,
    pub(super) state_hash: String,
    pub(super) blockers: Vec<Value>,
    pub(super) actions: Vec<RepositoryOnboardingActionResponse>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct RepositoryOnboardingActionResponse {
    pub(super) id: String,
    pub(super) lifecycle_stage: String,
    pub(super) resource: Value,
    pub(super) status: String,
    pub(super) effect_class: String,
    pub(super) external_effect_summary: String,
    pub(super) approval_requirements: Vec<String>,
    pub(super) expected_result: String,
    pub(super) requires_confirmation: bool,
    pub(super) blockers: Vec<String>,
    pub(super) state_hash: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GitHubRepositoryResponse {
    pub(super) id: u64,
    pub(super) full_name: String,
    pub(super) html_url: String,
    pub(super) default_branch: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GitHubCommitResponse {
    pub(super) sha: String,
}
