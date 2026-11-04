//! The sealed, stateless login cookie.
//!
//! Starting a login writes nothing to the database. The state, nonce, PKCE
//! verifier and validity window travel in a `__Host-` cookie sealed with
//! ChaCha20-Poly1305 under a server key, so only this deployment can create
//! or read one. Holding the cookie is what binds the login to the browser
//! that started it.
//!
//! Wire format, base64url without padding:
//! `version (1) || nonce (12) || ciphertext || tag (16)`.
//! The version, a domain-separation label, the tenant and the issuer are the
//! AEAD associated data, so a cookie sealed for another version, tenant,
//! issuer or key never opens.

use super::tokens::{is_well_formed_token, TOKEN_CHARS};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};

pub(crate) const LOGIN_COOKIE_KEY_BYTES: usize = 32;
pub(crate) const LOGIN_COOKIE_VERSION: u8 = 1;
const AAD_LABEL: &[u8] = b"pharness-campus-login-cookie\0";
const TAG_LEN: usize = 16;
const PLAINTEXT_LEN: usize = 8 + 8 + 3 * TOKEN_CHARS;
const SEALED_LEN: usize = 1 + NONCE_LEN + PLAINTEXT_LEN + TAG_LEN;
const SEALED_CHARS: usize = (SEALED_LEN * 4).div_ceil(3);

/// One in-flight login. The secrets never leave the sealed cookie except as
/// the state and challenge sent to the IdP and the verifier sent to the
/// token endpoint.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LoginAttempt {
    pub(crate) state: String,
    pub(crate) nonce: String,
    pub(crate) pkce_verifier: String,
    pub(crate) issued_at: i64,
    pub(crate) expires_at: i64,
}

impl std::fmt::Debug for LoginAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoginAttempt")
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

pub(crate) struct LoginCookieKey(LessSafeKey);

impl std::fmt::Debug for LoginCookieKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LoginCookieKey(..)")
    }
}

fn associated_data(version: u8, tenant_id: &str, issuer: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_LABEL.len() + 2 + tenant_id.len() + issuer.len());
    aad.extend_from_slice(AAD_LABEL);
    aad.push(version);
    aad.extend_from_slice(tenant_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(issuer.as_bytes());
    aad
}

impl LoginCookieKey {
    pub(crate) fn new(key: &[u8; LOGIN_COOKIE_KEY_BYTES]) -> Self {
        let key = UnboundKey::new(&CHACHA20_POLY1305, key)
            .expect("a 32-byte key is valid for ChaCha20-Poly1305");
        Self(LessSafeKey::new(key))
    }

    /// Seal `attempt` under a fresh random 96-bit nonce.
    pub(crate) fn seal(
        &self,
        rng: &SystemRandom,
        tenant_id: &str,
        issuer: &str,
        attempt: &LoginAttempt,
    ) -> Result<String, ring::error::Unspecified> {
        for token in [&attempt.state, &attempt.nonce, &attempt.pkce_verifier] {
            if !is_well_formed_token(token) {
                return Err(ring::error::Unspecified);
            }
        }
        let mut nonce = [0_u8; NONCE_LEN];
        rng.fill(&mut nonce)?;
        let mut in_out = Vec::with_capacity(PLAINTEXT_LEN + TAG_LEN);
        in_out.extend_from_slice(&attempt.issued_at.to_be_bytes());
        in_out.extend_from_slice(&attempt.expires_at.to_be_bytes());
        in_out.extend_from_slice(attempt.state.as_bytes());
        in_out.extend_from_slice(attempt.nonce.as_bytes());
        in_out.extend_from_slice(attempt.pkce_verifier.as_bytes());
        self.0.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(associated_data(LOGIN_COOKIE_VERSION, tenant_id, issuer)),
            &mut in_out,
        )?;
        let mut sealed = Vec::with_capacity(SEALED_LEN);
        sealed.push(LOGIN_COOKIE_VERSION);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&in_out);
        Ok(URL_SAFE_NO_PAD.encode(sealed))
    }

    /// Open a cookie value. Any malformed, tampered, foreign-key,
    /// foreign-tenant, foreign-issuer or unknown-version value is `None`.
    pub(crate) fn open(&self, tenant_id: &str, issuer: &str, value: &str) -> Option<LoginAttempt> {
        if value.len() != SEALED_CHARS {
            return None;
        }
        let sealed = URL_SAFE_NO_PAD.decode(value).ok()?;
        if sealed.len() != SEALED_LEN || sealed[0] != LOGIN_COOKIE_VERSION {
            return None;
        }
        let nonce = Nonce::try_assume_unique_for_key(&sealed[1..1 + NONCE_LEN]).ok()?;
        let mut in_out = sealed[1 + NONCE_LEN..].to_vec();
        let plaintext = self
            .0
            .open_in_place(
                nonce,
                Aad::from(associated_data(LOGIN_COOKIE_VERSION, tenant_id, issuer)),
                &mut in_out,
            )
            .ok()?;
        if plaintext.len() != PLAINTEXT_LEN {
            return None;
        }
        let (issued_at, rest) = plaintext.split_at(8);
        let (expires_at, rest) = rest.split_at(8);
        let token = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .ok()
                .filter(|token| is_well_formed_token(token))
                .map(str::to_string)
        };
        Some(LoginAttempt {
            issued_at: i64::from_be_bytes(issued_at.try_into().ok()?),
            expires_at: i64::from_be_bytes(expires_at.try_into().ok()?),
            state: token(&rest[..TOKEN_CHARS])?,
            nonce: token(&rest[TOKEN_CHARS..2 * TOKEN_CHARS])?,
            pkce_verifier: token(&rest[2 * TOKEN_CHARS..])?,
        })
    }
}
