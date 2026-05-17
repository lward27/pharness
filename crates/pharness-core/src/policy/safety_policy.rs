use super::{classify_command, CommandClass};
use crate::{AgentAction, ApprovalKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow {
        risk: RiskLevel,
        summary: String,
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
    Plan,
    DenyAllWrites,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyPolicy {
    pub mode: PolicyMode,
    pub allow_read_only_shell: bool,
    pub require_approval_for_writes: bool,
    pub require_approval_for_network: bool,
    pub require_approval_for_destructive: bool,
    pub deny_privileged: bool,
    pub deny_secret_access: bool,
}

impl Default for SafetyPolicy {
    fn default() -> Self {
        Self {
            mode: PolicyMode::Default,
            allow_read_only_shell: true,
            require_approval_for_writes: true,
            require_approval_for_network: true,
            require_approval_for_destructive: true,
            deny_privileged: true,
            deny_secret_access: true,
        }
    }
}

impl SafetyPolicy {
    pub fn evaluate_action(&self, action: &AgentAction) -> PolicyDecision {
        match action {
            AgentAction::Respond { .. }
            | AgentAction::Finish { .. }
            | AgentAction::RequestApproval { .. }
            | AgentAction::ReadFile { .. }
            | AgentAction::ListDir { .. }
            | AgentAction::SearchFiles { .. }
            | AgentAction::GitDiff { .. }
            | AgentAction::GitStatus { .. } => PolicyDecision::Allow {
                risk: RiskLevel::Low,
                summary: format!("{} is read-only", action.kind_name()),
            },
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
            AgentAction::WriteFile { path, .. } | AgentAction::PatchFile { path, .. } => {
                self.evaluate_write_action(path.as_str())
            }
            AgentAction::RunShell { cmd, .. } => self.evaluate_command(cmd),
        }
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
                PolicyDecision::Allow {
                    risk: RiskLevel::Medium,
                    summary: format!("trusted write allowed for {path}"),
                }
            }
            PolicyMode::TrustedWrites => PolicyDecision::Allow {
                risk: RiskLevel::Medium,
                summary: format!("trusted write allowed for {path}"),
            },
            PolicyMode::Default if self.require_approval_for_writes => PolicyDecision::Ask {
                approval_kind: ApprovalKind::FileWrite,
                risk: RiskLevel::Medium,
                summary: format!("file write requires approval: {path}"),
            },
            PolicyMode::Default => PolicyDecision::Allow {
                risk: RiskLevel::Medium,
                summary: format!("file write allowed: {path}"),
            },
        }
    }

    fn evaluate_command(&self, command: &str) -> PolicyDecision {
        match classify_command(command) {
            CommandClass::SafeReadOnly if self.allow_read_only_shell => PolicyDecision::Allow {
                risk: RiskLevel::Low,
                summary: format!("read-only shell command allowed: {command}"),
            },
            CommandClass::SafeReadOnly => PolicyDecision::Ask {
                approval_kind: ApprovalKind::ShellCommand,
                risk: RiskLevel::Low,
                summary: format!("shell command requires approval: {command}"),
            },
            CommandClass::WriteLocalProject => {
                if self.mode == PolicyMode::TrustedWrites && !self.require_approval_for_writes {
                    PolicyDecision::Allow {
                        risk: RiskLevel::Medium,
                        summary: format!("trusted local write command allowed: {command}"),
                    }
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
            CommandClass::DestructiveLocal => PolicyDecision::Allow {
                risk: RiskLevel::High,
                summary: format!("destructive command allowed by policy: {command}"),
            },
            CommandClass::Network if self.require_approval_for_network => PolicyDecision::Ask {
                approval_kind: ApprovalKind::Network,
                risk: RiskLevel::High,
                summary: format!(
                    "network or cluster-impacting command requires approval: {command}"
                ),
            },
            CommandClass::Network => PolicyDecision::Allow {
                risk: RiskLevel::High,
                summary: format!("network command allowed by policy: {command}"),
            },
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

        PolicyDecision::Allow {
            risk: RiskLevel::Low,
            summary: format!("{action_name} is a typed read-only cluster capability"),
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

#[cfg(test)]
mod tests {
    use super::{PolicyDecision, RiskLevel, SafetyPolicy};
    use crate::AgentAction;

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
}
