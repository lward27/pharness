//! Campus lab launch, status, stop and reset handlers.
//!
//! The owner, tenant, course role, profile, policy, lifetime and limits of a
//! lab session are derived on the server: the owner from the
//! [`CampusPrincipal`], everything else from the lab catalog. Request bodies
//! accept only an idempotency key and reject every other field, so a client
//! can never name an image, namespace, credential, profile or bound.
//!
//! A session is visible only to its owner and to course staff. Anyone else
//! gets the same `404` as for a session that does not exist.

use super::authz::{authorize, CampusAction, CampusActor, CampusDecision};
use super::error::CampusError;
use super::handlers::require_course_action;
use super::lab_catalog::is_lab_identifier;
use super::labs::{CampusLabs, LabLaunch};
use super::session::CampusPrincipal;
use super::state::CampusState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pharness_store::{
    CampusLabAdmission, CampusLabRejection, CampusLabSessionRecord, StoredCampusLabSession,
    CENTRAL_AUTHORITY_UNAVAILABLE,
};
use pharness_workspaces::{CampusLabDesiredState, CampusLabSessionState};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const IDEMPOTENCY_HEADER: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_CHARS: usize = 128;
const OWN_SESSION_LIST_LIMIT: i64 = 50;
const COURSE_SESSION_LIST_LIMIT: i64 = 200;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaunchRequest {
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyRequest {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabAccess {
    View,
    Stop,
    Reset,
    /// Owner only.
    Terminal,
}

fn labs(state: &CampusState) -> Result<&Arc<CampusLabs>, CampusError> {
    state.runtime.labs.as_ref().ok_or_else(|| {
        CampusError::unavailable("campus labs are not available").with_code("labs_unavailable")
    })
}

fn launchable(labs: &CampusLabs) -> Result<(), CampusError> {
    if labs.can_launch() {
        Ok(())
    } else {
        Err(
            CampusError::unavailable("campus labs cannot be launched right now")
                .with_code("labs_unavailable"),
        )
    }
}

fn session_not_found() -> CampusError {
    CampusError::not_found("campus lab session not found").with_code("session_not_found")
}

fn assignment_not_found() -> CampusError {
    CampusError::not_found("campus assignment not found").with_code("assignment_not_found")
}

/// An empty body is an empty request. Unknown fields are refused without
/// echoing them back.
pub(crate) fn parse_body<T: DeserializeOwned + Default>(body: &Bytes) -> Result<T, CampusError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(body).map_err(|_| {
        CampusError::bad_request("the request body is not a valid campus lab request")
            .with_code("invalid_request")
    })
}

fn idempotency_key(
    headers: &HeaderMap,
    from_body: Option<String>,
) -> Result<Option<String>, CampusError> {
    let invalid = || {
        CampusError::bad_request("idempotency key must be 1-128 characters of [A-Za-z0-9._:-]")
            .with_code("invalid_idempotency_key")
    };
    let from_header = headers
        .get(IDEMPOTENCY_HEADER)
        .map(|value| value.to_str().map(str::to_string).map_err(|_| invalid()))
        .transpose()?;
    let key = match (from_header, from_body) {
        (Some(header), Some(body)) if header != body => {
            return Err(
                CampusError::bad_request("the idempotency key header and body disagree")
                    .with_code("invalid_idempotency_key"),
            )
        }
        (header, body) => header.or(body),
    };
    if let Some(key) = &key {
        let valid = !key.is_empty()
            && key.len() <= MAX_IDEMPOTENCY_KEY_CHARS
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'));
        if !valid {
            return Err(invalid());
        }
    }
    Ok(key)
}

fn actor(principal: &CampusPrincipal) -> String {
    format!("campus-identity:{}", principal.identity_id)
}

/// The coarse lifecycle a student sees.
fn public_status(state: CampusLabSessionState) -> &'static str {
    use CampusLabSessionState as S;
    match state {
        S::Requested | S::Authorizing | S::Provisioning => "provisioning",
        S::Ready => "ready",
        S::Stopping | S::Cleaning => "stopping",
        S::CleanupBlocked => "cleanup_blocked",
        S::Terminated => "stopped",
        S::Failed => "failed",
    }
}

/// A fixed explanation for a failed or blocked session. Provider and journal
/// reasons stay in the journal and the logs.
fn safe_error(session: &StoredCampusLabSession) -> Option<Value> {
    let (code, message) = if session.state == CampusLabSessionState::CleanupBlocked {
        (
            "cleanup_blocked",
            "the lab could not be cleaned up yet; an operator will resolve it",
        )
    } else {
        let reason = session.failure_reason.as_deref()?;
        if reason == CENTRAL_AUTHORITY_UNAVAILABLE {
            (
                "launch_denied",
                "the lab could not be admitted; try again later",
            )
        } else if reason.starts_with("provision rejected") {
            ("provision_failed", "the lab could not be started")
        } else if reason.starts_with("workspace lost") {
            ("workspace_lost", "the lab workspace stopped unexpectedly")
        } else {
            ("lab_failed", "the lab failed")
        }
    };
    Some(json!({ "code": code, "message": message }))
}

fn session_view(record: &CampusLabSessionRecord) -> Value {
    let session = &record.session;
    let current = record.is_current();
    json!({
        "session_id": session.session_id,
        "course_id": session.course_id,
        "assignment_id": record.lab.assignment_id,
        "lab_id": record.lab.lab_id,
        "generation": record.generation,
        "status": public_status(session.state),
        "state": session.state.as_str(),
        "stop_requested": session.desired_state == CampusLabDesiredState::Terminated,
        "current": current,
        "reset_pending": current && record.lab.reset_requested_at.is_some(),
        "reset_from_session_id": record.reset_from_session_id,
        "profile": { "id": session.profile_id, "version": session.profile_version },
        "error": safe_error(session),
        "created_at": session.created_at,
        "updated_at": session.updated_at,
        "expires_at": session.expires_at,
        "terminated_at": session.terminated_at,
    })
}

fn rejection_error(rejection: CampusLabRejection) -> CampusError {
    match rejection {
        CampusLabRejection::NoSlotsAvailable => {
            CampusError::unavailable("no lab slots are available; try again later")
                .with_code("no_slots_available")
        }
        CampusLabRejection::CourseCapacityReached => {
            CampusError::unavailable("no lab slots are available for this course; try again later")
                .with_code("course_capacity_reached")
        }
        CampusLabRejection::IdentitySessionLimit { active_session_id } => CampusError::conflict(
            "you already have an active lab session; stop it before launching another",
        )
        .with_code("active_session_exists")
        .with_session_id(active_session_id),
        CampusLabRejection::IdempotencyKeyReused => {
            CampusError::conflict("this idempotency key was already used for another assignment")
                .with_code("idempotency_key_reused")
        }
        CampusLabRejection::ResetSuperseded { current_session_id } => {
            let error = CampusError::conflict("this lab session was already replaced")
                .with_code("reset_superseded");
            match current_session_id {
                Some(current) => error.with_session_id(current),
                None => error,
            }
        }
        CampusLabRejection::TeardownPending => {
            CampusError::conflict("the lab session is still being torn down")
                .with_code("teardown_pending")
        }
    }
}

async fn is_course_staff(
    state: &CampusState,
    principal: &CampusPrincipal,
    course_id: &str,
) -> Result<bool, CampusError> {
    let course_role = state
        .store
        .get_campus_course_role(&principal.key, course_id)
        .await?;
    let actor = CampusActor {
        tenant_id: &principal.key.tenant_id,
        is_tenant_admin: principal.is_tenant_admin,
        course_role,
    };
    Ok(authorize(
        &actor,
        &state.runtime.config.tenant_id,
        CampusAction::ManageLabs,
    ) == CampusDecision::Allow)
}

fn is_owner(record: &CampusLabSessionRecord, principal: &CampusPrincipal) -> bool {
    record.owner_identity_id == principal.identity_id
        && record.lab.tenant_id == principal.key.tenant_id
        && record.lab.issuer == principal.key.issuer
        && record.lab.subject == principal.key.subject
}

/// Load a session the caller may act on. Anything else, including a session
/// of another tenant, student or course, is reported as not found.
pub(crate) async fn authorized_session(
    state: &CampusState,
    principal: &CampusPrincipal,
    session_id: &str,
    access: LabAccess,
) -> Result<CampusLabSessionRecord, CampusError> {
    if !is_lab_identifier(session_id) {
        return Err(session_not_found());
    }
    let record = state
        .store
        .get_campus_lab_session_record(session_id)
        .await?
        .ok_or_else(session_not_found)?;
    if record.session.tenant_id != principal.key.tenant_id
        || record.session.tenant_id != state.runtime.config.tenant_id
    {
        return Err(session_not_found());
    }
    let allowed = is_owner(&record, principal)
        || (matches!(access, LabAccess::View | LabAccess::Stop)
            && is_course_staff(state, principal, &record.session.course_id).await?);
    if allowed {
        Ok(record)
    } else {
        Err(session_not_found())
    }
}

fn admission_response(
    labs: &CampusLabs,
    admission: CampusLabAdmission,
    field: &str,
) -> Result<Response, CampusError> {
    let (status, outcome, record) = match admission {
        CampusLabAdmission::Created(record) => {
            labs.wake();
            (StatusCode::CREATED, "created", record)
        }
        CampusLabAdmission::Active(record) => (StatusCode::OK, "existing", record),
        CampusLabAdmission::Replayed(record) => (StatusCode::OK, "replayed", record),
        CampusLabAdmission::Rejected(rejection) => return Err(rejection_error(rejection)),
    };
    Ok((
        status,
        Json(json!({ field: outcome, "session": session_view(&record) })),
    )
        .into_response())
}

pub(crate) async fn list_assignments(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(course_id): Path<String>,
) -> Result<Response, CampusError> {
    let role =
        require_course_action(&state, &principal, &course_id, CampusAction::ViewCourse).await?;
    let labs = labs(&state)?;
    let mut assignments = Vec::new();
    for assignment in labs.catalog.course_assignments(&course_id) {
        let mut session = Value::Null;
        if role.is_some() {
            let lab = state
                .store
                .get_campus_lab(
                    &principal.key.tenant_id,
                    &course_id,
                    &assignment.assignment_id,
                    &principal.key.issuer,
                    &principal.key.subject,
                )
                .await?;
            if let Some(current) = lab.and_then(|lab| lab.current_session_id) {
                if let Some(record) = state.store.get_campus_lab_session_record(&current).await? {
                    session = session_view(&record);
                }
            }
        }
        let profile = assignment.policy.profile();
        assignments.push(json!({
            "assignment_id": assignment.assignment_id,
            "title": assignment.title,
            "profile": {
                "id": profile.id,
                "version": profile.version,
                "kind": assignment.profile_kind,
            },
            "session_seconds": assignment.session_seconds,
            "launchable": role.is_some() && labs.can_launch(),
            "session": session,
        }));
    }
    Ok(Json(json!({ "course_id": course_id, "assignments": assignments })).into_response())
}

pub(crate) async fn launch(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path((course_id, assignment_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, CampusError> {
    let request: LaunchRequest = parse_body(&body)?;
    let key = idempotency_key(&headers, request.idempotency_key)?;
    require_course_action(&state, &principal, &course_id, CampusAction::LaunchLab).await?;
    if !is_lab_identifier(&assignment_id) {
        return Err(assignment_not_found());
    }
    let labs = labs(&state)?;
    let assignment = labs
        .catalog
        .assignment(&course_id, &assignment_id)
        .ok_or_else(assignment_not_found)?;
    launchable(labs)?;
    let now = state.runtime.clock.now();
    let admission = state
        .store
        .admit_campus_lab_launch(labs.admission(LabLaunch {
            owner: &principal.key,
            assignment,
            idempotency_key: key,
            reset_from_session_id: None,
            actor: actor(&principal),
            now,
        }))
        .await?;
    if let CampusLabAdmission::Created(record) = &admission {
        tracing::info!(
            identity_id = %principal.identity_id,
            session_id = %record.session.session_id,
            course_id = %course_id,
            assignment_id = %assignment_id,
            "campus lab session admitted"
        );
    }
    admission_response(labs, admission, "admission")
}

pub(crate) async fn list_my_sessions(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
) -> Result<Response, CampusError> {
    let records = state
        .store
        .list_campus_lab_session_records_for_identity(
            &principal.key.tenant_id,
            &principal.key.issuer,
            &principal.key.subject,
            OWN_SESSION_LIST_LIMIT,
        )
        .await?;
    let sessions = records.iter().map(session_view).collect::<Vec<_>>();
    Ok(Json(json!({ "sessions": sessions })).into_response())
}

pub(crate) async fn list_course_sessions(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(course_id): Path<String>,
) -> Result<Response, CampusError> {
    require_course_action(&state, &principal, &course_id, CampusAction::ManageLabs).await?;
    let records = state
        .store
        .list_live_campus_lab_session_records_for_course(
            &principal.key.tenant_id,
            &course_id,
            COURSE_SESSION_LIST_LIMIT,
        )
        .await?;
    let sessions = records
        .iter()
        .map(|record| {
            let mut view = session_view(record);
            view["owner"] = json!({ "issuer": record.lab.issuer, "subject": record.lab.subject });
            view
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "course_id": course_id, "sessions": sessions })).into_response())
}

pub(crate) async fn get_session(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(session_id): Path<String>,
) -> Result<Response, CampusError> {
    let record = authorized_session(&state, &principal, &session_id, LabAccess::View).await?;
    Ok(Json(json!({ "session": session_view(&record) })).into_response())
}

pub(crate) async fn stop_session(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Result<Response, CampusError> {
    let EmptyRequest {} = parse_body(&body)?;
    let record = authorized_session(&state, &principal, &session_id, LabAccess::Stop).await?;
    let reason = if is_owner(&record, &principal) {
        "stopped by the student"
    } else {
        "stopped by course staff"
    };
    let stopped = state
        .store
        .request_campus_lab_stop(
            &session_id,
            &actor(&principal),
            reason,
            false,
            state.runtime.clock.now(),
        )
        .await?;
    state.runtime.revalidate_terminals();
    let status = if stopped.session.state.is_terminal() {
        StatusCode::OK
    } else {
        if let Some(labs) = &state.runtime.labs {
            labs.wake();
        }
        StatusCode::ACCEPTED
    };
    tracing::info!(
        identity_id = %principal.identity_id,
        session_id = %session_id,
        "campus lab session stop requested"
    );
    Ok((status, Json(json!({ "session": session_view(&stopped) }))).into_response())
}

/// Replace the caller's lab session with a fresh generation of the same lab.
///
/// A live session is first torn down; the reconciler admits the next
/// generation once teardown is confirmed and answers `202` until then. A
/// session that is already torn down is replaced at once. Every generation
/// keeps the lab's budget identity.
pub(crate) async fn reset_session(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Result<Response, CampusError> {
    let EmptyRequest {} = parse_body(&body)?;
    let record = authorized_session(&state, &principal, &session_id, LabAccess::Reset).await?;
    require_course_action(
        &state,
        &principal,
        &record.session.course_id,
        CampusAction::LaunchLab,
    )
    .await?;
    let labs = labs(&state)?;
    let assignment = labs
        .catalog
        .assignment(&record.session.course_id, &record.lab.assignment_id)
        .ok_or_else(assignment_not_found)?;
    launchable(labs)?;
    let now = state.runtime.clock.now();
    if record.is_current() && !record.session.state.is_terminal() {
        let pending = state
            .store
            .request_campus_lab_stop(
                &session_id,
                &actor(&principal),
                "reset requested by the student",
                true,
                now,
            )
            .await?;
        labs.wake();
        state.runtime.revalidate_terminals();
        tracing::info!(
            identity_id = %principal.identity_id,
            session_id = %session_id,
            "campus lab reset requested"
        );
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "reset": "pending", "session": session_view(&pending) })),
        )
            .into_response());
    }
    let admission = state
        .store
        .admit_campus_lab_launch(labs.admission(LabLaunch {
            owner: &principal.key,
            assignment,
            idempotency_key: None,
            reset_from_session_id: Some(session_id.clone()),
            actor: actor(&principal),
            now,
        }))
        .await?;
    admission_response(labs, admission, "reset")
}
