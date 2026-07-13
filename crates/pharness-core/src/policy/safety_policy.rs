use super::{classify_command, CommandClass};
use crate::{AgentAction, ApprovalKind, CapabilityKind, RunScope};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow {
        risk: RiskLevel,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        grant_id: Option<String>,
    },
    Ask {
        approval_kind: ApprovalKind,
        risk: RiskLevel,
        summary: String,
    },
    Deny {
        risk: RiskLevel,
        summary: String,
    },
}

impl PolicyDecision {
    fn allow(risk: RiskLevel, summary: impl Into<String>) -> Self {
        Self::Allow {
            risk,
            summary: summary.into(),
            grant_id: None,
        }
    }

    fn allow_by_grant(
        risk: RiskLevel,
        summary: impl Into<String>,
        grant_id: impl Into<String>,
    ) -> Self {
        Self::Allow {
            risk,
            summary: summary.into(),
            grant_id: Some(grant_id.into()),
        }
    }

    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    Default,
    TrustedWrites,
    SupervisedAutonomy,
    Plan,
    DenyAllWrites,
}

impl PolicyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::TrustedWrites => "trusted_writes",
            Self::SupervisedAutonomy => "supervised_autonomy",
            Self::Plan => "plan",
            Self::DenyAllWrites => "deny_all_writes",
        }
    }
}

impl fmt::Display for PolicyMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PolicyMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "default" => Ok(Self::Default),
            "trusted_writes" => Ok(Self::TrustedWrites),
            "supervised_autonomy" => Ok(Self::SupervisedAutonomy),
            "plan" => Ok(Self::Plan),
            "deny_all_writes" => Ok(Self::DenyAllWrites),
            other => Err(format!("unsupported policy mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyPolicy {
    #[serde(default = "default_policy_subject")]
    pub subject: String,
    #[serde(default = "default_policy_environment")]
    pub environment: String,
    pub mode: PolicyMode,
    pub allow_read_only_shell: bool,
    pub require_approval_for_writes: bool,
    pub require_approval_for_network: bool,
    pub require_approval_for_destructive: bool,
    pub deny_privileged: bool,
    pub deny_secret_access: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_grants: Vec<PermissionGrant>,
}

impl Default for SafetyPolicy {
    fn default() -> Self {
        Self {
            subject: default_policy_subject(),
            environment: default_policy_environment(),
            mode: PolicyMode::Default,
            allow_read_only_shell: true,
            require_approval_for_writes: true,
            require_approval_for_network: true,
            require_approval_for_destructive: true,
            deny_privileged: true,
            deny_secret_access: true,
            permission_grants: Vec::new(),
        }
    }
}

impl SafetyPolicy {
    pub fn evaluate_action(&self, action: &AgentAction) -> PolicyDecision {
        self.evaluate_action_in_scope(action, &RunScope::default())
    }

    pub fn evaluate_action_in_scope(
        &self,
        action: &AgentAction,
        run_scope: &RunScope,
    ) -> PolicyDecision {
        let decision = self.evaluate_action_without_grants(action);
        self.apply_permission_grants(action, run_scope, decision)
    }

    fn evaluate_action_without_grants(&self, action: &AgentAction) -> PolicyDecision {
        match action {
            AgentAction::Respond { .. }
            | AgentAction::Finish { .. }
            | AgentAction::RequestApproval { .. }
            | AgentAction::ReadFile { .. }
            | AgentAction::ListDir { .. }
            | AgentAction::SearchFiles { .. }
            | AgentAction::GitDiff { .. }
            | AgentAction::GitStatus { .. } => PolicyDecision::allow(
                RiskLevel::Low,
                format!("{} is read-only", action.kind_name()),
            ),
            AgentAction::KubernetesGet {
                resource,
                namespace,
                name,
                label_selector,
                ..
            } => self.evaluate_cluster_read(
                "kubernetes_get",
                [
                    Some(resource.as_str()),
                    namespace.as_deref(),
                    name.as_deref(),
                    label_selector.as_deref(),
                ],
            ),
            AgentAction::ArgoGetApp { app, .. } => {
                self.evaluate_cluster_read("argo_get_app", [Some(app.as_str()), None, None])
            }
            AgentAction::PrometheusQuery { query, .. } => {
                self.evaluate_cluster_read("prometheus_query", [Some(query.as_str()), None, None])
            }
            AgentAction::PrometheusInventory { .. } => {
                self.evaluate_cluster_read("prometheus_inventory", [None, None, None])
            }
            AgentAction::LokiLogSummary { query, .. } => {
                self.evaluate_cluster_read("loki_log_summary", [Some(query.as_str()), None, None])
            }
            AgentAction::TektonGetPipelineRuns {
                namespace,
                name,
                label_selector,
                ..
            }
            | AgentAction::TektonGetTaskRuns {
                namespace,
                name,
                label_selector,
                ..
            } => self.evaluate_cluster_read(
                action.kind_name(),
                [
                    namespace.as_deref(),
                    name.as_deref(),
                    label_selector.as_deref(),
                ],
            ),
            AgentAction::TektonAnalyzePipelineRun {
                namespace, name, ..
            } => self.evaluate_cluster_read(
                action.kind_name(),
                [Some(namespace.as_str()), Some(name.as_str()), None],
            ),
            AgentAction::RegistryInspectImage {
                image_ref,
                registry_base_url,
                ..
            } => self.evaluate_cluster_read(
                action.kind_name(),
                [Some(image_ref.as_str()), registry_base_url.as_deref(), None],
            ),
            AgentAction::WriteFile { path, .. } | AgentAction::PatchFile { path, .. } => {
                self.evaluate_write_action(path.as_str())
            }
            AgentAction::RunShell { cmd, .. } => self.evaluate_command(cmd),
        }
    }

    fn apply_permission_grants(
        &self,
        action: &AgentAction,
        run_scope: &RunScope,
        decision: PolicyDecision,
    ) -> PolicyDecision {
        let PolicyDecision::Ask { risk, summary, .. } = &decision else {
            return decision;
        };
        let risk = *risk;

        let Some(grant) = self
            .permission_grants
            .iter()
            .find(|grant| grant.allows(&self.subject, &self.environment, run_scope, action, risk))
        else {
            return decision;
        };

        PolicyDecision::allow_by_grant(
            risk,
            format!("{}; allowed by permission grant {}", summary, grant.id),
            grant.id.clone(),
        )
    }

    fn evaluate_write_action(&self, path: &str) -> PolicyDecision {
        match self.mode {
            PolicyMode::Plan | PolicyMode::DenyAllWrites => PolicyDecision::Deny {
                risk: RiskLevel::Medium,
                summary: format!(
                    "file write to {path} blocked by policy mode {:?}",
                    self.mode
                ),
            },
            PolicyMode::TrustedWrites if !self.require_approval_for_writes => {
                PolicyDecision::allow(
                    RiskLevel::Medium,
                    format!("trusted write allowed for {path}"),
                )
            }
            PolicyMode::TrustedWrites => PolicyDecision::allow(
                RiskLevel::Medium,
                format!("trusted write allowed for {path}"),
            ),
            PolicyMode::Default | PolicyMode::SupervisedAutonomy
                if self.require_approval_for_writes =>
            {
                PolicyDecision::Ask {
                    approval_kind: ApprovalKind::FileWrite,
                    risk: RiskLevel::Medium,
                    summary: format!("file write requires approval: {path}"),
                }
            }
            PolicyMode::Default | PolicyMode::SupervisedAutonomy => {
                PolicyDecision::allow(RiskLevel::Medium, format!("file write allowed: {path}"))
            }
        }
    }

    fn evaluate_command(&self, command: &str) -> PolicyDecision {
        match classify_command(command) {
            CommandClass::SafeReadOnly if self.allow_read_only_shell => PolicyDecision::allow(
                RiskLevel::Low,
                format!("read-only shell command allowed: {command}"),
            ),
            CommandClass::SafeReadOnly => PolicyDecision::Ask {
                approval_kind: ApprovalKind::ShellCommand,
                risk: RiskLevel::Low,
                summary: format!("shell command requires approval: {command}"),
            },
            CommandClass::WriteLocalProject => {
                if self.mode == PolicyMode::TrustedWrites && !self.require_approval_for_writes {
                    PolicyDecision::allow(
                        RiskLevel::Medium,
                        format!("trusted local write command allowed: {command}"),
                    )
                } else {
                    PolicyDecision::Ask {
                        approval_kind: ApprovalKind::ShellCommand,
                        risk: RiskLevel::Medium,
                        summary: format!("local write command requires approval: {command}"),
                    }
                }
            }
            CommandClass::DestructiveLocal if self.require_approval_for_destructive => {
                PolicyDecision::Ask {
                    approval_kind: ApprovalKind::Destructive,
                    risk: RiskLevel::High,
                    summary: format!("destructive command requires approval: {command}"),
                }
            }
            CommandClass::DestructiveLocal => PolicyDecision::allow(
                RiskLevel::High,
                format!("destructive command allowed by policy: {command}"),
            ),
            CommandClass::Network if self.require_approval_for_network => PolicyDecision::Ask {
                approval_kind: ApprovalKind::Network,
                risk: RiskLevel::High,
                summary: format!(
                    "network or cluster-impacting command requires approval: {command}"
                ),
            },
            CommandClass::Network => PolicyDecision::allow(
                RiskLevel::High,
                format!("network command allowed by policy: {command}"),
            ),
            CommandClass::Privileged if self.deny_privileged => PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                summary: format!("privileged command denied: {command}"),
            },
            CommandClass::Privileged => PolicyDecision::Ask {
                approval_kind: ApprovalKind::Privileged,
                risk: RiskLevel::Critical,
                summary: format!("privileged command requires approval: {command}"),
            },
            CommandClass::SecretAccessing if self.deny_secret_access => PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                summary: format!("secret-accessing command denied: {command}"),
            },
            CommandClass::SecretAccessing => PolicyDecision::Ask {
                approval_kind: ApprovalKind::SecretAccess,
                risk: RiskLevel::Critical,
                summary: format!("secret-accessing command requires approval: {command}"),
            },
            CommandClass::Unknown => PolicyDecision::Ask {
                approval_kind: ApprovalKind::ShellCommand,
                risk: RiskLevel::Medium,
                summary: format!("unknown shell command requires approval: {command}"),
            },
        }
    }

    fn evaluate_cluster_read<const N: usize>(
        &self,
        action_name: &str,
        fields: [Option<&str>; N],
    ) -> PolicyDecision {
        if self.deny_secret_access && fields.into_iter().flatten().any(looks_secret_accessing) {
            return PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                summary: format!("{action_name} denied because it appears to access secrets"),
            };
        }

        PolicyDecision::allow(
            RiskLevel::Low,
            format!("{action_name} is a typed read-only cluster capability"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub id: String,
    pub subject: String,
    pub scope: PermissionGrantScope,
    pub policy: PermissionGrantPolicy,
    pub expires_at: Option<String>,
}

impl PermissionGrant {
    fn allows(
        &self,
        subject: &str,
        environment: &str,
        run_scope: &RunScope,
        action: &AgentAction,
        risk: RiskLevel,
    ) -> bool {
        self.subject == subject
            && self.scope.allows_environment(environment)
            && self.scope.allows_run_scope(run_scope)
            && self.policy.policy_mode == PolicyMode::TrustedWrites
            && matches!(
                action,
                AgentAction::WriteFile { .. } | AgentAction::PatchFile { .. }
            )
            && self.scope.allows_action(action)
            && self.scope.allows_risk(risk)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionGrantScope {
    pub environment: Option<String>,
    pub capability_kinds: Vec<CapabilityKind>,
    pub actions: Vec<String>,
    pub max_risk: Option<RiskLevel>,
    pub namespaces: Vec<String>,
    pub repos: Vec<String>,
    pub branches: Vec<String>,
    pub work_plan_ids: Vec<String>,
    pub change_set_ids: Vec<String>,
    pub pipeline_intent_ids: Vec<String>,
    pub production_impacting: Option<bool>,
}

impl PermissionGrantScope {
    fn allows_environment(&self, environment: &str) -> bool {
        self.environment.as_deref() == Some(environment)
    }

    fn allows_action(&self, action: &AgentAction) -> bool {
        !self.capability_kinds.is_empty()
            && self
                .capability_kinds
                .contains(&capability_kind_for_action(action))
            && (self.actions.is_empty()
                || self
                    .actions
                    .iter()
                    .any(|allowed| allowed == action.kind_name()))
    }

    fn allows_risk(&self, risk: RiskLevel) -> bool {
        self.max_risk
            .map(|max_risk| risk.rank() <= max_risk.rank())
            .unwrap_or(true)
    }

    fn allows_run_scope(&self, run_scope: &RunScope) -> bool {
        string_scope_matches(&self.namespaces, run_scope.namespace.as_deref())
            && string_scope_matches(&self.repos, run_scope.repo.as_deref())
            && string_scope_matches(&self.branches, run_scope.branch.as_deref())
            && string_scope_matches(&self.work_plan_ids, run_scope.work_plan_id.as_deref())
            && string_scope_matches(&self.change_set_ids, run_scope.change_set_id.as_deref())
            && self
                .production_impacting
                .map(|required| required == run_scope.production_impacting)
                .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrantPolicy {
    pub policy_mode: PolicyMode,
}

fn default_policy_subject() -> String {
    "agent:local-worker".to_string()
}

fn default_policy_environment() -> String {
    "local".to_string()
}

fn capability_kind_for_action(action: &AgentAction) -> CapabilityKind {
    match action {
        AgentAction::Respond { .. }
        | AgentAction::RequestApproval { .. }
        | AgentAction::Finish { .. } => CapabilityKind::AgentControl,
        AgentAction::ReadFile { .. }
        | AgentAction::WriteFile { .. }
        | AgentAction::PatchFile { .. }
        | AgentAction::ListDir { .. }
        | AgentAction::SearchFiles { .. } => CapabilityKind::Filesystem,
        AgentAction::RunShell { .. } => CapabilityKind::Shell,
        AgentAction::GitDiff { .. } | AgentAction::GitStatus { .. } => CapabilityKind::Git,
        AgentAction::KubernetesGet { .. } => CapabilityKind::KubernetesRead,
        AgentAction::ArgoGetApp { .. } => CapabilityKind::ArgoRead,
        AgentAction::PrometheusQuery { .. }
        | AgentAction::PrometheusInventory { .. }
        | AgentAction::LokiLogSummary { .. } => CapabilityKind::ObservabilityRead,
        AgentAction::TektonGetPipelineRuns { .. }
        | AgentAction::TektonGetTaskRuns { .. }
        | AgentAction::TektonAnalyzePipelineRun { .. } => CapabilityKind::TektonRead,
        AgentAction::RegistryInspectImage { .. } => CapabilityKind::RegistryRead,
    }
}

impl RiskLevel {
    fn rank(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

fn looks_secret_accessing(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "secret",
        "token",
        "password",
        "credential",
        "kubeconfig",
        "private_key",
    ]
    .into_iter()
    .any(|needle| value.contains(needle))
}

fn string_scope_matches(allowed: &[String], actual: Option<&str>) -> bool {
    allowed.is_empty()
        || actual
            .map(|actual| allowed.iter().any(|allowed| allowed == actual))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionGrant, PermissionGrantPolicy, PermissionGrantScope, PolicyDecision, PolicyMode,
        RiskLevel, SafetyPolicy,
    };
    use crate::{AgentAction, CapabilityKind, RunScope};

    #[test]
    fn allows_read_only_file_actions() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::ListDir {
            id: "act_list".into(),
            reason: "list".to_string(),
            path: ".".into(),
            depth: 1,
        });

        assert!(decision.is_allow());
    }

    #[test]
    fn asks_for_file_writes_by_default() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write".to_string(),
            path: "README.md".into(),
            content: "hello".to_string(),
        });

        assert!(matches!(
            decision,
            PolicyDecision::Ask {
                risk: RiskLevel::Medium,
                ..
            }
        ));
    }

    #[test]
    fn permission_grant_allows_matching_file_write() {
        let policy = SafetyPolicy {
            permission_grants: vec![local_write_grant("grant_local")],
            ..SafetyPolicy::default()
        };

        let decision = policy.evaluate_action(&AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write".to_string(),
            path: "README.md".into(),
            content: "hello".to_string(),
        });

        match decision {
            PolicyDecision::Allow { risk, grant_id, .. } => {
                assert_eq!(risk, RiskLevel::Medium);
                assert_eq!(grant_id.as_deref(), Some("grant_local"));
            }
            other => panic!("expected allow decision, got {other:?}"),
        }
    }

    #[test]
    fn permission_grant_does_not_override_denials_or_wrong_subject() {
        let denied = SafetyPolicy {
            mode: PolicyMode::DenyAllWrites,
            permission_grants: vec![local_write_grant("grant_local")],
            ..SafetyPolicy::default()
        }
        .evaluate_action(&AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write".to_string(),
            path: "README.md".into(),
            content: "hello".to_string(),
        });
        let wrong_subject = SafetyPolicy {
            subject: "agent:other".to_string(),
            permission_grants: vec![local_write_grant("grant_local")],
            ..SafetyPolicy::default()
        }
        .evaluate_action(&AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write".to_string(),
            path: "README.md".into(),
            content: "hello".to_string(),
        });

        assert!(matches!(denied, PolicyDecision::Deny { .. }));
        assert!(matches!(wrong_subject, PolicyDecision::Ask { .. }));
    }

    #[test]
    fn permission_grant_requires_matching_environment() {
        let decision = SafetyPolicy {
            environment: "staging".to_string(),
            permission_grants: vec![local_write_grant("grant_local")],
            ..SafetyPolicy::default()
        }
        .evaluate_action(&AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write".to_string(),
            path: "README.md".into(),
            content: "hello".to_string(),
        });

        assert!(matches!(decision, PolicyDecision::Ask { .. }));
    }

    #[test]
    fn permission_grant_requires_matching_run_scope() {
        let policy = SafetyPolicy {
            permission_grants: vec![scoped_write_grant("grant_scoped")],
            ..SafetyPolicy::default()
        };
        let matching = RunScope {
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            work_plan_id: None,
            change_set_id: None,
            production_impacting: false,
        };
        let wrong_namespace = RunScope {
            namespace: Some("apps-prod".to_string()),
            ..matching.clone()
        };
        let production_scope = RunScope {
            production_impacting: true,
            ..matching.clone()
        };

        let allowed = policy.evaluate_action_in_scope(&write_action(), &matching);
        let missing_scope = policy.evaluate_action(&write_action());
        let wrong_namespace = policy.evaluate_action_in_scope(&write_action(), &wrong_namespace);
        let production = policy.evaluate_action_in_scope(&write_action(), &production_scope);

        assert!(matches!(
            allowed,
            PolicyDecision::Allow {
                grant_id: Some(_),
                ..
            }
        ));
        assert!(matches!(missing_scope, PolicyDecision::Ask { .. }));
        assert!(matches!(wrong_namespace, PolicyDecision::Ask { .. }));
        assert!(matches!(production, PolicyDecision::Ask { .. }));
    }

    #[test]
    fn permission_grant_requires_matching_sdlc_envelope() {
        let mut grant = scoped_write_grant("grant_envelope");
        grant.scope.work_plan_ids = vec!["wplan_1".to_string()];
        grant.scope.change_set_ids = vec!["cset_1".to_string()];
        let policy = SafetyPolicy {
            permission_grants: vec![grant],
            ..SafetyPolicy::default()
        };
        let matching = RunScope {
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            work_plan_id: Some("wplan_1".to_string()),
            change_set_id: Some("cset_1".to_string()),
            production_impacting: false,
        };
        let wrong_change_set = RunScope {
            change_set_id: Some("cset_2".to_string()),
            ..matching.clone()
        };
        let missing_change_set = RunScope {
            change_set_id: None,
            ..matching.clone()
        };

        let allowed = policy.evaluate_action_in_scope(&write_action(), &matching);
        let wrong_change_set = policy.evaluate_action_in_scope(&write_action(), &wrong_change_set);
        let missing_change_set =
            policy.evaluate_action_in_scope(&write_action(), &missing_change_set);

        assert!(matches!(
            allowed,
            PolicyDecision::Allow {
                grant_id: Some(_),
                ..
            }
        ));
        assert!(matches!(wrong_change_set, PolicyDecision::Ask { .. }));
        assert!(matches!(missing_change_set, PolicyDecision::Ask { .. }));
    }

    #[test]
    fn permission_grant_scope_deserializes_sdlc_metadata() {
        let scope: PermissionGrantScope = serde_json::from_value(serde_json::json!({
            "environment": "dev",
            "capability_kinds": ["filesystem"],
            "actions": ["write_file"],
            "max_risk": "medium",
            "namespaces": ["apps-dev"],
            "repos": ["git@example.test/team/app.git"],
            "branches": ["feature/pharness"],
            "work_plan_ids": ["wplan_1"],
            "change_set_ids": ["cset_1"],
            "production_impacting": false
        }))
        .unwrap();

        assert_eq!(scope.namespaces, vec!["apps-dev"]);
        assert_eq!(scope.repos, vec!["git@example.test/team/app.git"]);
        assert_eq!(scope.branches, vec!["feature/pharness"]);
        assert_eq!(scope.work_plan_ids, vec!["wplan_1"]);
        assert_eq!(scope.change_set_ids, vec!["cset_1"]);
        assert_eq!(scope.production_impacting, Some(false));
    }

    #[test]
    fn denies_privileged_shell_by_default() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::RunShell {
            id: "act_shell".into(),
            reason: "shell".to_string(),
            cmd: "sudo whoami".to_string(),
            cwd: None,
            timeout_ms: None,
            dry_run: false,
        });

        assert!(matches!(
            decision,
            PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                ..
            }
        ));
    }

    #[test]
    fn allows_typed_cluster_reads() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::KubernetesGet {
            id: "act_kube".into(),
            reason: "inspect pods".to_string(),
            resource: "pods".to_string(),
            namespace: Some("argocd".to_string()),
            name: None,
            all_namespaces: false,
            label_selector: None,
        });

        assert!(matches!(
            decision,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
    }

    #[test]
    fn denies_secret_shaped_cluster_reads() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::KubernetesGet {
            id: "act_secret".into(),
            reason: "read secret".to_string(),
            resource: "secrets".to_string(),
            namespace: Some("argocd".to_string()),
            name: None,
            all_namespaces: false,
            label_selector: None,
        });

        assert!(matches!(
            decision,
            PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                ..
            }
        ));
    }

    #[test]
    fn allows_prometheus_inventory() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::PrometheusInventory {
            id: "act_prom_inventory".into(),
            reason: "inspect observability health".to_string(),
        });

        assert!(matches!(
            decision,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
    }

    #[test]
    fn allows_registry_image_inspection_and_denies_secret_shaped_refs() {
        let allowed = SafetyPolicy::default().evaluate_action(&AgentAction::RegistryInspectImage {
            id: "act_registry".into(),
            reason: "inspect image evidence".to_string(),
            image_ref: "registry.example.test/team/checkout-api:v1".to_string(),
            registry_base_url: Some("https://registry.example.test".to_string()),
        });
        let denied = SafetyPolicy::default().evaluate_action(&AgentAction::RegistryInspectImage {
            id: "act_registry_secret".into(),
            reason: "inspect image evidence".to_string(),
            image_ref: "registry.example.test/team/secret-token:v1".to_string(),
            registry_base_url: None,
        });

        assert!(matches!(
            allowed,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
        assert!(matches!(
            denied,
            PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                ..
            }
        ));
    }

    #[test]
    fn allows_loki_log_summary_and_denies_secret_shaped_query() {
        let allowed = SafetyPolicy::default().evaluate_action(&AgentAction::LokiLogSummary {
            id: "act_loki".into(),
            reason: "inspect logs".to_string(),
            query: "{namespace=\"apps-dev\"}".to_string(),
            since_seconds: Some(900),
            limit: Some(25),
        });
        let denied = SafetyPolicy::default().evaluate_action(&AgentAction::LokiLogSummary {
            id: "act_loki_secret".into(),
            reason: "inspect logs".to_string(),
            query: "{namespace=\"secret-store\"}".to_string(),
            since_seconds: Some(900),
            limit: Some(25),
        });

        assert!(matches!(
            allowed,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
        assert!(matches!(
            denied,
            PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                ..
            }
        ));
    }

    #[test]
    fn allows_typed_tekton_reads() {
        let decision =
            SafetyPolicy::default().evaluate_action(&AgentAction::TektonGetPipelineRuns {
                id: "act_tekton".into(),
                reason: "inspect pipeline runs".to_string(),
                namespace: Some("ci".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            });

        assert!(matches!(
            decision,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
    }

    #[test]
    fn denies_secret_shaped_tekton_reads() {
        let decision =
            SafetyPolicy::default().evaluate_action(&AgentAction::TektonGetPipelineRuns {
                id: "act_tekton_secret".into(),
                reason: "inspect pipeline runs".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            });

        assert!(matches!(
            decision,
            PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                ..
            }
        ));
    }

    #[test]
    fn allows_typed_tekton_task_run_reads() {
        let decision = SafetyPolicy::default().evaluate_action(&AgentAction::TektonGetTaskRuns {
            id: "act_tekton_tasks".into(),
            reason: "inspect task runs".to_string(),
            namespace: Some("ci".to_string()),
            name: None,
            all_namespaces: false,
            label_selector: None,
        });

        assert!(matches!(
            decision,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
    }

    #[test]
    fn allows_typed_tekton_pipeline_run_analysis() {
        let decision =
            SafetyPolicy::default().evaluate_action(&AgentAction::TektonAnalyzePipelineRun {
                id: "act_tekton_analysis".into(),
                reason: "analyze pipeline run".to_string(),
                namespace: "ci".to_string(),
                name: "build-app".to_string(),
            });

        assert!(matches!(
            decision,
            PolicyDecision::Allow {
                risk: RiskLevel::Low,
                ..
            }
        ));
    }

    #[test]
    fn denies_secret_shaped_tekton_pipeline_run_analysis() {
        let decision =
            SafetyPolicy::default().evaluate_action(&AgentAction::TektonAnalyzePipelineRun {
                id: "act_tekton_analysis_secret".into(),
                reason: "analyze pipeline run".to_string(),
                namespace: "ci".to_string(),
                name: "token-build".to_string(),
            });

        assert!(matches!(
            decision,
            PolicyDecision::Deny {
                risk: RiskLevel::Critical,
                ..
            }
        ));
    }

    fn local_write_grant(id: &str) -> PermissionGrant {
        PermissionGrant {
            id: id.to_string(),
            subject: "agent:local-worker".to_string(),
            scope: PermissionGrantScope {
                environment: Some("local".to_string()),
                capability_kinds: vec![CapabilityKind::Filesystem],
                actions: vec!["write_file".to_string(), "patch_file".to_string()],
                max_risk: Some(RiskLevel::Medium),
                namespaces: Vec::new(),
                repos: Vec::new(),
                branches: Vec::new(),
                work_plan_ids: Vec::new(),
                change_set_ids: Vec::new(),
                pipeline_intent_ids: Vec::new(),
                production_impacting: None,
            },
            policy: PermissionGrantPolicy {
                policy_mode: PolicyMode::TrustedWrites,
            },
            expires_at: None,
        }
    }

    fn scoped_write_grant(id: &str) -> PermissionGrant {
        let mut grant = local_write_grant(id);
        grant.scope.namespaces = vec!["apps-dev".to_string()];
        grant.scope.repos = vec!["git@example.test/team/app.git".to_string()];
        grant.scope.branches = vec!["feature/pharness".to_string()];
        grant.scope.production_impacting = Some(false);
        grant
    }

    fn write_action() -> AgentAction {
        AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write".to_string(),
            path: "README.md".into(),
            content: "hello".to_string(),
        }
    }
}
