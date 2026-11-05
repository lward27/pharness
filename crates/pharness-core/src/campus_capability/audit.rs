//! Approve and deny evidence for Campus capability calls.

use super::broker::{CampusCapabilityDecision, CampusCapabilityReason};
use super::registry::{CampusSelectorKind, CampusToolBinding};
use super::selector::CampusResourceSelector;
use super::{
    CampusActorRef, CampusCallPath, CampusCaller, CampusCapabilityRequest, CampusEffectClass,
};
use crate::campus_profile::CampusCapabilityScope;
use crate::policy::{
    CampusApproverRole, CampusDecisionOutcome, CampusDecisionReason, CampusPolicySource,
};
use crate::{
    canonical_json_sha256, AgentAction, ApprovalKind, CapabilityKind, RiskLevel, ToolError,
    ToolResult, ToolResultStatus,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const CAMPUS_CAPABILITY_EVENT_SCHEMA: &str = "pharness.dev/campus-capability-event/v1alpha1";
const MAX_RECORDED_IDENTIFIER_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CampusCapabilityEventKind {
    /// The broker decided; written before any effect.
    #[serde(rename = "campus_capability.decided")]
    Decided,
    /// The effect ran and the sandbox reported a result.
    #[serde(rename = "campus_capability.executed")]
    Executed,
    /// The effect ran and the executor reported an error.
    #[serde(rename = "campus_capability.failed")]
    Failed,
}

impl CampusCapabilityEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decided => "campus_capability.decided",
            Self::Executed => "campus_capability.executed",
            Self::Failed => "campus_capability.failed",
        }
    }
}

/// The capability a call declared and, when it is a Campus capability, the
/// registry binding it resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusCapabilityRef {
    /// The declared name, or its digest when it is not a plain identifier.
    pub declared: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<CapabilityKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<CampusCapabilityScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<CampusSelectorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutates: Option<bool>,
}

impl CampusCapabilityRef {
    fn new(declared: &str, binding: Option<&CampusToolBinding>) -> Self {
        Self {
            declared: recorded_identifier(declared),
            kind: binding.map(|binding| binding.capability.kind),
            scope: binding.map(|binding| binding.scope),
            selector: binding.map(|binding| binding.selector),
            mutates: binding.map(|binding| binding.capability.mutates),
        }
    }
}

/// The shape of an action's arguments without their values: strings are
/// reduced to their length, arrays to their item count and objects to their
/// entry count. Numbers and booleans, such as timeouts or a claimed success,
/// are kept. `sha256` binds the record to the exact arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusRedactedArguments {
    pub action: String,
    pub action_id: String,
    pub sha256: String,
    pub fields: BTreeMap<String, Value>,
}

impl CampusRedactedArguments {
    pub fn of(action: &AgentAction) -> Self {
        let value = action_value(action);
        let fields = value
            .as_object()
            .map(|object| {
                object
                    .iter()
                    .filter(|(key, _)| key.as_str() != "action")
                    .map(|(key, value)| (key.clone(), shape(value)))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            action: action.kind_name().into(),
            action_id: recorded_identifier(action.id().as_str()),
            sha256: digest(&value),
            fields,
        }
    }
}

/// What the sandbox reported about an effect. It is evidence of the report,
/// not of the outcome: the student's process may claim anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusUntrustedResult {
    pub trusted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_status: Option<ToolResultStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

impl CampusUntrustedResult {
    fn of(result: &Result<ToolResult, ToolError>) -> Self {
        match result {
            Ok(result) => Self {
                trusted: false,
                reported_status: Some(result.status),
                error_kind: None,
                summary_sha256: Some(digest(&json!(result.summary))),
                content_sha256: Some(digest(&result.content)),
            },
            Err(error) => Self {
                trusted: false,
                reported_status: None,
                error_kind: Some(error.kind_name().into()),
                summary_sha256: None,
                content_sha256: None,
            },
        }
    }
}

/// One approve, deny or result record. Identity and hashes come from the
/// trusted session authority and the boundary that authenticated the caller,
/// never from the request's claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusCapabilityEvent {
    pub schema_version: String,
    /// Content hash of the rest of the event.
    pub event_id: String,
    pub kind: CampusCapabilityEventKind,
    pub occurred_at: i64,
    pub call_path: CampusCallPath,
    pub caller: CampusCaller,
    /// The session the caller claimed. It is trusted only when
    /// `session_owner` is set, which means it resolved.
    pub session_id: String,
    /// Who asked: the authenticated principal on the direct path, the session
    /// owner on the tool path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<CampusActorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_owner: Option<CampusActorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course_id: Option<String>,
    pub outcome: CampusDecisionOutcome,
    pub reason: CampusCapabilityReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_reason: Option<CampusDecisionReason>,
    /// The authority whose rule decided.
    pub authority: CampusPolicySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_kind: Option<ApprovalKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<CampusApproverRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approver: Option<CampusActorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<CampusEffectClass>,
    pub capability: CampusCapabilityRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<CampusResourceSelector>,
    pub arguments: CampusRedactedArguments,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_policy_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CampusUntrustedResult>,
}

impl CampusCapabilityEvent {
    pub(super) fn decided(
        request: &CampusCapabilityRequest,
        decision: &CampusCapabilityDecision,
        now: i64,
    ) -> Self {
        Self::build(
            CampusCapabilityEventKind::Decided,
            request,
            decision,
            None,
            now,
        )
    }

    pub(super) fn finished(
        request: &CampusCapabilityRequest,
        decision: &CampusCapabilityDecision,
        result: &Result<ToolResult, ToolError>,
        now: i64,
    ) -> Self {
        let kind = match result {
            Ok(_) => CampusCapabilityEventKind::Executed,
            Err(_) => CampusCapabilityEventKind::Failed,
        };
        Self::build(
            kind,
            request,
            decision,
            Some(CampusUntrustedResult::of(result)),
            now,
        )
    }

    fn build(
        kind: CampusCapabilityEventKind,
        request: &CampusCapabilityRequest,
        decision: &CampusCapabilityDecision,
        result: Option<CampusUntrustedResult>,
        now: i64,
    ) -> Self {
        let authority = decision.authority();
        let policy = decision.policy.as_ref();
        let caller = match &request.caller {
            CampusCaller::SessionRuntime { session_id } => CampusCaller::SessionRuntime {
                session_id: recorded_identifier(session_id),
            },
            CampusCaller::Principal(actor) => CampusCaller::Principal(actor.clone()),
        };
        let actor = match &request.caller {
            CampusCaller::Principal(actor) => Some(actor.clone()),
            CampusCaller::SessionRuntime { .. } => {
                authority.map(|authority| authority.owner.clone())
            }
        };
        let mut event = Self {
            schema_version: CAMPUS_CAPABILITY_EVENT_SCHEMA.into(),
            event_id: String::new(),
            kind,
            occurred_at: now,
            call_path: request.caller.call_path(),
            caller,
            session_id: recorded_identifier(&request.claims.session_id),
            actor,
            session_owner: authority.map(|authority| authority.owner.clone()),
            course_id: authority.map(|authority| authority.course_id.clone()),
            outcome: decision.outcome,
            reason: decision.reason,
            policy_reason: policy.map(|record| record.reason),
            authority: decision.decided_by,
            risk: policy.and_then(|record| record.risk),
            approval_kind: policy.and_then(|record| record.approval_kind),
            approved_by: policy.and_then(|record| record.approved_by),
            approver: decision.approver.clone(),
            effect: decision.effect,
            capability: CampusCapabilityRef::new(&request.capability, decision.binding),
            resource: decision.resource.clone(),
            arguments: CampusRedactedArguments::of(&request.action),
            profile_hash: authority.map(|authority| authority.policy.profile_hash().into()),
            effective_policy_hash: authority
                .map(|authority| authority.policy.effective_policy_hash().into()),
            result,
        };
        let value = serde_json::to_value(&event).expect("campus capability event serializes");
        let hash = digest(&value);
        event.event_id = format!("ccap_{}", &hash.trim_start_matches("sha256:")[..32]);
        event
    }
}

/// The digest an approval must carry to be bound to `action`.
pub fn arguments_sha256(action: &AgentAction) -> String {
    digest(&action_value(action))
}

fn action_value(action: &AgentAction) -> Value {
    serde_json::to_value(action).expect("agent actions serialize")
}

fn digest(value: &Value) -> String {
    canonical_json_sha256(value).expect("canonical JSON serializes")
}

fn shape(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => json!({ "bytes": text.len() }),
        Value::Array(items) => json!({ "items": items.len() }),
        Value::Object(entries) => json!({ "entries": entries.len() }),
    }
}

/// Identifiers from a request are recorded verbatim only when they are short
/// and plain; anything else is recorded as its digest.
fn recorded_identifier(value: &str) -> String {
    let plain = (1..=MAX_RECORDED_IDENTIFIER_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if plain {
        value.into()
    } else {
        digest(&json!(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("campus capability audit is unavailable: {0}")]
pub struct CampusAuditError(pub String);

/// Durable destination for Campus capability events. A failed write must be
/// reported, never dropped: the broker refuses to run an effect whose
/// decision was not recorded.
#[async_trait]
pub trait CampusCapabilityAuditSink: Send + Sync {
    async fn record(&self, event: &CampusCapabilityEvent) -> Result<(), CampusAuditError>;
}
