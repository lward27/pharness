use super::super::config::{load_campus_config, validate_campus_startup, CampusConfig};
use super::{b64, random_login_cookie_key};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use jsonwebtoken::Algorithm;
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::path::PathBuf;

fn temp_file(contents: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pharness-campus-secret-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

/// A client secret file plus a valid login-cookie key file, both removed on
/// drop.
struct SecretFile(PathBuf, PathBuf);

impl SecretFile {
    fn new(contents: &str) -> Self {
        Self::bytes(contents.as_bytes())
    }

    fn bytes(contents: &[u8]) -> Self {
        Self(temp_file(contents), temp_file(&random_login_cookie_key()))
    }
}

impl Drop for SecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(&self.1);
    }
}

fn env(secret: &SecretFile) -> HashMap<&'static str, String> {
    HashMap::from([
        ("PHARNESS_CAMPUS_ENABLED", "true".to_string()),
        ("PHARNESS_CAMPUS_TENANT_ID", "pilot-university".to_string()),
        (
            "PHARNESS_CAMPUS_PUBLIC_ORIGIN",
            "https://campus.example.edu".to_string(),
        ),
        (
            "PHARNESS_CAMPUS_OIDC_ISSUER",
            "https://login.example.edu/realms/campus".to_string(),
        ),
        ("PHARNESS_CAMPUS_OIDC_CLIENT_ID", "pharness".to_string()),
        (
            "PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE",
            secret.0.display().to_string(),
        ),
        (
            "PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE",
            secret.1.display().to_string(),
        ),
        (
            "PHARNESS_CAMPUS_OIDC_AUTHORIZATION_ENDPOINT",
            "https://login.example.edu/realms/campus/protocol/openid-connect/auth".to_string(),
        ),
        (
            "PHARNESS_CAMPUS_OIDC_TOKEN_ENDPOINT",
            "https://login.example.edu/realms/campus/protocol/openid-connect/token".to_string(),
        ),
        (
            "PHARNESS_CAMPUS_OIDC_JWKS_URI",
            "https://login.example.edu/realms/campus/protocol/openid-connect/certs".to_string(),
        ),
        (
            "PHARNESS_CAMPUS_OIDC_REDIRECT_URI",
            "https://campus.example.edu/api/campus/auth/callback".to_string(),
        ),
    ])
}

fn load(values: &HashMap<&'static str, String>) -> anyhow::Result<Option<CampusConfig>> {
    load_campus_config(|name| values.get(name).cloned())
}

fn error(values: &HashMap<&'static str, String>) -> String {
    format!(
        "{:#}",
        load(values).expect_err("configuration must be rejected")
    )
}

#[test]
fn campus_is_disabled_unless_explicitly_enabled() {
    assert!(load_campus_config(|_| None).unwrap().is_none());
    for value in ["false", "0", "no", ""] {
        let values = HashMap::from([("PHARNESS_CAMPUS_ENABLED", value.to_string())]);
        assert!(load(&values).unwrap().is_none(), "{value}");
    }
    let values = HashMap::from([("PHARNESS_CAMPUS_ENABLED", "maybe".to_string())]);
    assert!(error(&values).contains("PHARNESS_CAMPUS_ENABLED"));
}

#[test]
fn complete_configuration_loads_with_bounded_defaults() {
    let secret = SecretFile::new("super-secret-value\n");
    let config = load(&env(&secret)).unwrap().unwrap();
    assert_eq!(config.tenant_id, "pilot-university");
    assert_eq!(config.public_origin, "https://campus.example.edu");
    assert_eq!(config.issuer, "https://login.example.edu/realms/campus");
    assert_eq!(
        config.allowed_algorithms,
        vec![Algorithm::RS256, Algorithm::ES256]
    );
    assert_eq!(config.clock_skew_seconds, 60);
    assert_eq!(config.session_absolute_ttl_seconds, 8 * 60 * 60);
    assert_eq!(config.session_idle_ttl_seconds, 30 * 60);
    assert_eq!(config.scopes, "openid email profile");
    assert_eq!(config.post_login_path, "/");
    assert!(!config.allow_insecure_loopback_http);
    assert!(config.bootstrap_admins.is_empty());
    let debug = format!("{config:?}");
    assert!(
        !debug.contains("super-secret-value"),
        "the client secret must never be formatted"
    );
    let key = config.login_cookie_key.expose_secret();
    assert_eq!(key.as_slice(), std::fs::read(&secret.1).unwrap().as_slice());
    assert!(
        !debug.contains(&format!("{key:?}")),
        "the key is never formatted"
    );
    assert!(!debug.contains(&b64(key)));
}

#[test]
fn every_required_value_is_enforced_when_enabled() {
    let secret = SecretFile::new("secret");
    for name in [
        "PHARNESS_CAMPUS_TENANT_ID",
        "PHARNESS_CAMPUS_PUBLIC_ORIGIN",
        "PHARNESS_CAMPUS_OIDC_ISSUER",
        "PHARNESS_CAMPUS_OIDC_CLIENT_ID",
        "PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE",
        "PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE",
        "PHARNESS_CAMPUS_OIDC_AUTHORIZATION_ENDPOINT",
        "PHARNESS_CAMPUS_OIDC_TOKEN_ENDPOINT",
        "PHARNESS_CAMPUS_OIDC_JWKS_URI",
        "PHARNESS_CAMPUS_OIDC_REDIRECT_URI",
    ] {
        let mut values = env(&secret);
        values.remove(name);
        assert!(error(&values).contains(name), "{name} must be required");
        values.insert(name, "   ".into());
        assert!(error(&values).contains(name), "{name} must not be blank");
    }
}

#[test]
fn endpoints_require_https_and_http_is_only_an_explicit_loopback_dev_path() {
    let secret = SecretFile::new("secret");
    for name in [
        "PHARNESS_CAMPUS_OIDC_ISSUER",
        "PHARNESS_CAMPUS_OIDC_AUTHORIZATION_ENDPOINT",
        "PHARNESS_CAMPUS_OIDC_TOKEN_ENDPOINT",
        "PHARNESS_CAMPUS_OIDC_JWKS_URI",
    ] {
        let mut values = env(&secret);
        values.insert(name, "http://login.example.edu/x".into());
        assert!(error(&values).contains(name), "{name} http must fail");
        values.insert(name, "http://127.0.0.1:8080/x".into());
        assert!(
            error(&values).contains(name),
            "{name} loopback http must fail by default"
        );
        values.insert(
            "PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP",
            "true".into(),
        );
        assert!(load(&values).is_ok(), "{name} loopback http with dev flag");
        values.insert(name, "http://login.example.edu/x".into());
        assert!(
            error(&values).contains("loopback"),
            "{name} non-loopback http fails even with the dev flag"
        );
        values.insert(name, "https://user:pw@login.example.edu/x".into());
        assert!(error(&values).contains(name));
        values.insert(name, "ftp://login.example.edu/x".into());
        assert!(error(&values).contains(name));
    }

    let mut values = env(&secret);
    values.insert(
        "PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP",
        "true".into(),
    );
    values.insert(
        "PHARNESS_CAMPUS_PUBLIC_ORIGIN",
        "http://localhost:4777".into(),
    );
    values.insert(
        "PHARNESS_CAMPUS_OIDC_REDIRECT_URI",
        "http://localhost:4777/api/campus/auth/callback".into(),
    );
    let config = load(&values).unwrap().unwrap();
    assert_eq!(config.public_origin, "http://localhost:4777");
    assert!(config.allow_insecure_loopback_http);
}

#[test]
fn redirect_uri_must_be_the_callback_on_the_public_origin() {
    let secret = SecretFile::new("secret");
    for redirect in [
        "https://other.example.edu/api/campus/auth/callback",
        "https://campus.example.edu/api/campus/auth/elsewhere",
        "https://campus.example.edu/api/campus/auth/callback?next=/",
        "https://campus.example.edu:8443/api/campus/auth/callback",
    ] {
        let mut values = env(&secret);
        values.insert("PHARNESS_CAMPUS_OIDC_REDIRECT_URI", redirect.into());
        assert!(
            error(&values).contains("PHARNESS_CAMPUS_OIDC_REDIRECT_URI"),
            "{redirect}"
        );
    }
    let mut values = env(&secret);
    values.insert(
        "PHARNESS_CAMPUS_PUBLIC_ORIGIN",
        "https://campus.example.edu/app".into(),
    );
    assert!(error(&values).contains("PHARNESS_CAMPUS_PUBLIC_ORIGIN"));
}

#[test]
fn only_asymmetric_algorithms_are_permitted() {
    let secret = SecretFile::new("secret");
    for algorithms in ["none", "HS256", "RS256,HS512", "hs384", "ES256K", ""] {
        let mut values = env(&secret);
        values.insert("PHARNESS_CAMPUS_OIDC_ALLOWED_ALGS", algorithms.into());
        if algorithms.is_empty() {
            // Blank falls back to the default allowlist.
            assert!(load(&values).is_ok());
        } else {
            assert!(
                error(&values).contains("PHARNESS_CAMPUS_OIDC_ALLOWED_ALGS"),
                "{algorithms}"
            );
        }
    }
    let mut values = env(&secret);
    values.insert(
        "PHARNESS_CAMPUS_OIDC_ALLOWED_ALGS",
        "ES384, PS256 ,ES384".into(),
    );
    assert_eq!(
        load(&values).unwrap().unwrap().allowed_algorithms,
        vec![Algorithm::ES384, Algorithm::PS256]
    );
}

#[test]
fn skew_and_session_lifetimes_are_bounded() {
    let secret = SecretFile::new("secret");
    let cases = [
        ("PHARNESS_CAMPUS_OIDC_CLOCK_SKEW_SECONDS", "301"),
        ("PHARNESS_CAMPUS_OIDC_CLOCK_SKEW_SECONDS", "-1"),
        ("PHARNESS_CAMPUS_OIDC_CLOCK_SKEW_SECONDS", "sixty"),
        ("PHARNESS_CAMPUS_SESSION_ABSOLUTE_TTL_SECONDS", "60"),
        ("PHARNESS_CAMPUS_SESSION_ABSOLUTE_TTL_SECONDS", "86401"),
        ("PHARNESS_CAMPUS_SESSION_IDLE_TTL_SECONDS", "10"),
        ("PHARNESS_CAMPUS_SESSION_IDLE_TTL_SECONDS", "28801"),
    ];
    for (name, value) in cases {
        let mut values = env(&secret);
        values.insert(name, value.into());
        assert!(error(&values).contains(name), "{name}={value}");
    }
    let mut values = env(&secret);
    values.insert("PHARNESS_CAMPUS_SESSION_ABSOLUTE_TTL_SECONDS", "900".into());
    let config = load(&values).unwrap().unwrap();
    assert_eq!(config.session_idle_ttl_seconds, 900);
}

#[test]
fn secret_file_must_exist_and_hold_a_secret_without_echoing_it() {
    let mut values = env(&SecretFile::new("unused"));
    values.insert(
        "PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE",
        "/nonexistent/pharness/campus-secret".into(),
    );
    assert!(error(&values).contains("PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE"));

    let empty = SecretFile::new("\n\n");
    let values = env(&empty);
    assert!(error(&values).contains("PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE"));

    let control = SecretFile::new("visible-part\u{7}hidden-part");
    let values = env(&control);
    let message = error(&values);
    assert!(message.contains("PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE"));
    assert!(!message.contains("visible-part"));
}

#[test]
fn bootstrap_admins_must_use_the_configured_issuer() {
    let secret = SecretFile::new("secret");
    let mut values = env(&secret);
    values.insert(
        "PHARNESS_CAMPUS_BOOTSTRAP_ADMINS",
        "https://login.example.edu/realms/campus|admin-1, https://login.example.edu/realms/campus|admin-2".into(),
    );
    assert_eq!(
        load(&values).unwrap().unwrap().bootstrap_admins,
        vec![
            (
                "https://login.example.edu/realms/campus".to_string(),
                "admin-1".to_string()
            ),
            (
                "https://login.example.edu/realms/campus".to_string(),
                "admin-2".to_string()
            ),
        ]
    );
    for admins in [
        "https://evil.example|admin",
        "admin-without-issuer",
        "https://login.example.edu/realms/campus|",
    ] {
        values.insert("PHARNESS_CAMPUS_BOOTSTRAP_ADMINS", admins.into());
        assert!(
            error(&values).contains("PHARNESS_CAMPUS_BOOTSTRAP_ADMINS"),
            "{admins}"
        );
    }
}

#[test]
fn tenant_scopes_and_post_login_path_are_validated() {
    let secret = SecretFile::new("secret");
    let cases = [
        ("PHARNESS_CAMPUS_TENANT_ID", "tenant with spaces"),
        ("PHARNESS_CAMPUS_OIDC_SCOPES", "email profile"),
        ("PHARNESS_CAMPUS_POST_LOGIN_PATH", "https://evil.example/"),
        ("PHARNESS_CAMPUS_POST_LOGIN_PATH", "//evil.example/"),
        ("PHARNESS_CAMPUS_POST_LOGIN_PATH", "/\\evil.example"),
    ];
    for (name, value) in cases {
        let mut values = env(&secret);
        values.insert(name, value.into());
        assert!(error(&values).contains(name), "{name}={value}");
    }
}

#[test]
fn enabled_campus_requires_operator_tokens() {
    let secret = SecretFile::new("secret");
    let config = load(&env(&secret)).unwrap().unwrap();
    let message = format!(
        "{:#}",
        validate_campus_startup(Some(&config), 0).unwrap_err()
    );
    assert!(message.contains("PHARNESS_OPERATOR_TOKENS"));
    assert!(validate_campus_startup(Some(&config), 1).is_ok());
    assert!(validate_campus_startup(None, 0).is_ok());
}

#[test]
fn login_cookie_key_is_32_random_bytes_raw_or_base64() {
    let secret = SecretFile::new("secret");
    let key = random_login_cookie_key();
    for (label, contents) in [
        ("raw", key.to_vec()),
        ("base64", format!("{}\n", STANDARD.encode(key)).into_bytes()),
        ("base64url", format!("  {}  ", b64(&key)).into_bytes()),
    ] {
        let file = SecretFile::bytes(&contents);
        let mut values = env(&secret);
        values.insert(
            "PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE",
            file.0.display().to_string(),
        );
        let config = load(&values).unwrap().unwrap();
        assert_eq!(config.login_cookie_key.expose_secret(), &key, "{label}");
    }

    let printable = "k".repeat(16) + &"e".repeat(16);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("31 raw bytes", key[..31].to_vec()),
        ("33 raw bytes", [key.as_slice(), &[7]].concat()),
        ("short base64", STANDARD.encode(&key[..16]).into_bytes()),
        (
            "long base64",
            STANDARD.encode([key, key].concat()).into_bytes(),
        ),
        (
            "not base64",
            "not a key at all, just some text!!".as_bytes().to_vec(),
        ),
        ("32 printable characters", printable.clone().into_bytes()),
        ("all zero", vec![0; 32]),
        ("oversized", vec![0x41; 4096]),
    ];
    for (label, contents) in cases {
        let file = SecretFile::bytes(&contents);
        let mut values = env(&secret);
        values.insert(
            "PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE",
            file.0.display().to_string(),
        );
        let message = error(&values);
        assert!(
            message.contains("PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE"),
            "{label}: {message}"
        );
        assert!(!message.contains(&printable), "{label} echoed the file");
        assert!(
            !message.contains("just some text"),
            "{label} echoed the file"
        );
    }

    let mut values = env(&secret);
    values.insert(
        "PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE",
        "/nonexistent/pharness/login-cookie-key".into(),
    );
    assert!(error(&values).contains("PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE"));
}
