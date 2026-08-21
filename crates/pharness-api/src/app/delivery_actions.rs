pub(in crate::app) const GIT_DELIVERY_ACTIONS: [&str; 4] = [
    "git_create_branch",
    "git_commit",
    "git_push",
    "github_create_pull_request",
];
pub(in crate::app) const GITOPS_DELIVERY_ACTIONS: [&str; 4] = [
    "git_create_branch",
    "git_commit",
    "git_push",
    "github_create_pull_request",
];
pub(in crate::app) const PIPELINE_DELIVERY_ACTIONS: [&str; 1] = ["tekton_create_pipeline_run"];
pub(in crate::app) const ARGO_SYNC_ACTIONS: [&str; 1] = ["argocd_sync"];
pub(in crate::app) const CLUSTER_DELIVERY_ACTIONS: [&str; 2] =
    ["tekton_create_pipeline_run", "argocd_sync"];
pub(in crate::app) const PRODUCTION_DELIVERY_ACTIONS: [&str; 1] = ["production_action"];
