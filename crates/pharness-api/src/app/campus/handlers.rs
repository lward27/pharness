//! Campus login, session and course-membership handlers.
//!
//! Tenant and actor always come from [`CampusPrincipal`]. Request bodies use
//! `deny_unknown_fields`, so a body naming a tenant, actor or email is
//! rejected rather than trusted.

use super::authz::{authorize, CampusAction, CampusActor, CampusDecision};
use super::config::is_valid_subject;
use super::error::CampusError;
use super::login_cookie::LoginAttempt;
use super::oidc::{OidcHttpError, TokenExchange};
use super::session::{
    clear_cookie, read_cookie, session_token, set_cookie, CampusPrincipal, LOGIN_COOKIE,
    SESSION_COOKIE,
};
use super::state::CampusState;
use super::tokens::{
    csrf_token_for_session, is_well_formed_token, pkce_challenge, random_token, secrets_match,
    sha256_hex,
};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pharness_store::{
    CampusCourseRole, CampusIdentityKey, ConsumeCampusLoginState, CreateCampusSession,
    PutCampusMembership, UpsertCampusIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) const LOGIN_ATTEMPT_TTL_SECONDS: i64 = 10 * 60;
const MAX_CODE_CHARS: usize = 2048;
const MAX_COURSE_ID_CHARS: usize = 128;

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn redirect(location: &str) -> Result<Response, CampusError> {
    let location = HeaderValue::from_str(location)
        .map_err(|_| CampusError::unavailable("campus login is misconfigured"))?;
    Ok(no_store(
        (StatusCode::FOUND, [(header::LOCATION, location)]).into_response(),
    ))
}

fn random(state: &CampusState) -> Result<String, CampusError> {
    random_token(&state.runtime.rng)
        .map_err(|_| CampusError::unavailable("secure randomness is unavailable"))
}

/// Start a one-time authorization-code login with PKCE, state and nonce.
///
/// Nothing is written to storage: the attempt lives only in the sealed login
/// cookie, so unauthenticated callers cannot exhaust any server-side
/// capacity here.
pub(crate) async fn login(State(state): State<CampusState>) -> Result<Response, CampusError> {
    let runtime = &state.runtime;
    let config = &runtime.config;
    let now = runtime.clock.now();
    let attempt = LoginAttempt {
        state: random(&state)?,
        nonce: random(&state)?,
        pkce_verifier: random(&state)?,
        issued_at: now,
        expires_at: now + LOGIN_ATTEMPT_TTL_SECONDS,
    };
    let sealed = runtime
        .login_cookie_key
        .seal(&runtime.rng, &config.tenant_id, &config.issuer, &attempt)
        .map_err(|_| CampusError::unavailable("secure randomness is unavailable"))?;
    let (login_state, nonce, verifier) = (attempt.state, attempt.nonce, attempt.pkce_verifier);

    let mut location = config.authorization_endpoint.clone();
    location
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", config.redirect_uri.as_str())
        .append_pair("scope", &config.scopes)
        .append_pair("state", &login_state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256");
    let mut response = redirect(location.as_str())?;
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie(LOGIN_COOKIE, &sealed, LOGIN_ATTEMPT_TTL_SECONDS),
    );
    Ok(response)
}

#[derive(Deserialize)]
pub(crate) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    iss: Option<String>,
}

/// Failure responses from the callback always clear the login cookie and
/// never set a session.
fn callback_failure(error: CampusError) -> Response {
    let mut response = error.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_cookie(LOGIN_COOKIE));
    response
}

pub(crate) async fn callback(
    State(state): State<CampusState>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Response {
    match complete_login(&state, query, &headers).await {
        Ok(response) => response,
        Err(error) => callback_failure(error),
    }
}

async fn complete_login(
    state: &CampusState,
    query: CallbackQuery,
    headers: &HeaderMap,
) -> Result<Response, CampusError> {
    let runtime = &state.runtime;
    let config = &runtime.config;
    if query.error.is_some() {
        return Err(CampusError::bad_request(
            "the identity provider did not complete the login",
        ));
    }
    // RFC 9207 mix-up defense when the IdP sends the issuer parameter.
    if query.iss.as_deref().is_some_and(|iss| iss != config.issuer) {
        return Err(CampusError::bad_request(
            "login response came from another issuer",
        ));
    }
    let (Some(code), Some(login_state)) = (query.code, query.state) else {
        return Err(CampusError::bad_request("login response is incomplete"));
    };
    if code.is_empty() || code.len() > MAX_CODE_CHARS || !is_well_formed_token(&login_state) {
        return Err(CampusError::bad_request("login response is malformed"));
    }

    let now = runtime.clock.now();
    let not_this_browser =
        || CampusError::bad_request("login attempt was not started by this browser");
    let attempt = match read_cookie(headers, LOGIN_COOKIE) {
        Ok(Some(value)) => runtime
            .login_cookie_key
            .open(&config.tenant_id, &config.issuer, &value),
        _ => None,
    }
    .ok_or_else(not_this_browser)?;
    if !secrets_match(&login_state, &attempt.state) {
        return Err(not_this_browser());
    }
    if attempt.expires_at <= now {
        return Err(CampusError::bad_request("login attempt expired"));
    }
    if attempt.issued_at > now + config.clock_skew_seconds {
        return Err(CampusError::bad_request("login attempt is not yet valid"));
    }
    // Recorded only once the sealed cookie has proved this browser started
    // the login, and before the code is redeemed, so a state is single-use
    // even when the exchange or token validation fails afterwards.
    let fresh = state
        .store
        .consume_campus_login_state(ConsumeCampusLoginState {
            state_hash: sha256_hex(&attempt.state),
            tenant_id: config.tenant_id.clone(),
            now,
            expires_at: attempt.expires_at,
        })
        .await?;
    if !fresh {
        return Err(CampusError::bad_request("login attempt was already used"));
    }

    let tokens = runtime
        .http
        .exchange_code(TokenExchange {
            code: &code,
            code_verifier: &attempt.pkce_verifier,
            redirect_uri: config.redirect_uri.as_str(),
        })
        .await
        .map_err(|error| {
            tracing::warn!(reason = ?error, "campus token exchange failed");
            match error {
                OidcHttpError::Rejected => {
                    CampusError::unauthorized("the identity provider rejected the login")
                }
                OidcHttpError::Unavailable | OidcHttpError::InvalidResponse => {
                    CampusError::bad_gateway("the identity provider is unavailable")
                }
            }
        })?;
    let verified = runtime
        .validator
        .validate(&tokens.id_token, &attempt.nonce, now)
        .await
        .map_err(|error| {
            tracing::warn!(reason = error.as_str(), "campus id_token rejected");
            CampusError::unauthorized("the identity assertion is not valid")
        })?;
    drop(tokens);

    let identity = state
        .store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: format!("cid_{}", uuid::Uuid::now_v7().simple()),
            key: CampusIdentityKey {
                tenant_id: config.tenant_id.clone(),
                issuer: config.issuer.clone(),
                subject: verified.subject,
            },
            email: verified.email,
            display_name: verified.name,
            now,
        })
        .await?;

    // A login replaces any session this browser already presented.
    if let Some(previous) = session_token(headers) {
        state
            .store
            .revoke_campus_session(&sha256_hex(&previous), now)
            .await?;
    }
    let session_id = random(state)?;
    let absolute_expires_at = now + config.session_absolute_ttl_seconds;
    state
        .store
        .create_campus_session(CreateCampusSession {
            id_hash: sha256_hex(&session_id),
            tenant_id: config.tenant_id.clone(),
            identity_id: identity.id.clone(),
            csrf_hash: sha256_hex(&csrf_token_for_session(&session_id)),
            now,
            idle_expires_at: (now + config.session_idle_ttl_seconds).min(absolute_expires_at),
            absolute_expires_at,
        })
        .await?;
    tracing::info!(identity_id = %identity.id, "campus session created");

    let mut response = redirect(&format!(
        "{}{}",
        config.public_origin, config.post_login_path
    ))?;
    *response.status_mut() = StatusCode::SEE_OTHER;
    let cookies = response.headers_mut();
    cookies.append(header::SET_COOKIE, clear_cookie(LOGIN_COOKIE));
    cookies.append(
        header::SET_COOKIE,
        set_cookie(
            SESSION_COOKIE,
            &session_id,
            config.session_absolute_ttl_seconds,
        ),
    );
    Ok(response)
}

pub(crate) async fn logout(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
) -> Result<Response, CampusError> {
    state
        .store
        .revoke_campus_session(&principal.session_id_hash, state.runtime.clock.now())
        .await?;
    tracing::info!(identity_id = %principal.identity_id, "campus session revoked by logout");
    let mut response = no_store(Json(json!({ "logged_out": true })).into_response());
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_cookie(SESSION_COOKIE));
    Ok(response)
}

#[derive(Serialize)]
struct MembershipView {
    course_id: String,
    role: CampusCourseRole,
}

pub(crate) async fn me(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
) -> Result<Response, CampusError> {
    let memberships = state
        .store
        .list_campus_memberships_for_identity(&principal.key)
        .await?
        .into_iter()
        .map(|(course_id, role)| MembershipView { course_id, role })
        .collect::<Vec<_>>();
    Ok(no_store(
        Json(json!({
            "tenant_id": principal.key.tenant_id,
            "identity": {
                "id": principal.identity_id,
                "issuer": principal.key.issuer,
                "subject": principal.key.subject,
                "email": principal.email,
                "display_name": principal.display_name,
            },
            "is_tenant_admin": principal.is_tenant_admin,
            "memberships": memberships,
            "csrf_token": principal.csrf_token,
            "session": {
                "idle_expires_at": principal.idle_expires_at,
                "absolute_expires_at": principal.absolute_expires_at,
            },
        }))
        .into_response(),
    ))
}

fn validate_course_id(course_id: &str) -> Result<(), CampusError> {
    let valid = !course_id.is_empty()
        && course_id.len() <= MAX_COURSE_ID_CHARS
        && course_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(CampusError::bad_request(
            "course id must be 1-128 characters of [A-Za-z0-9._-]",
        ))
    }
}

/// Resolve the caller's current standing for a course and require `action`.
/// Roles are read from storage on every request, never from the session.
pub(crate) async fn require_course_action(
    state: &CampusState,
    principal: &CampusPrincipal,
    course_id: &str,
    action: CampusAction,
) -> Result<Option<CampusCourseRole>, CampusError> {
    validate_course_id(course_id)?;
    let course_role = state
        .store
        .get_campus_course_role(&principal.key, course_id)
        .await?;
    let actor = CampusActor {
        tenant_id: &principal.key.tenant_id,
        is_tenant_admin: principal.is_tenant_admin,
        course_role,
    };
    match authorize(&actor, &state.runtime.config.tenant_id, action) {
        CampusDecision::Allow => Ok(course_role),
        CampusDecision::Deny => Err(CampusError::forbidden(
            "not authorized for this campus course",
        )),
    }
}

pub(crate) async fn get_course(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(course_id): Path<String>,
) -> Result<Response, CampusError> {
    let role =
        require_course_action(&state, &principal, &course_id, CampusAction::ViewCourse).await?;
    if role.is_none()
        && !state
            .store
            .campus_course_exists(&principal.key.tenant_id, &course_id)
            .await?
    {
        return Err(CampusError::not_found("campus course not found"));
    }
    Ok(no_store(
        Json(json!({
            "tenant_id": principal.key.tenant_id,
            "course_id": course_id,
            "role": role,
            "is_tenant_admin": principal.is_tenant_admin,
        }))
        .into_response(),
    ))
}

pub(crate) async fn list_members(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(course_id): Path<String>,
) -> Result<Response, CampusError> {
    require_course_action(&state, &principal, &course_id, CampusAction::ViewRoster).await?;
    let members = state
        .store
        .list_campus_course_members(&principal.key.tenant_id, &course_id)
        .await?
        .into_iter()
        .map(|member| {
            json!({
                "issuer": member.issuer,
                "subject": member.subject,
                "role": member.role,
                "email": member.email,
                "display_name": member.display_name,
            })
        })
        .collect::<Vec<_>>();
    Ok(no_store(
        Json(json!({ "course_id": course_id, "members": members })).into_response(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PutMemberRequest {
    issuer: String,
    subject: String,
    role: CampusCourseRole,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteMemberRequest {
    issuer: String,
    subject: String,
}

fn member_key(
    state: &CampusState,
    principal: &CampusPrincipal,
    issuer: String,
    subject: String,
) -> Result<CampusIdentityKey, CampusError> {
    if issuer != state.runtime.config.issuer {
        return Err(CampusError::bad_request(
            "members must use the configured campus issuer",
        ));
    }
    if !is_valid_subject(&subject) {
        return Err(CampusError::bad_request("member subject is invalid"));
    }
    Ok(CampusIdentityKey {
        tenant_id: principal.key.tenant_id.clone(),
        issuer,
        subject,
    })
}

pub(crate) async fn put_member(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(course_id): Path<String>,
    Json(request): Json<PutMemberRequest>,
) -> Result<Response, CampusError> {
    require_course_action(&state, &principal, &course_id, CampusAction::ManageMembers).await?;
    let key = member_key(&state, &principal, request.issuer, request.subject)?;
    let created = state
        .store
        .put_campus_course_membership(PutCampusMembership {
            key: key.clone(),
            course_id: course_id.clone(),
            role: request.role,
            granted_by_identity_id: principal.identity_id.clone(),
            now: state.runtime.clock.now(),
        })
        .await?;
    tracing::info!(
        actor = %principal.identity_id,
        course_id = %course_id,
        role = request.role.as_str(),
        "campus course membership set"
    );
    Ok(no_store(
        Json(json!({
            "course_id": course_id,
            "issuer": key.issuer,
            "subject": key.subject,
            "role": request.role,
            "created": created,
        }))
        .into_response(),
    ))
}

pub(crate) async fn delete_member(
    State(state): State<CampusState>,
    Extension(principal): Extension<CampusPrincipal>,
    Path(course_id): Path<String>,
    Json(request): Json<DeleteMemberRequest>,
) -> Result<Response, CampusError> {
    require_course_action(&state, &principal, &course_id, CampusAction::ManageMembers).await?;
    let key = member_key(&state, &principal, request.issuer, request.subject)?;
    let removed = state
        .store
        .delete_campus_course_membership(&key, &course_id)
        .await?;
    tracing::info!(
        actor = %principal.identity_id,
        course_id = %course_id,
        removed,
        "campus course membership removed"
    );
    Ok(no_store(
        Json(json!({ "course_id": course_id, "removed": removed })).into_response(),
    ))
}
