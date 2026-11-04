//! Random secrets and their hashed or derived forms.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Length of a base64url-encoded 256-bit token without padding.
pub(crate) const TOKEN_CHARS: usize = 43;

const CSRF_DOMAIN: &[u8] = b"pharness-campus-csrf-v1\0";

/// A 256-bit random token, base64url without padding.
pub(crate) fn random_token(rng: &SystemRandom) -> Result<String, ring::error::Unspecified> {
    let mut bytes = [0_u8; 32];
    rng.fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn is_well_formed_token(value: &str) -> bool {
    value.len() == TOKEN_CHARS
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(crate) fn sha256_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Compare two secrets without revealing where they differ or their lengths.
pub(crate) fn secrets_match(provided: &str, expected: &str) -> bool {
    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    bool::from(provided.as_slice().ct_eq(expected.as_slice()))
}

/// RFC 7636 S256 code challenge.
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The per-session CSRF token, derived from the secret session id with domain
/// separation so it can be recomputed from the cookie without being stored.
/// Knowing it does not reveal the session id.
pub(crate) fn csrf_token_for_session(session_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CSRF_DOMAIN);
    hasher.update(session_id.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}
