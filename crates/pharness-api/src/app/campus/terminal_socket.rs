//! The Campus terminal WebSocket, `GET /api/campus/lab-sessions/:id/terminal`.
//!
//! The handshake must carry the Campus session cookie (checked by the
//! session middleware) and the Campus `Origin`; the caller must be the lab
//! session's owner. After the upgrade the first message must be
//! `{"type":"auth","ticket":...,"cols":...,"rows":...}` with a ticket from
//! [`super::terminal::issue_ticket`] for this lab session and this browser
//! session. The ticket travels inside the socket, never in the URL, so it
//! cannot end up in proxy or access logs.
//!
//! Then binary frames carry terminal bytes in both directions, the browser
//! sends `{"type":"resize","cols":...,"rows":...}` as text, and the server
//! reports `{"type":"ready",...}` and `{"type":"closed","reason":...}`.
//!
//! An open terminal re-checks the browser session and the lab journal every
//! [`CampusTerminalLimits::recheck_interval`] and immediately when a
//! handler signals [`CampusTerminals::revalidate`]. Logout, stop, reset,
//! expiry, a new generation or a lost course membership close it. Closing
//! drops the attacher's stream, which ends the terminal process.

use super::authz::{authorize, CampusAction, CampusActor, CampusDecision};
use super::error::CampusError;
use super::lab_catalog::is_lab_identifier;
use super::lab_handlers::{authorized_session, LabAccess};
use super::session::CampusPrincipal;
use super::state::CampusState;
use super::terminal::{
    session_pids, terminal_admissible, terminals, TerminalDenial, TERMINAL_PROTOCOL,
};
use super::terminal_registry::{
    CampusTerminalLimits, CampusTerminals, TerminalGrant, TicketRefusal,
};
use super::tokens::{is_well_formed_token, secrets_match};
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use pharness_workspaces::campus_terminal::{
    CampusTerminalError, CampusTerminalInput, CampusTerminalSize, CampusTerminalStream,
    CampusTerminalTarget,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval_at, sleep_until, timeout, Instant};

/// How much output may be sent at once before the rate limit applies.
const OUTPUT_BURST: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ClientMessage {
    Auth {
        ticket: String,
        cols: u16,
        rows: u16,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
}

/// Why a terminal closed, with the WebSocket close code the browser sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseReason {
    Exited,
    ClientClosed,
    TicketRejected,
    AuthTimeout,
    InvalidMessage,
    TooManyTerminals,
    Unavailable,
    WorkloadNotRunning,
    Denied(TerminalDenial),
    LoggedOut,
    AccessRevoked,
    IdleTimeout,
    ClientTooSlow,
    Internal,
}

impl CloseReason {
    pub(crate) fn code(self) -> u16 {
        match self {
            Self::Exited | Self::ClientClosed => 1000,
            Self::Internal => 1011,
            Self::TicketRejected => 4001,
            Self::AuthTimeout => 4002,
            Self::InvalidMessage => 4003,
            Self::TooManyTerminals => 4004,
            Self::Unavailable => 4005,
            Self::WorkloadNotRunning => 4006,
            Self::Denied(_) => 4010,
            Self::LoggedOut => 4011,
            Self::AccessRevoked => 4012,
            Self::IdleTimeout => 4013,
            Self::ClientTooSlow => 4014,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Exited => "exited",
            Self::ClientClosed => "client_closed",
            Self::TicketRejected => "ticket_rejected",
            Self::AuthTimeout => "auth_timeout",
            Self::InvalidMessage => "invalid_message",
            Self::TooManyTerminals => "too_many_terminals",
            Self::Unavailable => "terminal_unavailable",
            Self::WorkloadNotRunning => "workload_not_running",
            Self::Denied(denial) => denial.code(),
            Self::LoggedOut => "logged_out",
            Self::AccessRevoked => "access_revoked",
            Self::IdleTimeout => "idle_timeout",
            Self::ClientTooSlow => "client_too_slow",
            Self::Internal => "internal_error",
        }
    }
}

fn origin_matches(headers: &HeaderMap, public_origin: &str) -> bool {
    // Browsers always send Origin on a WebSocket handshake, and the
    // same-origin policy does not apply to WebSockets, so a missing or
    // foreign Origin is refused rather than tolerated.
    let origin_ok = headers
        .get(header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| secrets_match(origin, public_origin));
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|site| site.as_bytes().eq_ignore_ascii_case(b"cross-site"));
    origin_ok && !cross_site
}

pub(crate) async fn terminal_socket(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, CampusError> {
    let terminals = terminals(&state)?.clone();
    if !origin_matches(&headers, &state.runtime.config.public_origin) {
        return Err(
            CampusError::forbidden("request origin does not match the campus origin")
                .with_code("origin_mismatch"),
        );
    }
    if !is_lab_identifier(&session_id) {
        return Err(
            CampusError::not_found("campus lab session not found").with_code("session_not_found")
        );
    }
    authorized_session(&state, &principal, &session_id, LabAccess::Terminal).await?;
    let upgrade = upgrade.map_err(|_| {
        CampusError::bad_request("a WebSocket upgrade is required").with_code("websocket_required")
    })?;
    let limits = terminals.limits.clone();
    Ok(upgrade
        .protocols([TERMINAL_PROTOCOL])
        .max_message_size(limits.max_message_bytes)
        .max_frame_size(limits.max_message_bytes)
        .on_upgrade(move |socket| async move {
            let connection = Connection {
                state,
                terminals,
                principal,
                session_id,
                connection_id: format!("ctc_{}", uuid::Uuid::now_v7().simple()),
            };
            connection.run(socket).await;
        })
        .into_response())
}

struct Connection {
    state: CampusState,
    terminals: Arc<CampusTerminals>,
    principal: CampusPrincipal,
    session_id: String,
    connection_id: String,
}

#[derive(Default)]
struct Counters {
    bytes_in: u64,
    bytes_out: u64,
}

impl Connection {
    fn limits(&self) -> &CampusTerminalLimits {
        &self.terminals.limits
    }

    fn actor(&self) -> String {
        format!("campus-identity:{}", self.principal.identity_id)
    }

    async fn audit(&self, kind: &str, payload: Value) -> bool {
        match self
            .state
            .store
            .record_campus_terminal_event(
                &self.session_id,
                kind,
                &self.actor(),
                &payload,
                self.state.runtime.clock.now(),
            )
            .await
        {
            Ok(_) => true,
            Err(error) => {
                tracing::error!(%error, session_id = %self.session_id, kind, "campus terminal audit failed");
                false
            }
        }
    }

    async fn refuse(&self, socket: &mut WebSocket, reason: CloseReason, detail: &str) {
        tracing::warn!(
            identity_id = %self.principal.identity_id,
            session_id = %self.session_id,
            connection_id = %self.connection_id,
            reason = reason.as_str(),
            detail,
            "campus terminal refused"
        );
        self.audit(
            "campus_terminal.refused",
            json!({
                "connection_id": self.connection_id,
                "reason": reason.as_str(),
                "detail": detail,
            }),
        )
        .await;
        self.close(socket, reason).await;
    }

    async fn close(&self, socket: &mut WebSocket, reason: CloseReason) {
        let send_timeout = self.limits().send_timeout;
        let status = json!({ "type": "closed", "reason": reason.as_str() }).to_string();
        let _ = timeout(send_timeout, socket.send(Message::Text(status))).await;
        let frame = CloseFrame {
            code: reason.code(),
            reason: reason.as_str().into(),
        };
        let _ = timeout(send_timeout, socket.send(Message::Close(Some(frame)))).await;
    }

    async fn run(self, mut socket: WebSocket) {
        let (ticket, size) = match self.authenticate(&mut socket).await {
            Ok(first) => first,
            Err(reason) => {
                if reason != CloseReason::ClientClosed {
                    self.refuse(&mut socket, reason, "").await;
                }
                return;
            }
        };
        let now = self.state.runtime.clock.now();
        let grant = match self.terminals.redeem(
            &ticket,
            &self.principal.identity_id,
            &self.principal.session_id_hash,
            &self.session_id,
            now,
        ) {
            Ok(grant) => grant,
            Err(refusal) => {
                let detail = match refusal {
                    TicketRefusal::Unknown => "unknown_or_used",
                    TicketRefusal::Expired => "expired",
                    TicketRefusal::WrongSession => "wrong_session",
                };
                self.refuse(&mut socket, CloseReason::TicketRejected, detail)
                    .await;
                return;
            }
        };
        if let Err(reason) = self.recheck(&grant).await {
            self.refuse(&mut socket, reason, "").await;
            return;
        }
        let Some(_place) = self.terminals.open(&self.session_id) else {
            self.refuse(&mut socket, CloseReason::TooManyTerminals, "")
                .await;
            return;
        };
        let target = CampusTerminalTarget {
            workload: grant.workload.clone(),
            pids: grant.pids,
            size,
        };
        let stream = match self.terminals.attacher().attach(&target).await {
            Ok(stream) => stream,
            Err(error) => {
                let reason = match &error {
                    CampusTerminalError::NotRunning(_) => CloseReason::WorkloadNotRunning,
                    CampusTerminalError::Refused(_) | CampusTerminalError::Unavailable(_) => {
                        CloseReason::Unavailable
                    }
                };
                tracing::warn!(%error, session_id = %self.session_id, "campus terminal attach failed");
                self.refuse(&mut socket, reason, "").await;
                return;
            }
        };
        let opened = self
            .audit(
                "campus_terminal.opened",
                json!({
                    "connection_id": self.connection_id,
                    "lab_generation": grant.lab_generation,
                    "cols": size.cols,
                    "rows": size.rows,
                }),
            )
            .await;
        if !opened {
            self.close(&mut socket, CloseReason::Internal).await;
            return;
        }
        tracing::info!(
            identity_id = %self.principal.identity_id,
            session_id = %self.session_id,
            connection_id = %self.connection_id,
            "campus terminal opened"
        );
        let ready = json!({
            "type": "ready",
            "connection_id": self.connection_id,
            "cols": size.cols,
            "rows": size.rows,
        })
        .to_string();
        let started = Instant::now();
        let mut counters = Counters::default();
        let reason = match timeout(
            self.limits().send_timeout,
            socket.send(Message::Text(ready)),
        )
        .await
        {
            Ok(Ok(())) => self.pump(&mut socket, stream, &grant, &mut counters).await,
            _ => CloseReason::ClientClosed,
        };
        if reason != CloseReason::ClientClosed {
            self.close(&mut socket, reason).await;
        }
        tracing::info!(
            identity_id = %self.principal.identity_id,
            session_id = %self.session_id,
            connection_id = %self.connection_id,
            reason = reason.as_str(),
            "campus terminal closed"
        );
        self.audit(
            "campus_terminal.closed",
            json!({
                "connection_id": self.connection_id,
                "reason": reason.as_str(),
                "bytes_in": counters.bytes_in,
                "bytes_out": counters.bytes_out,
                "duration_ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            }),
        )
        .await;
    }

    /// The first message: the ticket and the initial size.
    async fn authenticate(
        &self,
        socket: &mut WebSocket,
    ) -> Result<(String, CampusTerminalSize), CloseReason> {
        let first = timeout(self.limits().auth_timeout, socket.recv())
            .await
            .map_err(|_| CloseReason::AuthTimeout)?;
        let text = match first {
            Some(Ok(Message::Text(text))) => text,
            None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {
                return Err(CloseReason::ClientClosed)
            }
            Some(Ok(_)) => return Err(CloseReason::InvalidMessage),
        };
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Auth { ticket, cols, rows }) => {
                if !is_well_formed_token(&ticket) {
                    return Err(CloseReason::TicketRejected);
                }
                let size =
                    CampusTerminalSize::new(cols, rows).ok_or(CloseReason::InvalidMessage)?;
                Ok((ticket, size))
            }
            _ => Err(CloseReason::InvalidMessage),
        }
    }

    /// Whether the browser session, the lab session and the course role
    /// still allow this terminal.
    async fn recheck(&self, grant: &TerminalGrant) -> Result<(), CloseReason> {
        let state = &self.state;
        let config = &state.runtime.config;
        let now = state.runtime.clock.now();
        let internal = |error: &dyn std::fmt::Display| {
            tracing::error!(%error, session_id = %self.session_id, "campus terminal recheck failed");
            CloseReason::Internal
        };
        let active = state
            .store
            .get_active_campus_session(&grant.browser_session_hash, &config.tenant_id, now)
            .await
            .map_err(|error| internal(&error))?
            .ok_or(CloseReason::LoggedOut)?;
        if active.identity.issuer != config.issuer || active.identity.id != grant.identity_id {
            return Err(CloseReason::LoggedOut);
        }
        let record = state
            .store
            .get_campus_lab_session_record(&grant.lab_session_id)
            .await
            .map_err(|error| internal(&error))?
            .ok_or(CloseReason::Denied(TerminalDenial::Stopped))?;
        if record.owner_identity_id != grant.identity_id {
            return Err(CloseReason::AccessRevoked);
        }
        terminal_admissible(&record, Some(grant), now).map_err(CloseReason::Denied)?;
        if session_pids(state, &record).map_err(CloseReason::Denied)? != grant.pids {
            return Err(CloseReason::Denied(TerminalDenial::PolicyChanged));
        }
        let key = &self.principal.key;
        let course_role = state
            .store
            .get_campus_course_role(key, &grant.course_id)
            .await
            .map_err(|error| internal(&error))?;
        let is_tenant_admin = state
            .store
            .is_campus_tenant_admin(key)
            .await
            .map_err(|error| internal(&error))?;
        let actor = CampusActor {
            tenant_id: &key.tenant_id,
            is_tenant_admin,
            course_role,
        };
        if authorize(&actor, &config.tenant_id, CampusAction::LaunchLab) != CampusDecision::Allow {
            return Err(CloseReason::AccessRevoked);
        }
        Ok(())
    }

    async fn send(&self, socket: &mut WebSocket, message: Message) -> Result<(), CloseReason> {
        match timeout(self.limits().send_timeout, socket.send(message)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(CloseReason::ClientClosed),
            Err(_) => Err(CloseReason::ClientTooSlow),
        }
    }

    async fn pump(
        &self,
        socket: &mut WebSocket,
        mut stream: CampusTerminalStream,
        grant: &TerminalGrant,
        counters: &mut Counters,
    ) -> CloseReason {
        let limits = self.limits().clone();
        let mut revalidation = self.terminals.revalidation();
        let mut recheck = interval_at(
            Instant::now() + limits.recheck_interval,
            limits.recheck_interval,
        );
        let mut ping = interval_at(Instant::now() + limits.ping_interval, limits.ping_interval);
        let mut last_input = Instant::now();
        let mut output_ready_at = Instant::now();
        let rate = limits.output_bytes_per_second.max(1);
        loop {
            let throttled = Instant::now() < output_ready_at;
            tokio::select! {
                message = socket.recv() => {
                    let input = match message {
                        None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {
                            return CloseReason::ClientClosed
                        }
                        Some(Ok(Message::Binary(bytes))) => {
                            counters.bytes_in = counters.bytes_in.saturating_add(bytes.len() as u64);
                            CampusTerminalInput::Data(bytes)
                        }
                        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                            Ok(ClientMessage::Resize { cols, rows }) => {
                                match CampusTerminalSize::new(cols, rows) {
                                    Some(size) => CampusTerminalInput::Resize(size),
                                    None => return CloseReason::InvalidMessage,
                                }
                            }
                            _ => return CloseReason::InvalidMessage,
                        },
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    };
                    last_input = Instant::now();
                    match timeout(limits.send_timeout, stream.input.send(input)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => return CloseReason::Exited,
                        Err(_) => return CloseReason::Unavailable,
                    }
                }
                output = stream.output.recv(), if !throttled => {
                    let Some(bytes) = output else {
                        return CloseReason::Exited;
                    };
                    let length = bytes.len() as u64;
                    if let Err(reason) = self.send(socket, Message::Binary(bytes)).await {
                        return reason;
                    }
                    counters.bytes_out = counters.bytes_out.saturating_add(length);
                    let now = Instant::now();
                    let floor = now.checked_sub(OUTPUT_BURST).unwrap_or(now);
                    output_ready_at = output_ready_at.max(floor)
                        + Duration::from_secs_f64(length as f64 / rate as f64);
                }
                _ = sleep_until(output_ready_at), if throttled => {}
                changed = revalidation.changed() => {
                    if changed.is_err() {
                        return CloseReason::Unavailable;
                    }
                    if let Err(reason) = self.recheck(grant).await {
                        return reason;
                    }
                }
                _ = recheck.tick() => {
                    if let Err(reason) = self.recheck(grant).await {
                        return reason;
                    }
                }
                _ = ping.tick() => {
                    if let Err(reason) = self.send(socket, Message::Ping(Vec::new())).await {
                        return reason;
                    }
                }
                _ = sleep_until(last_input + limits.idle_timeout) => {
                    return CloseReason::IdleTimeout;
                }
            }
        }
    }
}
