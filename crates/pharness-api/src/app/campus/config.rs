//! Explicit Campus OIDC configuration.
//!
//! Every endpoint is configured directly; there is no discovery fetch, so the
//! IdP cannot change PHarness endpoints at runtime. Swapping the test IdP for
//! the institution's issuer is a configuration-only change.

use anyhow::{bail, Context};
use jsonwebtoken::Algorithm;
use secrecy::SecretString;
use std::net::IpAddr;
use url::Url;

pub(crate) const ENV_ENABLED: &str = "PHARNESS_CAMPUS_ENABLED";
pub(crate) const ENV_TENANT_ID: &str = "PHARNESS_CAMPUS_TENANT_ID";
pub(crate) const ENV_PUBLIC_ORIGIN: &str = "PHARNESS_CAMPUS_PUBLIC_ORIGIN";
pub(crate) const ENV_ISSUER: &str = "PHARNESS_CAMPUS_OIDC_ISSUER";
pub(crate) const ENV_CLIENT_ID: &str = "PHARNESS_CAMPUS_OIDC_CLIENT_ID";
pub(crate) const ENV_CLIENT_SECRET_FILE: &str = "PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE";
pub(crate) const ENV_AUTHORIZATION_ENDPOINT: &str = "PHARNESS_CAMPUS_OIDC_AUTHORIZATION_ENDPOINT";
pub(crate) const ENV_TOKEN_ENDPOINT: &str = "PHARNESS_CAMPUS_OIDC_TOKEN_ENDPOINT";
pub(crate) const ENV_JWKS_URI: &str = "PHARNESS_CAMPUS_OIDC_JWKS_URI";
pub(crate) const ENV_REDIRECT_URI: &str = "PHARNESS_CAMPUS_OIDC_REDIRECT_URI";
pub(crate) const ENV_SCOPES: &str = "PHARNESS_CAMPUS_OIDC_SCOPES";
pub(crate) const ENV_ALLOWED_ALGS: &str = "PHARNESS_CAMPUS_OIDC_ALLOWED_ALGS";
pub(crate) const ENV_CLOCK_SKEW: &str = "PHARNESS_CAMPUS_OIDC_CLOCK_SKEW_SECONDS";
pub(crate) const ENV_SESSION_ABSOLUTE_TTL: &str = "PHARNESS_CAMPUS_SESSION_ABSOLUTE_TTL_SECONDS";
pub(crate) const ENV_SESSION_IDLE_TTL: &str = "PHARNESS_CAMPUS_SESSION_IDLE_TTL_SECONDS";
pub(crate) const ENV_BOOTSTRAP_ADMINS: &str = "PHARNESS_CAMPUS_BOOTSTRAP_ADMINS";
pub(crate) const ENV_POST_LOGIN_PATH: &str = "PHARNESS_CAMPUS_POST_LOGIN_PATH";
pub(crate) const ENV_ALLOW_INSECURE_LOOPBACK_HTTP: &str =
    "PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP";

/// The only callback path; the redirect URI must name it on the public origin
/// so that the `__Host-` login cookie is sent back to the callback.
pub(crate) const CALLBACK_PATH: &str = "/api/campus/auth/callback";

const DEFAULT_SCOPES: &str = "openid email profile";
const DEFAULT_ALLOWED_ALGS: &str = "RS256,ES256";
const DEFAULT_CLOCK_SKEW_SECONDS: i64 = 60;
const MAX_CLOCK_SKEW_SECONDS: i64 = 300;
const DEFAULT_ABSOLUTE_TTL_SECONDS: i64 = 8 * 60 * 60;
const MIN_ABSOLUTE_TTL_SECONDS: i64 = 15 * 60;
const MAX_ABSOLUTE_TTL_SECONDS: i64 = 24 * 60 * 60;
const DEFAULT_IDLE_TTL_SECONDS: i64 = 30 * 60;
const MIN_IDLE_TTL_SECONDS: i64 = 5 * 60;
const MAX_BOOTSTRAP_ADMINS: usize = 32;
const MAX_CLIENT_SECRET_BYTES: usize = 4096;
pub(crate) const MAX_SUBJECT_CHARS: usize = 255;
pub(crate) const MAX_ISSUER_CHARS: usize = 512;

#[derive(Debug, Clone)]
pub(crate) struct CampusConfig {
    pub(crate) tenant_id: String,
    /// ASCII serialization of the browser origin, e.g. `https://campus.example.edu`.
    pub(crate) public_origin: String,
    /// Exact `iss` value; compared byte-for-byte, never normalized.
    pub(crate) issuer: String,
    pub(crate) client_id: String,
    pub(crate) client_secret: SecretString,
    pub(crate) authorization_endpoint: Url,
    pub(crate) token_endpoint: Url,
    pub(crate) jwks_uri: Url,
    pub(crate) redirect_uri: Url,
    pub(crate) scopes: String,
    pub(crate) allowed_algorithms: Vec<Algorithm>,
    pub(crate) clock_skew_seconds: i64,
    pub(crate) session_absolute_ttl_seconds: i64,
    pub(crate) session_idle_ttl_seconds: i64,
    /// `(issuer, subject)` pairs that are tenant administrators.
    pub(crate) bootstrap_admins: Vec<(String, String)>,
    pub(crate) post_login_path: String,
    pub(crate) allow_insecure_loopback_http: bool,
}

/// Load Campus configuration from the process environment.
///
/// Returns `Ok(None)` when Campus is disabled. When enabled, any missing or
/// invalid value is a startup error.
pub(crate) fn load_campus_config_from_env() -> anyhow::Result<Option<CampusConfig>> {
    load_campus_config(|name| std::env::var(name).ok())
}

pub(crate) fn load_campus_config(
    lookup: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<CampusConfig>> {
    let value = |name: &str| {
        lookup(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let required = |name: &str| {
        value(name).with_context(|| format!("{name} is required when {ENV_ENABLED} is true"))
    };

    if !parse_bool(ENV_ENABLED, value(ENV_ENABLED))? {
        return Ok(None);
    }
    let allow_insecure_loopback_http = parse_bool(
        ENV_ALLOW_INSECURE_LOOPBACK_HTTP,
        value(ENV_ALLOW_INSECURE_LOOPBACK_HTTP),
    )?;

    let tenant_id = required(ENV_TENANT_ID)?;
    if !is_identifier(&tenant_id, 64) {
        bail!("{ENV_TENANT_ID} must be 1-64 characters of [A-Za-z0-9._-]");
    }

    let origin_url = secure_url(
        ENV_PUBLIC_ORIGIN,
        &required(ENV_PUBLIC_ORIGIN)?,
        allow_insecure_loopback_http,
    )?;
    if !matches!(origin_url.path(), "" | "/") || origin_url.query().is_some() {
        bail!("{ENV_PUBLIC_ORIGIN} must be an origin without a path or query");
    }
    let public_origin = origin_url.origin().ascii_serialization();

    let issuer = required(ENV_ISSUER)?;
    if issuer.chars().count() > MAX_ISSUER_CHARS {
        bail!("{ENV_ISSUER} must be at most {MAX_ISSUER_CHARS} characters");
    }
    let issuer_url = secure_url(ENV_ISSUER, &issuer, allow_insecure_loopback_http)?;
    if issuer_url.query().is_some() {
        bail!("{ENV_ISSUER} must not contain a query");
    }

    let client_id = required(ENV_CLIENT_ID)?;
    if client_id.len() > 256 || client_id.chars().any(char::is_control) {
        bail!("{ENV_CLIENT_ID} must be at most 256 printable characters");
    }
    let client_secret = read_client_secret(&required(ENV_CLIENT_SECRET_FILE)?)?;

    let authorization_endpoint = secure_url(
        ENV_AUTHORIZATION_ENDPOINT,
        &required(ENV_AUTHORIZATION_ENDPOINT)?,
        allow_insecure_loopback_http,
    )?;
    let token_endpoint = secure_url(
        ENV_TOKEN_ENDPOINT,
        &required(ENV_TOKEN_ENDPOINT)?,
        allow_insecure_loopback_http,
    )?;
    let jwks_uri = secure_url(
        ENV_JWKS_URI,
        &required(ENV_JWKS_URI)?,
        allow_insecure_loopback_http,
    )?;
    let redirect_uri = secure_url(
        ENV_REDIRECT_URI,
        &required(ENV_REDIRECT_URI)?,
        allow_insecure_loopback_http,
    )?;
    if redirect_uri.origin().ascii_serialization() != public_origin
        || redirect_uri.path() != CALLBACK_PATH
        || redirect_uri.query().is_some()
    {
        bail!("{ENV_REDIRECT_URI} must be {public_origin}{CALLBACK_PATH}");
    }

    let scopes = value(ENV_SCOPES).unwrap_or_else(|| DEFAULT_SCOPES.to_string());
    let scope_items = scopes.split_whitespace().collect::<Vec<_>>();
    if !scope_items.contains(&"openid")
        || scope_items.iter().any(|scope| {
            !scope
                .chars()
                .all(|c| c.is_ascii_graphic() && c != '"' && c != '\\')
        })
    {
        bail!("{ENV_SCOPES} must be space-separated scope tokens including openid");
    }
    let scopes = scope_items.join(" ");

    let allowed_algorithms = parse_algorithms(
        &value(ENV_ALLOWED_ALGS).unwrap_or_else(|| DEFAULT_ALLOWED_ALGS.to_string()),
    )?;

    let clock_skew_seconds = parse_bounded(
        ENV_CLOCK_SKEW,
        value(ENV_CLOCK_SKEW),
        DEFAULT_CLOCK_SKEW_SECONDS,
        0,
        MAX_CLOCK_SKEW_SECONDS,
    )?;
    let session_absolute_ttl_seconds = parse_bounded(
        ENV_SESSION_ABSOLUTE_TTL,
        value(ENV_SESSION_ABSOLUTE_TTL),
        DEFAULT_ABSOLUTE_TTL_SECONDS,
        MIN_ABSOLUTE_TTL_SECONDS,
        MAX_ABSOLUTE_TTL_SECONDS,
    )?;
    let session_idle_ttl_seconds = parse_bounded(
        ENV_SESSION_IDLE_TTL,
        value(ENV_SESSION_IDLE_TTL),
        DEFAULT_IDLE_TTL_SECONDS.min(session_absolute_ttl_seconds),
        MIN_IDLE_TTL_SECONDS,
        session_absolute_ttl_seconds,
    )?;

    let bootstrap_admins = parse_bootstrap_admins(&issuer, value(ENV_BOOTSTRAP_ADMINS))?;

    let post_login_path = value(ENV_POST_LOGIN_PATH).unwrap_or_else(|| "/".to_string());
    if !post_login_path.starts_with('/')
        || post_login_path.starts_with("//")
        || post_login_path.contains('\\')
        || post_login_path.chars().any(char::is_control)
        || post_login_path.len() > 512
    {
        bail!("{ENV_POST_LOGIN_PATH} must be a same-origin absolute path such as /campus/");
    }

    Ok(Some(CampusConfig {
        tenant_id,
        public_origin,
        issuer,
        client_id,
        client_secret,
        authorization_endpoint,
        token_endpoint,
        jwks_uri,
        redirect_uri,
        scopes,
        allowed_algorithms,
        clock_skew_seconds,
        session_absolute_ttl_seconds,
        session_idle_ttl_seconds,
        bootstrap_admins,
        post_login_path,
        allow_insecure_loopback_http,
    }))
}

/// Campus is a student-facing surface; it must never sit next to SDLC
/// operator routes that are open because no operator token is configured.
pub(crate) fn validate_campus_startup(
    config: Option<&CampusConfig>,
    operator_token_count: usize,
) -> anyhow::Result<()> {
    if config.is_some() && operator_token_count == 0 {
        bail!(
            "PHARNESS_OPERATOR_TOKENS must contain at least one token when {ENV_ENABLED} is true"
        );
    }
    Ok(())
}

fn parse_bool(name: &str, value: Option<String>) -> anyhow::Result<bool> {
    match value.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("0" | "false" | "no") => Ok(false),
        Some("1" | "true" | "yes") => Ok(true),
        Some(_) => bail!("{name} must be true or false"),
    }
}

fn parse_bounded(
    name: &str,
    value: Option<String>,
    default: i64,
    min: i64,
    max: i64,
) -> anyhow::Result<i64> {
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed: i64 = value
        .parse()
        .with_context(|| format!("{name} must be an integer number of seconds"))?;
    if !(min..=max).contains(&parsed) {
        bail!("{name} must be between {min} and {max} seconds");
    }
    Ok(parsed)
}

fn is_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub(crate) fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => false,
    }
}

/// Require https. Plain http is accepted only for a loopback host and only
/// when the explicit development flag is set.
fn secure_url(name: &str, value: &str, allow_insecure_loopback_http: bool) -> anyhow::Result<Url> {
    let url = Url::parse(value).with_context(|| format!("{name} must be an absolute URL"))?;
    if url.host().is_none() || !url.username().is_empty() || url.password().is_some() {
        bail!("{name} must name a host and must not contain credentials");
    }
    if url.fragment().is_some() {
        bail!("{name} must not contain a fragment");
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if allow_insecure_loopback_http && is_loopback_host(&url) => Ok(url),
        "http" if allow_insecure_loopback_http => {
            bail!("{name} may use http only for a loopback host")
        }
        _ => bail!(
            "{name} must use https (http is only accepted for loopback hosts with {ENV_ALLOW_INSECURE_LOOPBACK_HTTP}=true)"
        ),
    }
}

fn read_client_secret(path: &str) -> anyhow::Result<SecretString> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {ENV_CLIENT_SECRET_FILE} at {path}"))?;
    if bytes.len() > MAX_CLIENT_SECRET_BYTES {
        bail!("{ENV_CLIENT_SECRET_FILE} at {path} is larger than {MAX_CLIENT_SECRET_BYTES} bytes");
    }
    let secret = String::from_utf8(bytes)
        .ok()
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty() && !secret.chars().any(char::is_control));
    match secret {
        Some(secret) => Ok(SecretString::new(secret)),
        None => {
            bail!("{ENV_CLIENT_SECRET_FILE} at {path} must contain one non-empty printable secret")
        }
    }
}

/// Only asymmetric signature algorithms are supported; `none` and HMAC
/// algorithms are rejected because a client secret must never verify an
/// identity assertion.
fn parse_algorithms(value: &str) -> anyhow::Result<Vec<Algorithm>> {
    let mut algorithms = Vec::new();
    for name in value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let algorithm = match name {
            "RS256" => Algorithm::RS256,
            "RS384" => Algorithm::RS384,
            "RS512" => Algorithm::RS512,
            "PS256" => Algorithm::PS256,
            "PS384" => Algorithm::PS384,
            "PS512" => Algorithm::PS512,
            "ES256" => Algorithm::ES256,
            "ES384" => Algorithm::ES384,
            other
                if other.eq_ignore_ascii_case("none")
                    || other.to_ascii_uppercase().starts_with("HS") =>
            {
                bail!("{ENV_ALLOWED_ALGS} must not permit {other}")
            }
            other => bail!("{ENV_ALLOWED_ALGS} contains unsupported algorithm {other}"),
        };
        if !algorithms.contains(&algorithm) {
            algorithms.push(algorithm);
        }
    }
    if algorithms.is_empty() {
        bail!("{ENV_ALLOWED_ALGS} must list at least one algorithm");
    }
    Ok(algorithms)
}

pub(crate) fn is_valid_subject(subject: &str) -> bool {
    !subject.is_empty()
        && subject.chars().count() <= MAX_SUBJECT_CHARS
        && !subject.chars().any(char::is_control)
}

fn parse_bootstrap_admins(
    issuer: &str,
    value: Option<String>,
) -> anyhow::Result<Vec<(String, String)>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let mut admins = Vec::new();
    for entry in value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let Some((admin_issuer, subject)) = entry.rsplit_once('|') else {
            bail!("{ENV_BOOTSTRAP_ADMINS} entries must be issuer|subject pairs");
        };
        let (admin_issuer, subject) = (admin_issuer.trim(), subject.trim());
        if admin_issuer != issuer {
            bail!("{ENV_BOOTSTRAP_ADMINS} entries must use the configured {ENV_ISSUER}");
        }
        if !is_valid_subject(subject) {
            bail!("{ENV_BOOTSTRAP_ADMINS} contains an invalid subject");
        }
        let pair = (admin_issuer.to_string(), subject.to_string());
        if !admins.contains(&pair) {
            admins.push(pair);
        }
    }
    if admins.len() > MAX_BOOTSTRAP_ADMINS {
        bail!("{ENV_BOOTSTRAP_ADMINS} may list at most {MAX_BOOTSTRAP_ADMINS} administrators");
    }
    Ok(admins)
}
