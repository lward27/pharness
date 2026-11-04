use super::super::login_cookie::{LoginAttempt, LoginCookieKey, LOGIN_COOKIE_VERSION};
use super::super::tokens::random_token;
use super::{b64, random_login_cookie_key, ISSUER, TENANT};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::SystemRandom;

fn attempt() -> LoginAttempt {
    let rng = SystemRandom::new();
    LoginAttempt {
        state: random_token(&rng).unwrap(),
        nonce: random_token(&rng).unwrap(),
        pkce_verifier: random_token(&rng).unwrap(),
        issued_at: 1_900_000_000,
        expires_at: 1_900_000_600,
    }
}

fn seal(key: &LoginCookieKey, attempt: &LoginAttempt) -> String {
    key.seal(&SystemRandom::new(), TENANT, ISSUER, attempt)
        .unwrap()
}

#[test]
fn a_sealed_cookie_opens_only_under_the_same_key_tenant_and_issuer() {
    let key_bytes = random_login_cookie_key();
    let key = LoginCookieKey::new(&key_bytes);
    let attempt = attempt();
    let sealed = seal(&key, &attempt);
    assert_eq!(key.open(TENANT, ISSUER, &sealed), Some(attempt.clone()));
    assert_eq!(
        LoginCookieKey::new(&key_bytes).open(TENANT, ISSUER, &sealed),
        Some(attempt.clone()),
        "the same key bytes open it after a restart"
    );

    for secret in [&attempt.state, &attempt.nonce, &attempt.pkce_verifier] {
        assert!(!sealed.contains(secret.as_str()), "secrets are encrypted");
    }
    let raw = URL_SAFE_NO_PAD.decode(&sealed).unwrap();
    assert_eq!(raw[0], LOGIN_COOKIE_VERSION);

    let other_key = LoginCookieKey::new(&random_login_cookie_key());
    assert_eq!(other_key.open(TENANT, ISSUER, &sealed), None);
    assert_eq!(key.open("tenant-b", ISSUER, &sealed), None);
    assert_eq!(
        key.open(TENANT, "https://idp.campus.test/realms/other", &sealed),
        None
    );
}

#[test]
fn every_single_bit_flip_is_rejected() {
    let key = LoginCookieKey::new(&random_login_cookie_key());
    let raw = URL_SAFE_NO_PAD.decode(seal(&key, &attempt())).unwrap();
    for index in 0..raw.len() {
        for bit in 0..8 {
            let mut tampered = raw.clone();
            tampered[index] ^= 1 << bit;
            assert_eq!(
                key.open(TENANT, ISSUER, &b64(&tampered)),
                None,
                "byte {index} bit {bit}"
            );
        }
    }
}

#[test]
fn truncated_extended_and_malformed_values_are_rejected() {
    let key = LoginCookieKey::new(&random_login_cookie_key());
    let sealed = seal(&key, &attempt());
    let raw = URL_SAFE_NO_PAD.decode(&sealed).unwrap();
    let mut extended = raw.clone();
    extended.push(0);
    for value in [
        String::new(),
        sealed[..sealed.len() - 1].to_string(),
        format!("{sealed}A"),
        b64(&raw[..raw.len() - 1]),
        b64(&extended),
        sealed.replace(|c: char| c.is_ascii_alphabetic(), "*"),
        "A".repeat(sealed.len()),
    ] {
        assert_eq!(key.open(TENANT, ISSUER, &value), None, "{value}");
    }
}

#[test]
fn nonces_are_fresh_and_malformed_attempts_are_never_sealed() {
    let key = LoginCookieKey::new(&random_login_cookie_key());
    let attempt = attempt();
    let first = URL_SAFE_NO_PAD.decode(seal(&key, &attempt)).unwrap();
    let second = URL_SAFE_NO_PAD.decode(seal(&key, &attempt)).unwrap();
    assert_ne!(first[1..13], second[1..13], "a random nonce per cookie");
    assert_ne!(first, second);

    let mut bad = attempt;
    bad.state = "short".into();
    assert!(key
        .seal(&SystemRandom::new(), TENANT, ISSUER, &bad)
        .is_err());
}

#[test]
fn debug_output_never_reveals_secrets() {
    let attempt = attempt();
    let debug = format!("{attempt:?}");
    assert!(!debug.contains(&attempt.state));
    assert!(!debug.contains(&attempt.nonce));
    assert!(!debug.contains(&attempt.pkce_verifier));
    let key_bytes = random_login_cookie_key();
    let debug = format!("{:?}", LoginCookieKey::new(&key_bytes));
    assert!(!debug.contains(&b64(&key_bytes)));
}
