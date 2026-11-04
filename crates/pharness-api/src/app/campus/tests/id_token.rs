use super::super::oidc::{
    IdTokenError, IdTokenValidator, JWKS_MAX_AGE_SECONDS, JWKS_MIN_REFRESH_SECONDS,
};
use super::{
    b64, base_claims, compact_jws, test_config, EcKey, MockIdp, CLIENT_ID, CLIENT_SECRET, START,
};
use hmac::{Hmac, Mac};
use jsonwebtoken::Algorithm;
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use serde_json::{json, Value};
use sha2::Sha256;
use std::sync::{Arc, OnceLock};

const NONCE: &str = "expected-nonce";

fn validator(idp: &Arc<MockIdp>) -> IdTokenValidator {
    IdTokenValidator::new(&test_config(), idp.clone())
}

fn setup() -> (EcKey, Arc<MockIdp>, IdTokenValidator) {
    let key = EcKey::generate("k1");
    let idp = MockIdp::with_keys(&[key.jwk()]);
    let validator = validator(&idp);
    (key, idp, validator)
}

async fn check(validator: &IdTokenValidator, token: &str) -> Result<(), IdTokenError> {
    validator.validate(token, NONCE, START).await.map(|_| ())
}

fn claims() -> Value {
    base_claims("student-1", NONCE, START)
}

#[tokio::test]
async fn a_valid_es256_token_yields_only_subject_and_display_claims() {
    let (key, _, validator) = setup();
    let verified = validator
        .validate(&key.sign(&claims()), NONCE, START)
        .await
        .unwrap();
    assert_eq!(verified.subject, "student-1");
    assert_eq!(verified.email.as_deref(), Some("student-1@campus.test"));
    assert_eq!(verified.name.as_deref(), Some("User student-1"));
}

#[tokio::test]
async fn issuer_audience_and_authorized_party_are_exact() {
    let (key, _, validator) = setup();
    let cases: Vec<(Value, IdTokenError)> = vec![
        (
            json!({"iss": "https://idp.campus.test/realms/other"}),
            IdTokenError::InvalidIssuer,
        ),
        (
            json!({"iss": "https://idp.campus.test/realms/pilot/"}),
            IdTokenError::InvalidIssuer,
        ),
        (json!({"iss": null}), IdTokenError::InvalidIssuer),
        (
            json!({"aud": "another-client"}),
            IdTokenError::InvalidAudience,
        ),
        (
            json!({"aud": ["another-client"]}),
            IdTokenError::InvalidAudience,
        ),
        (json!({"aud": null}), IdTokenError::InvalidAudience),
        (
            json!({"aud": [CLIENT_ID, "other"]}),
            IdTokenError::InvalidAuthorizedParty,
        ),
        (
            json!({"aud": [CLIENT_ID, "other"], "azp": "other"}),
            IdTokenError::InvalidAuthorizedParty,
        ),
        (
            json!({"azp": "other"}),
            IdTokenError::InvalidAuthorizedParty,
        ),
    ];
    for (overrides, expected) in cases {
        let mut claims = claims();
        for (name, value) in overrides.as_object().unwrap() {
            if value.is_null() {
                claims.as_object_mut().unwrap().remove(name);
            } else {
                claims[name] = value.clone();
            }
        }
        assert_eq!(
            check(&validator, &key.sign(&claims)).await,
            Err(expected),
            "{overrides}"
        );
    }
    let mut multi = claims();
    multi["aud"] = json!([CLIENT_ID, "other"]);
    multi["azp"] = json!(CLIENT_ID);
    assert_eq!(check(&validator, &key.sign(&multi)).await, Ok(()));
}

#[tokio::test]
async fn time_claims_are_checked_with_bounded_skew() {
    let (key, _, validator) = setup();
    let mut within_skew = claims();
    within_skew["exp"] = json!(START - 30);
    assert_eq!(check(&validator, &key.sign(&within_skew)).await, Ok(()));

    let cases = [
        ("exp", json!(START - 61), IdTokenError::Expired),
        ("exp", Value::Null, IdTokenError::Expired),
        ("nbf", json!(START + 61), IdTokenError::NotYetValid),
        ("iat", json!(START + 61), IdTokenError::IssuedInFuture),
        ("iat", Value::Null, IdTokenError::Malformed),
    ];
    for (name, value, expected) in cases {
        let mut claims = claims();
        if value.is_null() {
            claims.as_object_mut().unwrap().remove(name);
        } else {
            claims[name] = value.clone();
        }
        assert_eq!(
            check(&validator, &key.sign(&claims)).await,
            Err(expected),
            "{name}={value}"
        );
    }
    let mut nbf_within_skew = claims();
    nbf_within_skew["nbf"] = json!(START + 59);
    assert_eq!(check(&validator, &key.sign(&nbf_within_skew)).await, Ok(()));
}

#[tokio::test]
async fn nonce_and_subject_are_required() {
    let (key, _, validator) = setup();
    let mut wrong = claims();
    wrong["nonce"] = json!("attacker-nonce");
    assert_eq!(
        check(&validator, &key.sign(&wrong)).await,
        Err(IdTokenError::InvalidNonce)
    );
    let mut missing = claims();
    missing.as_object_mut().unwrap().remove("nonce");
    assert_eq!(
        check(&validator, &key.sign(&missing)).await,
        Err(IdTokenError::InvalidNonce)
    );
    for subject in [json!(""), json!("x".repeat(256)), json!("bad\nsubject")] {
        let mut claims = claims();
        claims["sub"] = subject;
        assert_eq!(
            check(&validator, &key.sign(&claims)).await,
            Err(IdTokenError::InvalidSubject)
        );
    }
    let mut no_subject = claims();
    no_subject.as_object_mut().unwrap().remove("sub");
    assert_eq!(
        check(&validator, &key.sign(&no_subject)).await,
        Err(IdTokenError::InvalidSubject)
    );
}

#[tokio::test]
async fn none_hmac_and_confused_algorithms_are_rejected() {
    let (key, _, validator) = setup();
    let unsigned = format!(
        "{}.{}.",
        b64(json!({"alg": "none", "kid": "k1"}).to_string().as_bytes()),
        b64(claims().to_string().as_bytes())
    );
    assert_eq!(
        check(&validator, &unsigned).await,
        Err(IdTokenError::Malformed)
    );

    // An attacker who knows the client secret (or the public key) still
    // cannot mint an HMAC-signed token.
    let hmac_signed = compact_jws(
        &json!({"alg": "HS256", "kid": "k1"}),
        &claims(),
        |message| {
            let mut mac = Hmac::<Sha256>::new_from_slice(CLIENT_SECRET.as_bytes()).unwrap();
            mac.update(message);
            mac.finalize().into_bytes().to_vec()
        },
    );
    assert_eq!(
        check(&validator, &hmac_signed).await,
        Err(IdTokenError::DisallowedAlgorithm)
    );

    // RS256 is allowed, but kid k1 is a P-256 key.
    let confused = key.sign_with_header(&json!({"alg": "RS256", "kid": "k1"}), &claims());
    assert_eq!(
        check(&validator, &confused).await,
        Err(IdTokenError::AlgorithmMismatch)
    );

    // ES384 is outside the default allowlist.
    let es384 = key.sign_with_header(&json!({"alg": "ES384", "kid": "k1"}), &claims());
    assert_eq!(
        check(&validator, &es384).await,
        Err(IdTokenError::DisallowedAlgorithm)
    );

    // With ES384 allowed, a P-256 key still cannot verify it.
    let mut config = test_config();
    config.allowed_algorithms.push(Algorithm::ES384);
    let idp = MockIdp::with_keys(&[key.jwk()]);
    let widened = IdTokenValidator::new(&config, idp);
    assert_eq!(
        check(&widened, &es384).await,
        Err(IdTokenError::AlgorithmMismatch)
    );
}

#[tokio::test]
async fn kid_is_required_and_signatures_are_verified() {
    let (key, idp, validator) = setup();
    let missing = key.sign_with_header(&json!({"alg": "ES256"}), &claims());
    assert_eq!(
        check(&validator, &missing).await,
        Err(IdTokenError::MissingKid)
    );
    let empty = key.sign_with_header(&json!({"alg": "ES256", "kid": ""}), &claims());
    assert_eq!(
        check(&validator, &empty).await,
        Err(IdTokenError::MissingKid)
    );
    assert_eq!(idp.fetches(), 0, "no JWKS fetch before a kid is known");

    let token = key.sign(&claims());
    let (signing_input, _) = token.rsplit_once('.').unwrap();
    let forged_payload = {
        let mut parts = signing_input.split('.');
        let header = parts.next().unwrap();
        let mut elevated = claims();
        elevated["sub"] = json!("admin");
        format!("{header}.{}", b64(elevated.to_string().as_bytes()))
    };
    let original_signature = token.rsplit_once('.').unwrap().1;
    assert_eq!(
        check(
            &validator,
            &format!("{forged_payload}.{original_signature}")
        )
        .await,
        Err(IdTokenError::InvalidSignature)
    );

    let other = EcKey::generate("k1");
    assert_eq!(
        check(&validator, &other.sign(&claims())).await,
        Err(IdTokenError::InvalidSignature),
        "same kid, different key"
    );
    assert_eq!(
        check(&validator, "not-a-jwt").await,
        Err(IdTokenError::Malformed)
    );
}

#[tokio::test]
async fn jwks_rotation_accepts_new_keys_and_drops_removed_keys() {
    let old = EcKey::generate("old");
    let new = EcKey::generate("new");
    let idp = MockIdp::with_keys(&[old.jwk()]);
    let validator = validator(&idp);
    let at = |now: i64, key: &EcKey| {
        let token = key.sign(&base_claims("student-1", NONCE, now));
        (token, now)
    };

    let (token, now) = at(START, &old);
    validator.validate(&token, NONCE, now).await.unwrap();
    assert_eq!(idp.fetches(), 1);
    validator.validate(&token, NONCE, now + 5).await.unwrap();
    assert_eq!(idp.fetches(), 1, "known kid uses the cache");

    // The IdP rotates: the new kid triggers one refetch and is accepted.
    idp.set_keys(&[new.jwk()]);
    let now = START + JWKS_MIN_REFRESH_SECONDS;
    let (token, _) = at(now, &new);
    validator.validate(&token, NONCE, now).await.unwrap();
    assert_eq!(idp.fetches(), 2);

    // The removed key no longer validates once the set was replaced.
    let (token, _) = at(now, &old);
    assert_eq!(
        validator.validate(&token, NONCE, now).await,
        Err(IdTokenError::UnknownKid)
    );
    assert_eq!(idp.fetches(), 2, "refetch is rate limited");

    // Unknown kids within the refresh interval do not hammer the IdP.
    let stranger = EcKey::generate("stranger");
    let (token, _) = at(now + 1, &stranger);
    assert_eq!(
        validator.validate(&token, NONCE, now + 1).await,
        Err(IdTokenError::UnknownKid)
    );
    assert_eq!(idp.fetches(), 2);
}

#[tokio::test]
async fn cached_keys_expire_so_a_withdrawn_key_stops_validating() {
    let first = EcKey::generate("first");
    let second = EcKey::generate("second");
    let idp = MockIdp::with_keys(&[first.jwk(), second.jwk()]);
    let validator = validator(&idp);
    validator
        .validate(&first.sign(&base_claims("s", NONCE, START)), NONCE, START)
        .await
        .unwrap();

    idp.set_keys(&[second.jwk()]);
    let later = START + JWKS_MAX_AGE_SECONDS;
    assert_eq!(
        validator
            .validate(&first.sign(&base_claims("s", NONCE, later)), NONCE, later)
            .await,
        Err(IdTokenError::UnknownKid)
    );
    assert_eq!(idp.fetches(), 2);

    // If the IdP is unreachable once the cache is stale, nothing validates.
    idp.set_raw_jwks(json!("not a jwks"));
    let much_later = later + JWKS_MAX_AGE_SECONDS;
    assert_eq!(
        validator
            .validate(
                &second.sign(&base_claims("s", NONCE, much_later)),
                NONCE,
                much_later
            )
            .await,
        Err(IdTokenError::JwksUnavailable)
    );
}

#[tokio::test]
async fn symmetric_mismatched_and_duplicate_jwks_entries_are_never_trusted() {
    let key = EcKey::generate("ec");
    let mut wrong_alg = key.jwk();
    wrong_alg["alg"] = json!("RS256");
    wrong_alg["kid"] = json!("declared-rs256");
    let mut encryption = key.jwk();
    encryption["use"] = json!("enc");
    encryption["kid"] = json!("enc");
    let idp = MockIdp::with_keys(&[
        json!({"kty": "oct", "kid": "shared", "k": b64(CLIENT_SECRET.as_bytes())}),
        wrong_alg,
        encryption,
    ]);
    let validator = validator(&idp);
    for kid in ["shared", "declared-rs256", "enc"] {
        let token = key.sign_with_header(&json!({"alg": "ES256", "kid": kid}), &claims());
        assert_eq!(
            check(&validator, &token).await,
            Err(IdTokenError::UnknownKid),
            "{kid}"
        );
    }

    let duplicate = MockIdp::with_keys(&[key.jwk(), key.jwk()]);
    let validator = IdTokenValidator::new(&test_config(), duplicate);
    let token = key.sign_with_header(&json!({"alg": "ES256", "kid": "ec"}), &claims());
    assert_eq!(
        check(&validator, &token).await,
        Err(IdTokenError::JwksUnavailable)
    );
}

/// A 2048-bit RSA key generated once per test process; ring cannot
/// generate RSA keys and jsonwebtoken (via ring) rejects keys under 2048 bits.
struct RsaFixture {
    pair: RsaKeyPair,
    n: Vec<u8>,
    e: Vec<u8>,
}

fn rsa_fixture() -> &'static RsaFixture {
    static FIXTURE: OnceLock<RsaFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
        let der = key.to_pkcs1_der().unwrap();
        RsaFixture {
            pair: RsaKeyPair::from_der(der.as_bytes()).unwrap(),
            n: key.n().to_bytes_be(),
            e: key.e().to_bytes_be(),
        }
    })
}

fn rsa_jwk(kid: &str, n: &[u8], e: &[u8]) -> Value {
    json!({
        "kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256",
        "n": b64(n), "e": b64(e),
    })
}

fn rs256_token(header: &Value, claims: &Value) -> String {
    let fixture = rsa_fixture();
    let rng = SystemRandom::new();
    compact_jws(header, claims, |message| {
        let mut signature = vec![0; fixture.pair.public().modulus_len()];
        fixture
            .pair
            .sign(&RSA_PKCS1_SHA256, &rng, message, &mut signature)
            .unwrap();
        signature
    })
}

/// RS256 keys are built from the JWK's `n` and `e`: the same signature
/// verifies only under the JWK that carries the signer's exact modulus and
/// exponent.
#[tokio::test]
async fn rs256_keys_are_constructed_from_modulus_and_exponent() {
    let fixture = rsa_fixture();
    let mut garbage_modulus = fixture.n.clone();
    for (index, byte) in garbage_modulus.iter_mut().enumerate().skip(1) {
        *byte ^= u8::try_from(index % 251).unwrap() | 1;
    }
    let mut neighbour_modulus = fixture.n.clone();
    let last = neighbour_modulus.len() - 1;
    neighbour_modulus[last] ^= 0x02;
    let idp = MockIdp::with_keys(&[
        rsa_jwk("correct", &fixture.n, &fixture.e),
        rsa_jwk("garbage-modulus", &garbage_modulus, &fixture.e),
        rsa_jwk("neighbour-modulus", &neighbour_modulus, &fixture.e),
        rsa_jwk("other-exponent", &fixture.n, &[0x01, 0x00, 0x03]),
    ]);
    let validator = validator(&idp);
    for (kid, expected) in [
        ("correct", Ok(())),
        ("garbage-modulus", Err(IdTokenError::InvalidSignature)),
        ("neighbour-modulus", Err(IdTokenError::InvalidSignature)),
        ("other-exponent", Err(IdTokenError::InvalidSignature)),
    ] {
        let token = rs256_token(&json!({"alg": "RS256", "kid": kid}), &claims());
        assert_eq!(check(&validator, &token).await, expected, "{kid}");
    }

    let es = compact_jws(
        &json!({"alg": "ES256", "kid": "correct"}),
        &claims(),
        |_| vec![0x42; 64],
    );
    assert_eq!(
        check(&validator, &es).await,
        Err(IdTokenError::AlgorithmMismatch)
    );
}

/// A full RS256 round trip with a key generated when the test runs.
#[tokio::test]
async fn rs256_round_trip_with_a_runtime_generated_key() {
    let fixture = rsa_fixture();
    let idp = MockIdp::with_keys(&[rsa_jwk("rsa-runtime", &fixture.n, &fixture.e)]);
    let validator = validator(&idp);
    let header = json!({"alg": "RS256", "kid": "rsa-runtime"});
    let token = rs256_token(&header, &claims());
    assert_eq!(check(&validator, &token).await, Ok(()));

    let mut tampered = claims();
    tampered["nonce"] = json!("other");
    let token = rs256_token(&header, &tampered);
    assert_eq!(
        check(&validator, &token).await,
        Err(IdTokenError::InvalidNonce)
    );
    let (input, _) = token.rsplit_once('.').unwrap();
    let forged = format!("{input}.{}", b64(&[0x11; 256]));
    assert_eq!(
        check(&validator, &forged).await,
        Err(IdTokenError::InvalidSignature)
    );
}
