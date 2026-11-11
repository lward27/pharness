use super::audit::{
    arguments_sha256, CampusAuditError, CampusCapabilityAuditSink, CampusCapabilityEvent,
};
use super::registry::{campus_tool_binding, CampusToolBinding};
use super::selector::CampusResourceSelector;
use super::{
    CampusActorRef, CampusAuthoritySource, CampusAuthorityStatus, CampusAuthorizedEffect,
    CampusBrokerClock, CampusCaller, CampusCapabilityRequest, CampusEffectClass,
    CampusEffectExecutor, CampusSessionAuthority, CampusTrustedApproval,
};
use crate::campus_policy::{
    command_line, CampusApproverRole, CampusDecisionOutcome, CampusDecisionRecord,
    CampusPolicySource,
};
use pharness_core::{AgentAction, PolicyMode, RunScope, SafetyPolicy, ToolError, ToolResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The SDLC baseline every Campus decision is evaluated against. Writes
/// inside the session workspace are the student's own work and need no
/// approval; shell commands other than read-only ones need a role approval;
/// destructive commands need an instructor; network, environment-setup,
/// privileged and secret-accessing operations are refused. Permission grants
/// never apply to Campus sessions.
pub fn campus_baseline_policy() -> SafetyPolicy {
    SafetyPolicy {
        subject: "campus:session".into(),
        environment: "campus".into(),
        mode: PolicyMode::SupervisedAutonomy,
        allow_read_only_shell: true,
        require_approval_for_writes: false,
        require_approval_for_network: true,
        require_approval_for_destructive: true,
        deny_privileged: true,
        deny_secret_access: true,
        permission_grants: Vec::new(),
    }
}

/// Fragments of a command line that point at platform or broker credentials.
/// The sandbox must not contain any of them; refusing the command as well is
/// defence in depth and, like the command classifier, heuristic.
const CREDENTIAL_REFERENCES: &[&str] = &[
    "/var/run/secrets",
    "/run/secrets",
    "serviceaccount/token",
    "pharness_worker_token",
    "pharness_operator_token",
    "169.254.169.254",
    "metadata.google.internal",
];

/// Why the broker reached its decision. Every value other than
/// `authorized`, `approval_required` and `policy_denied` is a broker check
/// that failed before policy was consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusCapabilityReason {
    Authorized,
    ApprovalRequired,
    PolicyDenied,
    UnknownSession,
    AuthorityUnavailable,
    CallerNotSessionOwner,
    RuntimeSessionMismatch,
    SessionRevoked,
    SessionExpired,
    PolicyHashMismatch,
    ProfileHashMismatch,
    UndeclaredCapability,
    InvalidResource,
    UndeclaredResource,
    UntrustedApproval,
    CredentialReference,
    PrivilegedEffectNotApproved,
}

impl CampusCapabilityReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authorized => "authorized",
            Self::ApprovalRequired => "approval_required",
            Self::PolicyDenied => "policy_denied",
            Self::UnknownSession => "unknown_session",
            Self::AuthorityUnavailable => "authority_unavailable",
            Self::CallerNotSessionOwner => "caller_not_session_owner",
            Self::RuntimeSessionMismatch => "runtime_session_mismatch",
            Self::SessionRevoked => "session_revoked",
            Self::SessionExpired => "session_expired",
            Self::PolicyHashMismatch => "policy_hash_mismatch",
            Self::ProfileHashMismatch => "profile_hash_mismatch",
            Self::UndeclaredCapability => "undeclared_capability",
            Self::InvalidResource => "invalid_resource",
            Self::UndeclaredResource => "undeclared_resource",
            Self::UntrustedApproval => "untrusted_approval",
            Self::CredentialReference => "credential_reference",
            Self::PrivilegedEffectNotApproved => "privileged_effect_not_approved",
        }
    }
}

/// The broker's decision for one request.
#[derive(Debug, Clone)]
pub struct CampusCapabilityDecision {
    pub outcome: CampusDecisionOutcome,
    pub reason: CampusCapabilityReason,
    /// The authority whose rule decided: the platform for broker checks,
    /// otherwise the source the effective policy attributes the decision to.
    pub decided_by: CampusPolicySource,
    /// For the caller only. It may quote the action, so it is never audited.
    pub summary: String,
    pub binding: Option<&'static CampusToolBinding>,
    pub resource: Option<CampusResourceSelector>,
    pub effect: Option<CampusEffectClass>,
    pub policy: Option<CampusDecisionRecord>,
    pub approver: Option<CampusActorRef>,
    pub(super) authority: Option<Arc<CampusSessionAuthority>>,
}

impl CampusCapabilityDecision {
    fn deny(
        authority: Option<Arc<CampusSessionAuthority>>,
        reason: CampusCapabilityReason,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            outcome: CampusDecisionOutcome::Deny,
            reason,
            decided_by: CampusPolicySource::Platform,
            summary: summary.into(),
            binding: None,
            resource: None,
            effect: None,
            policy: None,
            approver: None,
            authority,
        }
    }

    pub fn is_allow(&self) -> bool {
        self.outcome == CampusDecisionOutcome::Allow
    }

    /// The session authority the decision was made under, when the claimed
    /// session resolved.
    pub fn authority(&self) -> Option<&CampusSessionAuthority> {
        self.authority.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct CampusCapabilityOutcome {
    pub decision: CampusCapabilityDecision,
    /// Present only when the effect ran. It came from the student's sandbox
    /// and is untrusted, including a reported success.
    pub result: Option<Result<ToolResult, ToolError>>,
}

impl CampusCapabilityOutcome {
    pub fn executed(&self) -> bool {
        self.result.is_some()
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CampusBrokerError {
    /// The decision could not be recorded, so the effect did not run.
    #[error("campus capability decision could not be recorded; nothing ran: {0}")]
    AuditUnavailable(CampusAuditError),
    /// The effect ran but its result could not be recorded.
    #[error("campus capability result could not be recorded: {0}")]
    AuditUnavailableAfterEffect(CampusAuditError),
}

/// The trusted boundary in front of every Campus tool effect.
pub struct CampusCapabilityBroker {
    source: Arc<dyn CampusAuthoritySource>,
    executor: Arc<dyn CampusEffectExecutor>,
    audit: Arc<dyn CampusCapabilityAuditSink>,
    clock: Arc<dyn CampusBrokerClock>,
}

impl CampusCapabilityBroker {
    pub fn new(
        source: Arc<dyn CampusAuthoritySource>,
        executor: Arc<dyn CampusEffectExecutor>,
        audit: Arc<dyn CampusCapabilityAuditSink>,
        clock: Arc<dyn CampusBrokerClock>,
    ) -> Self {
        Self {
            source,
            executor,
            audit,
            clock,
        }
    }

    /// The single authorization function for every Campus call path. It
    /// has no side effects other than reading the authority source.
    pub async fn authorize(&self, request: &CampusCapabilityRequest) -> CampusCapabilityDecision {
        use CampusCapabilityReason as R;
        let claims = &request.claims;
        let snapshot = match self.source.session(&claims.session_id).await {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                return CampusCapabilityDecision::deny(
                    None,
                    R::UnknownSession,
                    "no Campus lab session matches the claimed session",
                )
            }
            Err(_) => {
                return CampusCapabilityDecision::deny(
                    None,
                    R::AuthorityUnavailable,
                    "Campus session authority is unavailable",
                )
            }
        };
        let authority = snapshot.authority;
        let deny = |reason, summary: &str| {
            CampusCapabilityDecision::deny(Some(authority.clone()), reason, summary)
        };
        match &request.caller {
            CampusCaller::SessionRuntime { session_id } if *session_id != authority.session_id => {
                return deny(
                    R::RuntimeSessionMismatch,
                    "the runtime is bound to a different session",
                )
            }
            CampusCaller::Principal(actor) if *actor != authority.owner => {
                return deny(
                    R::CallerNotSessionOwner,
                    "no Campus lab session matches the claimed session",
                )
            }
            _ => {}
        }
        if snapshot.status != CampusAuthorityStatus::Active {
            return deny(R::SessionRevoked, "the Campus lab session is not active");
        }
        if self.clock.now() >= authority.expires_at {
            return deny(R::SessionExpired, "the Campus lab session has expired");
        }
        if claims.effective_policy_hash != authority.policy.effective_policy_hash() {
            return deny(
                R::PolicyHashMismatch,
                "the claimed effective policy is not the session's current policy",
            );
        }
        if claims.profile_hash != authority.policy.profile_hash() {
            return deny(
                R::ProfileHashMismatch,
                "the claimed profile is not the session's profile",
            );
        }

        let action = &request.action;
        let binding = match campus_tool_binding(&request.capability) {
            Some(binding) if binding.tool == action.kind_name() => binding,
            _ => {
                return deny(
                    R::UndeclaredCapability,
                    "the declared capability is not a Campus capability for this action",
                )
            }
        };
        let bound = |decision: CampusCapabilityDecision| CampusCapabilityDecision {
            binding: Some(binding),
            ..decision
        };
        let resource = match CampusResourceSelector::for_action(action) {
            Ok(resource) => resource,
            Err(error) => return bound(deny(R::InvalidResource, &error.to_string())),
        };
        let declared_matches = match &request.resource {
            Some(declared) => *declared == resource,
            None => matches!(request.caller, CampusCaller::SessionRuntime { .. }),
        };
        let with_resource = |decision: CampusCapabilityDecision| CampusCapabilityDecision {
            resource: Some(resource.clone()),
            ..bound(decision)
        };
        if !declared_matches {
            return with_resource(deny(
                R::UndeclaredResource,
                "the declared resource selector is not the one this action touches",
            ));
        }
        if references_credentials(action) {
            return with_resource(deny(
                R::CredentialReference,
                "the command refers to platform or broker credentials",
            ));
        }

        let approval = match self
            .source
            .approval(&authority.session_id, action.id())
            .await
        {
            Ok(approval) => approval,
            Err(_) => {
                return with_resource(deny(
                    R::AuthorityUnavailable,
                    "Campus session authority is unavailable",
                ))
            }
        };
        if let Some(approval) = &approval {
            if !approval_is_trusted(&authority, action, approval) {
                return with_resource(deny(
                    R::UntrustedApproval,
                    "the recorded approval is not bound to this action and session",
                ));
            }
        }

        let record = authority.policy.evaluate_sandbox_action(
            &campus_baseline_policy(),
            action,
            &RunScope::default(),
            approval.as_ref().map(|approval| &approval.approval),
        );
        let effect = CampusEffectClass::for_approval_kind(record.approval_kind);
        let approver = record
            .approved_by
            .and(approval.as_ref())
            .map(|approval| approval.approver.clone());
        let (outcome, reason) = match record.outcome {
            CampusDecisionOutcome::Allow
                if effect == CampusEffectClass::Privileged
                    && record.approved_by != Some(CampusApproverRole::Instructor) =>
            {
                (CampusDecisionOutcome::Deny, R::PrivilegedEffectNotApproved)
            }
            CampusDecisionOutcome::Allow => (CampusDecisionOutcome::Allow, R::Authorized),
            CampusDecisionOutcome::Ask => (CampusDecisionOutcome::Ask, R::ApprovalRequired),
            CampusDecisionOutcome::Deny => (CampusDecisionOutcome::Deny, R::PolicyDenied),
        };
        CampusCapabilityDecision {
            outcome,
            reason,
            decided_by: record.authority,
            summary: record.summary.clone(),
            binding: Some(binding),
            resource: Some(resource),
            effect: Some(effect),
            approver,
            policy: Some(record),
            authority: Some(authority.clone()),
        }
    }

    /// Authorize, record the decision, and only then run the effect. The
    /// executor is reachable only through this method.
    pub async fn execute(
        &self,
        request: CampusCapabilityRequest,
    ) -> Result<CampusCapabilityOutcome, CampusBrokerError> {
        let decision = self.authorize(&request).await;
        let decided = CampusCapabilityEvent::decided(&request, &decision, self.clock.now());
        self.audit
            .record(&decided)
            .await
            .map_err(CampusBrokerError::AuditUnavailable)?;
        let (Some(authority), Some(resource), true) = (
            decision.authority.as_deref(),
            decision.resource.as_ref(),
            decision.is_allow(),
        ) else {
            return Ok(CampusCapabilityOutcome {
                decision,
                result: None,
            });
        };
        let effect = CampusAuthorizedEffect {
            session_id: authority.session_id.clone(),
            course_id: authority.course_id.clone(),
            owner: authority.owner.clone(),
            resource: resource.clone(),
        };
        let result = self.executor.execute(&effect, &request.action).await;
        let finished =
            CampusCapabilityEvent::finished(&request, &decision, &result, self.clock.now());
        self.audit
            .record(&finished)
            .await
            .map_err(CampusBrokerError::AuditUnavailableAfterEffect)?;
        Ok(CampusCapabilityOutcome {
            decision,
            result: Some(result),
        })
    }
}

fn references_credentials(action: &AgentAction) -> bool {
    let AgentAction::RunWorkspaceCommand {
        executable, args, ..
    } = action
    else {
        return false;
    };
    let line = command_line(executable, args).to_ascii_lowercase();
    CREDENTIAL_REFERENCES
        .iter()
        .any(|fragment| line.contains(fragment))
        || (line.contains("/proc/") && line.contains("environ"))
}

/// An approval counts only if it was recorded for this session's tenant, for
/// exactly this action, and by an approver entitled to the role: the session
/// owner for a student approval, someone else for an instructor approval.
fn approval_is_trusted(
    authority: &CampusSessionAuthority,
    action: &AgentAction,
    approval: &CampusTrustedApproval,
) -> bool {
    let approver_matches_role = match approval.role() {
        CampusApproverRole::Student => approval.approver == authority.owner,
        CampusApproverRole::Instructor => approval.approver != authority.owner,
    };
    approval.approver.tenant_id == authority.owner.tenant_id
        && approver_matches_role
        && approval.approval.action_id == *action.id()
        && approval.arguments_sha256 == arguments_sha256(action)
}
