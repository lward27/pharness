//! OIDC provider interactions and id_token validation.
//!
//! HTTP calls sit behind [`OidcHttp`] so tests use an in-process IdP. Tokens
//! returned by the IdP are validated and then dropped: no id_token or
//! access_token is stored, logged, returned or forwarded anywhere.

use super::config::{is_valid_subject, CampusConfig};
use super::tokens::secrets_match;
use async_trait::async_trait;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use url::Url;

const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;
const MAX_KID_CHARS: usize = 256;
const MAX_JWKS_BYTES: usize = 256 * 1024;
const MAX_JWKS_KEYS: usize = 64;
const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_EMAIL_CHARS: usize = 320;
const MAX_NAME_CHARS: usize = 200;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// An unknown `kid` triggers at most one JWKS fetch per this interval, so a
/// stream of forged tokens cannot hammer the IdP.
pub(crate) const JWKS_MIN_REFRESH_SECONDS: i64 = 60;
/// Cached keys are refetched on use after this age so a key the IdP removed
/// stops validating even when no unknown `kid` appears.
pub(crate) const JWKS_MAX_AGE_SECONDS: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OidcHttpError {
    Unavailable,
    Rejected,
    InvalidResponse,
}

pub(crate) struct TokenExchange<'a> {
    pub(crate) code: &'a str,
    pub(crate) code_verifier: &'a str,
    pub(crate) redirect_uri: &'a str,
}

/// The only part of a token response PHarness reads. Access and refresh
/// tokens are never deserialized.
pub(crate) struct IdTokenResponse {
    pub(crate) id_token: String,
}

impl std::fmt::Debug for IdTokenResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IdTokenResponse([REDACTED])")
    }
}

#[async_trait]
pub(crate) trait OidcHttp: Send + Sync {
    async fn fetch_jwks(&self) -> Result<Vec<u8>, OidcHttpError>;
    async fn exchange_code(
        &self,
        request: TokenExchange<'_>,
    ) -> Result<IdTokenResponse, OidcHttpError>;
}

/// Production IdP client: rustls, no redirects, bounded time and body size.
pub(crate) struct ReqwestOidcHttp {
    client: reqwest::Client,
    token_endpoint: Url,
    jwks_uri: Url,
    client_id: String,
    client_secret: SecretString,
}

impl ReqwestOidcHttp {
    pub(crate) fn new(config: &CampusConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .https_only(!config.allow_insecure_loopback_http)
            .build()?;
        Ok(Self {
            client,
            token_endpoint: config.token_endpoint.clone(),
            jwks_uri: config.jwks_uri.clone(),
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
        })
    }
}

async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, OidcHttpError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(OidcHttpError::InvalidResponse);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| OidcHttpError::Unavailable)?
    {
        if body.len() + chunk.len() > limit {
            return Err(OidcHttpError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[async_trait]
impl OidcHttp for ReqwestOidcHttp {
    async fn fetch_jwks(&self) -> Result<Vec<u8>, OidcHttpError> {
        let response = self
            .client
            .get(self.jwks_uri.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| OidcHttpError::Unavailable)?;
        if !response.status().is_success() {
            return Err(OidcHttpError::Rejected);
        }
        read_limited(response, MAX_JWKS_BYTES).await
    }

    async fn exchange_code(
        &self,
        request: TokenExchange<'_>,
    ) -> Result<IdTokenResponse, OidcHttpError> {
        // RFC 6749 section 2.3.1: client credentials are form-encoded before
        // being placed in the Basic authorization header.
        let response = self
            .client
            .post(self.token_endpoint.clone())
            .basic_auth(
                form_encode(&self.client_id),
                Some(form_encode(self.client_secret.expose_secret())),
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", request.code),
                ("redirect_uri", request.redirect_uri),
                ("code_verifier", request.code_verifier),
            ])
            .send()
            .await
            .map_err(|_| OidcHttpError::Unavailable)?;
        if !response.status().is_success() {
            return Err(OidcHttpError::Rejected);
        }
        let body = read_limited(response, MAX_TOKEN_RESPONSE_BYTES).await?;
        #[derive(Deserialize)]
        struct TokenResponseBody {
            id_token: Option<String>,
        }
        let parsed: TokenResponseBody =
            serde_json::from_slice(&body).map_err(|_| OidcHttpError::InvalidResponse)?;
        parsed
            .id_token
            .filter(|token| !token.is_empty())
            .map(|id_token| IdTokenResponse { id_token })
            .ok_or(OidcHttpError::InvalidResponse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdTokenError {
    Malformed,
    DisallowedAlgorithm,
    MissingKid,
    UnknownKid,
    AlgorithmMismatch,
    InvalidSignature,
    JwksUnavailable,
    InvalidIssuer,
    InvalidAudience,
    InvalidAuthorizedParty,
    Expired,
    NotYetValid,
    IssuedInFuture,
    InvalidNonce,
    InvalidSubject,
}

impl IdTokenError {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::DisallowedAlgorithm => "disallowed_algorithm",
            Self::MissingKid => "missing_kid",
            Self::UnknownKid => "unknown_kid",
            Self::AlgorithmMismatch => "algorithm_mismatch",
            Self::InvalidSignature => "invalid_signature",
            Self::JwksUnavailable => "jwks_unavailable",
            Self::InvalidIssuer => "invalid_issuer",
            Self::InvalidAudience => "invalid_audience",
            Self::InvalidAuthorizedParty => "invalid_authorized_party",
            Self::Expired => "expired",
            Self::NotYetValid => "not_yet_valid",
            Self::IssuedInFuture => "issued_in_future",
            Self::InvalidNonce => "invalid_nonce",
            Self::InvalidSubject => "invalid_subject",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyFamily {
    Rsa,
    EcP256,
    EcP384,
}

impl KeyFamily {
    fn accepts(self, algorithm: Algorithm) -> bool {
        match self {
            Self::Rsa => matches!(
                algorithm,
                Algorithm::RS256
                    | Algorithm::RS384
                    | Algorithm::RS512
                    | Algorithm::PS256
                    | Algorithm::PS384
                    | Algorithm::PS512
            ),
            Self::EcP256 => algorithm == Algorithm::ES256,
            Self::EcP384 => algorithm == Algorithm::ES384,
        }
    }
}

#[derive(Clone)]
struct VerificationKey {
    family: KeyFamily,
    declared_algorithm: Option<Algorithm>,
    key: DecodingKey,
}

#[derive(Deserialize)]
struct RawJwks {
    keys: Vec<RawJwk>,
}

#[derive(Deserialize)]
struct RawJwk {
    kty: String,
    kid: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    alg: Option<String>,
    n: Option<String>,
    e: Option<String>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

/// Parse a JWKS into verification keys indexed by `kid`.
///
/// Symmetric (`oct`) and unsupported keys are skipped, never accepted. A
/// duplicate `kid` makes the whole set ambiguous and is rejected.
fn parse_jwks(body: &[u8]) -> Result<HashMap<String, VerificationKey>, IdTokenError> {
    if body.len() > MAX_JWKS_BYTES {
        return Err(IdTokenError::JwksUnavailable);
    }
    let jwks: RawJwks = serde_json::from_slice(body).map_err(|_| IdTokenError::JwksUnavailable)?;
    if jwks.keys.len() > MAX_JWKS_KEYS {
        return Err(IdTokenError::JwksUnavailable);
    }
    let mut keys = HashMap::new();
    for jwk in jwks.keys {
        let Some(kid) = jwk
            .kid
            .clone()
            .filter(|kid| !kid.is_empty() && kid.len() <= MAX_KID_CHARS)
        else {
            continue;
        };
        if jwk
            .key_use
            .as_deref()
            .is_some_and(|key_use| key_use != "sig")
        {
            continue;
        }
        let Some(key) = verification_key(&jwk) else {
            continue;
        };
        if keys.insert(kid, key).is_some() {
            return Err(IdTokenError::JwksUnavailable);
        }
    }
    Ok(keys)
}

fn verification_key(jwk: &RawJwk) -> Option<VerificationKey> {
    let (family, key) = match jwk.kty.as_str() {
        "RSA" => {
            let key =
                DecodingKey::from_rsa_components(jwk.n.as_deref()?, jwk.e.as_deref()?).ok()?;
            (KeyFamily::Rsa, key)
        }
        "EC" => {
            let family = match jwk.crv.as_deref()? {
                "P-256" => KeyFamily::EcP256,
                "P-384" => KeyFamily::EcP384,
                _ => return None,
            };
            let key = DecodingKey::from_ec_components(jwk.x.as_deref()?, jwk.y.as_deref()?).ok()?;
            (family, key)
        }
        _ => return None,
    };
    let declared_algorithm = match jwk.alg.as_deref() {
        None => None,
        Some(alg) => {
            let algorithm = alg.parse::<Algorithm>().ok()?;
            if !family.accepts(algorithm) {
                return None;
            }
            Some(algorithm)
        }
    };
    Some(VerificationKey {
        family,
        declared_algorithm,
        key,
    })
}

#[derive(Default)]
struct JwksState {
    keys: HashMap<String, VerificationKey>,
    fetched_at: Option<i64>,
    last_attempt_at: Option<i64>,
}

/// JWKS cache that supports key rotation.
///
/// A token with an unknown `kid`, or any token once the cache is older than
/// [`JWKS_MAX_AGE_SECONDS`], triggers a refetch that is rate-limited to one
/// attempt per [`JWKS_MIN_REFRESH_SECONDS`]. A successful refetch replaces the
/// whole key set, so a key removed by the IdP stops validating.
pub(crate) struct JwksCache {
    http: Arc<dyn OidcHttp>,
    state: Mutex<JwksState>,
}

impl JwksCache {
    pub(crate) fn new(http: Arc<dyn OidcHttp>) -> Self {
        Self {
            http,
            state: Mutex::new(JwksState::default()),
        }
    }

    async fn key_for(&self, kid: &str, now: i64) -> Result<VerificationKey, IdTokenError> {
        let mut state = self.state.lock().await;
        let stale = state
            .fetched_at
            .map_or(true, |fetched| now - fetched >= JWKS_MAX_AGE_SECONDS);
        let may_refresh = state.last_attempt_at.map_or(true, |attempted| {
            now - attempted >= JWKS_MIN_REFRESH_SECONDS
        });
        let mut refresh_error = None;
        if (stale || !state.keys.contains_key(kid)) && may_refresh {
            state.last_attempt_at = Some(now);
            match self
                .http
                .fetch_jwks()
                .await
                .map_err(|_| IdTokenError::JwksUnavailable)
                .and_then(|body| parse_jwks(&body))
            {
                Ok(keys) => {
                    state.keys = keys;
                    state.fetched_at = Some(now);
                }
                Err(error) => {
                    tracing::warn!(reason = error.as_str(), "campus JWKS refresh failed");
                    refresh_error = Some(error);
                }
            }
        }
        if state
            .fetched_at
            .map_or(true, |fetched| now - fetched >= JWKS_MAX_AGE_SECONDS)
        {
            // Never keep trusting keys the IdP may have withdrawn.
            state.keys.clear();
            return Err(refresh_error.unwrap_or(IdTokenError::JwksUnavailable));
        }
        state.keys.get(kid).cloned().ok_or(IdTokenError::UnknownKid)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct RawClaims {
    iss: Option<String>,
    sub: Option<String>,
    aud: Option<Audience>,
    azp: Option<String>,
    exp: Option<i64>,
    nbf: Option<i64>,
    iat: Option<i64>,
    nonce: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

/// The only claims PHarness keeps. `subject` is an identifier; `email` and
/// `name` are display-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedIdToken {
    pub(crate) subject: String,
    pub(crate) email: Option<String>,
    pub(crate) name: Option<String>,
}

pub(crate) struct IdTokenValidator {
    issuer: String,
    client_id: String,
    allowed_algorithms: Vec<Algorithm>,
    clock_skew_seconds: i64,
    jwks: JwksCache,
}

impl IdTokenValidator {
    pub(crate) fn new(config: &CampusConfig, http: Arc<dyn OidcHttp>) -> Self {
        Self {
            issuer: config.issuer.clone(),
            client_id: config.client_id.clone(),
            allowed_algorithms: config.allowed_algorithms.clone(),
            clock_skew_seconds: config.clock_skew_seconds,
            jwks: JwksCache::new(http),
        }
    }

    pub(crate) async fn validate(
        &self,
        token: &str,
        expected_nonce: &str,
        now: i64,
    ) -> Result<VerifiedIdToken, IdTokenError> {
        if token.is_empty() || token.len() > MAX_ID_TOKEN_BYTES {
            return Err(IdTokenError::Malformed);
        }
        // `alg: none` does not parse into `Algorithm` and is rejected here.
        let header = jsonwebtoken::decode_header(token).map_err(|_| IdTokenError::Malformed)?;
        if !self.allowed_algorithms.contains(&header.alg) {
            return Err(IdTokenError::DisallowedAlgorithm);
        }
        let kid = header
            .kid
            .as_deref()
            .filter(|kid| !kid.is_empty() && kid.len() <= MAX_KID_CHARS)
            .ok_or(IdTokenError::MissingKid)?;
        let key = self.jwks.key_for(kid, now).await?;
        if !key.family.accepts(header.alg)
            || key
                .declared_algorithm
                .is_some_and(|declared| declared != header.alg)
        {
            return Err(IdTokenError::AlgorithmMismatch);
        }

        // Signature and algorithm only; every claim is checked below against
        // the injected clock so that skew handling is explicit.
        let mut validation = Validation::new(header.alg);
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        validation.required_spec_claims = HashSet::new();
        validation.leeway = 0;
        let claims = jsonwebtoken::decode::<RawClaims>(token, &key.key, &validation)
            .map_err(|error| match error.kind() {
                ErrorKind::InvalidSignature | ErrorKind::Crypto(_) => {
                    IdTokenError::InvalidSignature
                }
                ErrorKind::InvalidAlgorithm | ErrorKind::InvalidAlgorithmName => {
                    IdTokenError::AlgorithmMismatch
                }
                _ => IdTokenError::Malformed,
            })?
            .claims;
        self.check_claims(claims, expected_nonce, now)
    }

    fn check_claims(
        &self,
        claims: RawClaims,
        expected_nonce: &str,
        now: i64,
    ) -> Result<VerifiedIdToken, IdTokenError> {
        if claims.iss.as_deref() != Some(self.issuer.as_str()) {
            return Err(IdTokenError::InvalidIssuer);
        }
        let audiences = match claims.aud {
            Some(Audience::One(audience)) => vec![audience],
            Some(Audience::Many(audiences)) => audiences,
            None => Vec::new(),
        };
        if !audiences.iter().any(|audience| audience == &self.client_id) {
            return Err(IdTokenError::InvalidAudience);
        }
        match claims.azp.as_deref() {
            Some(azp) if azp != self.client_id => return Err(IdTokenError::InvalidAuthorizedParty),
            None if audiences.len() > 1 => return Err(IdTokenError::InvalidAuthorizedParty),
            _ => {}
        }
        let skew = self.clock_skew_seconds;
        match claims.exp {
            Some(exp) if exp.saturating_add(skew) > now => {}
            _ => return Err(IdTokenError::Expired),
        }
        if claims.nbf.is_some_and(|nbf| nbf.saturating_sub(skew) > now) {
            return Err(IdTokenError::NotYetValid);
        }
        match claims.iat {
            Some(iat) if iat.saturating_sub(skew) <= now => {}
            Some(_) => return Err(IdTokenError::IssuedInFuture),
            None => return Err(IdTokenError::Malformed),
        }
        match claims.nonce.as_deref() {
            Some(nonce) if secrets_match(nonce, expected_nonce) => {}
            _ => return Err(IdTokenError::InvalidNonce),
        }
        let subject = claims
            .sub
            .filter(|subject| is_valid_subject(subject))
            .ok_or(IdTokenError::InvalidSubject)?;
        Ok(VerifiedIdToken {
            subject,
            email: display_claim(claims.email, MAX_EMAIL_CHARS),
            name: display_claim(claims.name, MAX_NAME_CHARS),
        })
    }
}

fn display_claim(value: Option<String>, max_chars: usize) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| {
        !value.is_empty()
            && value.chars().count() <= max_chars
            && !value.chars().any(char::is_control)
    })
}
