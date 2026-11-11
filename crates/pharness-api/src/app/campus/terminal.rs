//! Campus browser terminals: configuration, ticket issuance and whether a
//! lab session admits a terminal.
//!
//! A browser obtains a terminal in two steps. A CSRF-protected `POST
//! .../terminal-ticket` authorizes the caller against the lab journal and
//! returns a short-lived random ticket, kept by
//! [`super::terminal_registry`]. The browser then opens the WebSocket (see
//! [`super::terminal_socket`]) and presents the ticket as its first message.

use super::config::CampusConfig;
use super::error::CampusError;
use super::lab_handlers::{authorized_session, parse_body, EmptyRequest, LabAccess};
use super::session::CampusPrincipal;
use super::state::CampusState;
use super::terminal_registry::{CampusTerminalLimits, CampusTerminals, TerminalGrant};
use super::tokens::random_token;
use anyhow::{bail, Context};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pharness_store::CampusLabSessionRecord;
use pharness_workspaces::campus_lab_session::kubernetes::KubectlCampusKubeApi;
use pharness_workspaces::campus_terminal::{
    CampusTerminalAttacher, FakeCampusTerminal, KubectlCampusTerminal, KubectlCampusTerminalConfig,
};
use pharness_workspaces::{CampusLabDesiredState, CampusWorkloadRef};
use serde_json::json;
use std::sync::Arc;

pub(crate) const ENV_TERMINAL: &str = "PHARNESS_CAMPUS_TERMINAL";
pub(crate) const ENV_TERMINAL_CONFIG_FILE: &str = "PHARNESS_CAMPUS_TERMINAL_CONFIG_FILE";
/// The WebSocket subprotocol; also names the message format.
pub(crate) const TERMINAL_PROTOCOL: &str = "pharness.campus.terminal.v1";

/// Why a lab session no longer admits a terminal, as reported to the
/// browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalDenial {
    NotReady,
    Stopped,
    Expired,
    Superseded,
    GenerationChanged,
    PolicyChanged,
}

impl TerminalDenial {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NotReady => "session_not_ready",
            Self::Stopped => "session_stopped",
            Self::Expired => "session_expired",
            Self::Superseded => "session_superseded",
            Self::GenerationChanged => "generation_changed",
            Self::PolicyChanged => "policy_changed",
        }
    }
}

/// Whether `record` admits a terminal now. With `grant`, also whether it is
/// still the exact session, generation and workload the grant was for.
pub(crate) fn terminal_admissible(
    record: &CampusLabSessionRecord,
    grant: Option<&TerminalGrant>,
    now: i64,
) -> Result<CampusWorkloadRef, TerminalDenial> {
    let session = &record.session;
    // The reconciler stops an expired session soon after its expiry, so
    // expiry is reported as such unless the session was stopped, revoked
    // or quarantined before it ran out.
    let ended_by_expiry = session.expires_at <= now
        && session.quarantined_at.is_none()
        && !matches!(session.revoked_at, Some(revoked) if revoked < session.expires_at);
    if ended_by_expiry {
        return Err(TerminalDenial::Expired);
    }
    if session.desired_state == CampusLabDesiredState::Terminated
        || session.revoked_at.is_some()
        || session.quarantined_at.is_some()
        || session.state.is_terminal()
    {
        return Err(TerminalDenial::Stopped);
    }
    if !record.is_current() || record.lab.reset_requested_at.is_some() {
        return Err(TerminalDenial::Superseded);
    }
    if !session.state.grants_access() {
        return Err(TerminalDenial::NotReady);
    }
    let workload = session.workload().ok_or(TerminalDenial::NotReady)?;
    if let Some(grant) = grant {
        if grant.lab_session_id != session.session_id
            || grant.lab_generation != record.generation
            || grant.workload != workload
        {
            return Err(TerminalDenial::GenerationChanged);
        }
        if grant.effective_policy_hash != record.effective_policy_hash {
            return Err(TerminalDenial::PolicyChanged);
        }
    }
    Ok(workload)
}

pub(crate) fn terminals(state: &CampusState) -> Result<&Arc<CampusTerminals>, CampusError> {
    state.runtime.terminals.as_ref().ok_or_else(|| {
        CampusError::not_found("campus terminals are not available")
            .with_code("terminal_unavailable")
    })
}

fn denial_error(denial: TerminalDenial) -> CampusError {
    let message = match denial {
        TerminalDenial::NotReady => "the lab session is not ready for a terminal",
        TerminalDenial::Stopped => "the lab session is stopped or stopping",
        TerminalDenial::Expired => "the lab session has expired",
        TerminalDenial::Superseded | TerminalDenial::GenerationChanged => {
            "this lab session was replaced"
        }
        TerminalDenial::PolicyChanged => "the lab policy changed; start a new session",
    };
    CampusError::conflict(message).with_code(denial.code())
}

/// The process limit of the session's effective policy, provided the
/// catalog still holds the exact policy the session was admitted under.
pub(crate) fn session_pids(
    state: &CampusState,
    record: &CampusLabSessionRecord,
) -> Result<u32, TerminalDenial> {
    let labs = state
        .runtime
        .labs
        .as_ref()
        .ok_or(TerminalDenial::NotReady)?;
    let assignment = labs
        .catalog
        .assignment(&record.session.course_id, &record.lab.assignment_id)
        .ok_or(TerminalDenial::PolicyChanged)?;
    if assignment.policy.effective_policy_hash() != record.effective_policy_hash {
        return Err(TerminalDenial::PolicyChanged);
    }
    match assignment.policy.constraints().pids {
        0 => Err(TerminalDenial::PolicyChanged),
        pids => Ok(pids),
    }
}

/// `POST /api/campus/lab-sessions/:session_id/terminal-ticket`
///
/// Only the session's owner may open its terminal; course staff and
/// everyone else get `404`. The session must be ready, current, not stopping
/// and unexpired, and the owner must still be allowed to launch labs in the
/// course.
pub(crate) async fn issue_ticket(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Result<Response, CampusError> {
    let EmptyRequest {} = parse_body(&body)?;
    let terminals = terminals(&state)?;
    let record = authorized_session(&state, &principal, &session_id, LabAccess::Terminal).await?;
    super::handlers::require_course_action(
        &state,
        &principal,
        &record.session.course_id,
        super::authz::CampusAction::LaunchLab,
    )
    .await?;
    let now = state.runtime.clock.now();
    let workload = terminal_admissible(&record, None, now).map_err(denial_error)?;
    let pids = session_pids(&state, &record).map_err(denial_error)?;
    let grant = TerminalGrant {
        identity_id: principal.identity_id.clone(),
        browser_session_hash: principal.session_id_hash.clone(),
        lab_session_id: session_id.clone(),
        course_id: record.session.course_id.clone(),
        lab_generation: record.generation,
        workload,
        effective_policy_hash: record.effective_policy_hash.clone(),
        pids,
    };
    let ticket = random_token(&state.runtime.rng).map_err(|_| {
        CampusError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "a terminal ticket could not be issued",
        )
    })?;
    let expires_at = terminals.issue(&ticket, grant, now).ok_or_else(|| {
        CampusError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many terminal tickets are outstanding; try again shortly",
        )
        .with_code("too_many_tickets")
    })?;
    tracing::info!(
        identity_id = %principal.identity_id,
        session_id = %session_id,
        "campus terminal ticket issued"
    );
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "ticket": ticket,
            "expires_at": expires_at,
            "socket_path": format!("/api/campus/lab-sessions/{session_id}/terminal"),
            "protocol": TERMINAL_PROTOCOL,
        })),
    )
        .into_response())
}

/// How terminals are attached, from `PHARNESS_CAMPUS_TERMINAL`:
/// unset or `none` disables them; `fake` (loopback development only) runs
/// the in-process fake; `kubectl` execs into session Pods with the
/// identity and slot map in `PHARNESS_CAMPUS_TERMINAL_CONFIG_FILE`.
pub(crate) fn load_campus_terminals_from_env(
    config: &CampusConfig,
    has_labs: bool,
) -> anyhow::Result<Option<CampusTerminals>> {
    load_campus_terminals(
        std::env::var(ENV_TERMINAL).ok().as_deref(),
        std::env::var(ENV_TERMINAL_CONFIG_FILE).ok().as_deref(),
        config,
        has_labs,
    )
}

pub(crate) fn load_campus_terminals(
    kind: Option<&str>,
    config_file: Option<&str>,
    config: &CampusConfig,
    has_labs: bool,
) -> anyhow::Result<Option<CampusTerminals>> {
    let attacher: Arc<dyn CampusTerminalAttacher> = match kind.map(str::trim) {
        None | Some("" | "none") => return Ok(None),
        Some("fake") if config.allow_insecure_loopback_http => Arc::new(FakeCampusTerminal::new()),
        Some("fake") => bail!(
            "{ENV_TERMINAL}=fake is only accepted for local development with PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP=true"
        ),
        Some("kubectl") => {
            let path = config_file
                .with_context(|| format!("{ENV_TERMINAL}=kubectl needs {ENV_TERMINAL_CONFIG_FILE}"))?;
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {ENV_TERMINAL_CONFIG_FILE} {path}"))?;
            let terminal_config: KubectlCampusTerminalConfig = serde_json::from_str(&text)
                .with_context(|| format!("{ENV_TERMINAL_CONFIG_FILE} is not a valid terminal configuration"))?;
            let api = KubectlCampusKubeApi::new(
                terminal_config.kubectl.clone(),
                terminal_config.global_args.clone(),
            );
            Arc::new(KubectlCampusTerminal::new(terminal_config, Arc::new(api))?)
        }
        Some(_) => bail!("{ENV_TERMINAL} must be one of none, fake or kubectl"),
    };
    if !has_labs {
        bail!("{ENV_TERMINAL} needs a Campus lab catalog (PHARNESS_CAMPUS_LABS_FILE)");
    }
    Ok(Some(CampusTerminals::new(
        attacher,
        CampusTerminalLimits::default(),
    )))
}
