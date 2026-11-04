//! Campus browser sessions, cookies and CSRF enforcement.
//!
//! Only the `__Host-pharness-campus` cookie authenticates a Campus request.
//! `Authorization` headers are never read here, so operator and worker bearer
//! tokens cannot authorize Campus routes; conversely the operator middleware
//! never reads cookies.

use super::config::CampusConfig;
use super::error::CampusError;
use super::state::CampusState;
use super::tokens::{csrf_token_for_session, is_well_formed_token, secrets_match, sha256_hex};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use pharness_store::CampusIdentityKey;

pub(crate) const SESSION_COOKIE: &str = "__Host-pharness-campus";
pub(crate) const LOGIN_COOKIE: &str = "__Host-pharness-campus-login";
pub(crate) const CSRF_HEADER: &str = "x-campus-csrf";

/// `__Host-` requires `Secure`, `Path=/` and no `Domain`, which pins the
/// cookie to exactly this origin. `SameSite=Lax` still sends it on the IdP's
/// top-level redirect back to the callback.
pub(crate) fn set_cookie(name: &str, value: &str, max_age_seconds: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}"
    ))
    .expect("cookie names and base64url tokens are valid header values")
}

pub(crate) fn clear_cookie(name: &str) -> HeaderValue {
    set_cookie(name, "", 0)
}

/// Read one cookie by exact name. Duplicate cookies are treated as an error
/// rather than picking one.
pub(crate) fn read_cookie(headers: &HeaderMap, name: &str) -> Result<Option<String>, ()> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| ())?;
        for pair in value.split(';') {
            let Some((cookie_name, cookie_value)) = pair.trim().split_once('=') else {
                continue;
            };
            if cookie_name.trim() == name {
                if found.is_some() {
                    return Err(());
                }
                found = Some(cookie_value.trim().to_string());
            }
        }
    }
    Ok(found)
}

/// A well-formed session token from the request, if any.
pub(crate) fn session_token(headers: &HeaderMap) -> Option<String> {
    read_cookie(headers, SESSION_COOKIE)
        .ok()
        .flatten()
        .filter(|value| is_well_formed_token(value))
}

/// The authenticated Campus caller, derived entirely from the server-side
/// session and grants that were current when the request arrived.
#[derive(Clone)]
pub(crate) struct CampusPrincipal {
    pub(crate) key: CampusIdentityKey,
    pub(crate) identity_id: String,
    pub(crate) email: Option<String>,
    pub(crate) display_name: Option<String>,
    pub(crate) is_tenant_admin: bool,
    pub(crate) session_id_hash: String,
    pub(crate) csrf_token: String,
    pub(crate) idle_expires_at: i64,
    pub(crate) absolute_expires_at: i64,
}

fn unauthenticated() -> Response {
    let mut response =
        CampusError::unauthorized("campus session is missing, expired or revoked").into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_cookie(SESSION_COOKIE));
    response
}

pub(crate) fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Reject a state-changing request that is cross-origin or lacks the
/// session's CSRF token.
fn check_csrf(
    headers: &HeaderMap,
    config: &CampusConfig,
    expected_csrf_hash: &str,
) -> Result<(), CampusError> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let matches = origin
            .to_str()
            .is_ok_and(|origin| secrets_match(origin, &config.public_origin));
        if !matches {
            return Err(CampusError::forbidden(
                "request origin does not match the campus origin",
            ));
        }
    }
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site.as_bytes().eq_ignore_ascii_case(b"cross-site"))
    {
        return Err(CampusError::forbidden(
            "cross-site requests are not accepted",
        ));
    }
    let provided = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| is_well_formed_token(value))
        .ok_or_else(|| CampusError::forbidden("missing or invalid campus CSRF token"))?;
    if !secrets_match(&sha256_hex(provided), expected_csrf_hash) {
        return Err(CampusError::forbidden(
            "missing or invalid campus CSRF token",
        ));
    }
    Ok(())
}

/// Authenticate every protected Campus route from the session cookie alone.
pub(crate) async fn require_campus_session(
    State(state): State<CampusState>,
    mut request: Request,
    next: Next,
) -> Response {
    let runtime = &state.runtime;
    let config = &runtime.config;
    let Ok(Some(session_id)) = read_cookie(request.headers(), SESSION_COOKIE) else {
        return unauthenticated();
    };
    if !is_well_formed_token(&session_id) {
        return unauthenticated();
    }
    let session_id_hash = sha256_hex(&session_id);
    let now = runtime.clock.now();
    let active = match state
        .store
        .touch_campus_session(
            &session_id_hash,
            &config.tenant_id,
            now,
            config.session_idle_ttl_seconds,
        )
        .await
    {
        Ok(Some(active)) => active,
        Ok(None) => return unauthenticated(),
        Err(error) => return CampusError::from(error).into_response(),
    };
    // A session minted by a previous IdP configuration is not valid for the
    // current issuer.
    if active.identity.issuer != config.issuer {
        return unauthenticated();
    }
    if !is_safe_method(request.method()) {
        if let Err(error) = check_csrf(request.headers(), config, &active.csrf_hash) {
            return error.into_response();
        }
    }
    let key = CampusIdentityKey {
        tenant_id: active.tenant_id.clone(),
        issuer: active.identity.issuer.clone(),
        subject: active.identity.subject.clone(),
    };
    let is_tenant_admin = match state.store.is_campus_tenant_admin(&key).await {
        Ok(is_admin) => is_admin,
        Err(error) => return CampusError::from(error).into_response(),
    };
    request.extensions_mut().insert(CampusPrincipal {
        key,
        identity_id: active.identity.id,
        email: active.identity.email,
        display_name: active.identity.display_name,
        is_tenant_admin,
        session_id_hash,
        csrf_token: csrf_token_for_session(&session_id),
        idle_expires_at: active.idle_expires_at,
        absolute_expires_at: active.absolute_expires_at,
    });
    next.run(request).await
}
