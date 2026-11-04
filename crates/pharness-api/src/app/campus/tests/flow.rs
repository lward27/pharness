use super::super::login_cookie::LoginAttempt;
use super::super::session::{LOGIN_COOKIE, SESSION_COOKIE};
use super::super::state::CampusMount;
use super::{
    b64, base_claims, body_json, body_text, full_app, issued_cookie, random_login_cookie_key,
    request, send, session_hash, set_cookies, Harness, Session, CLIENT_ID, ISSUER, ORIGIN, TENANT,
};
use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use pharness_store::SqliteStore;
use secrecy::ExposeSecret;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn disabled_or_unavailable_campus_answers_every_path_without_sessions() {
    for (mount, expected) in [
        (CampusMount::Disabled, StatusCode::NOT_FOUND),
        (
            CampusMount::Unavailable("campus is unavailable in read-only mode"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let app = full_app(
            Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            mount,
        );
        for (method, path) in [
            ("GET", "/api/campus"),
            ("GET", "/api/campus/"),
            ("GET", "/api/campus/auth/login"),
            ("GET", "/api/campus/auth/callback?code=c&state=s"),
            ("POST", "/api/campus/auth/logout"),
            ("GET", "/api/campus/me"),
            ("GET", "/api/campus/courses/cs101"),
            ("PUT", "/api/campus/courses/cs101/members"),
            ("DELETE", "/api/campus/courses/cs101/members"),
            ("GET", "/api/campus/anything/else"),
        ] {
            let response = send(
                &app,
                request(method, path)
                    .header(header::COOKIE, format!("{SESSION_COOKIE}=x"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(response.status(), expected, "{method} {path}");
            assert!(
                response.headers().get(header::SET_COOKIE).is_none(),
                "{method} {path} must not set cookies"
            );
            assert!(response.headers().get(header::LOCATION).is_none());
        }
    }
}

#[tokio::test]
async fn login_uses_pkce_state_nonce_and_hardened_cookies() {
    let harness = Harness::new().await;
    let login = harness.get("/api/campus/auth/login", None).await;
    assert_eq!(login.status(), StatusCode::FOUND);
    assert_eq!(login.headers()[header::CACHE_CONTROL], "no-store");
    let login_cookie = &set_cookies(login.headers(), LOGIN_COOKIE)[0];
    assert!(login_cookie.ends_with("; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=600"));
    assert!(!login_cookie.to_ascii_lowercase().contains("domain"));
    let location = login.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string();
    let state = url::Url::parse(&location)
        .unwrap()
        .query_pairs()
        .find(|(name, _)| name == "state")
        .unwrap()
        .1
        .into_owned();
    assert!(
        !login_cookie.contains(&state),
        "the login cookie carries the state only in sealed form"
    );

    let start = harness.start_login().await;
    assert_eq!(start.location.host_str(), Some("idp.campus.test"));
    assert_eq!(start.location.path(), "/authorize");
    let query = start
        .location
        .query_pairs()
        .into_owned()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(query["kc_idp_hint"], "campus", "configured query is kept");
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(
        query["redirect_uri"],
        format!("{ORIGIN}/api/campus/auth/callback")
    );
    assert_eq!(query["scope"], "openid email profile");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(start.state.len(), 43);
    assert_eq!(start.nonce.len(), 43);
    assert_ne!(start.state, start.nonce);

    let id_token = harness
        .key
        .sign(&base_claims("student-1", &start.nonce, harness.now()));
    let code = harness.code_for(&start, id_token.clone());
    let callback = harness
        .callback(&start.state, &code, Some(&start.login_cookie))
        .await;
    assert_eq!(callback.status(), StatusCode::SEE_OTHER);
    assert_eq!(callback.headers()[header::LOCATION], format!("{ORIGIN}/"));
    let session_cookie = &set_cookies(callback.headers(), SESSION_COOKIE)[0];
    assert!(session_cookie.starts_with("__Host-pharness-campus="));
    assert!(session_cookie.ends_with("; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=28800"));
    assert!(!session_cookie.to_ascii_lowercase().contains("domain"));
    assert_eq!(
        set_cookies(callback.headers(), LOGIN_COOKIE),
        vec![format!(
            "{LOGIN_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0"
        )]
    );

    {
        let exchanges = harness.idp.exchanges.lock().unwrap();
        let (exchanged_code, verifier, redirect) = exchanges.last().unwrap();
        assert_eq!(exchanged_code, &code);
        assert_eq!(redirect, &format!("{ORIGIN}/api/campus/auth/callback"));
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier, &start.challenge);
    }

    let session = harness.session_from(callback).await;
    let me = harness.get("/api/campus/me", Some(&session)).await;
    assert_eq!(me.headers()[header::CACHE_CONTROL], "no-store");
    let text = body_text(me).await;
    assert!(!text.contains(&id_token), "id_token is never returned");
    assert!(!text.contains(id_token.split('.').nth(1).unwrap()));
    let me: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(me["tenant_id"], "tenant-a");
    assert_eq!(me["identity"]["subject"], "student-1");
    assert_eq!(me["identity"]["email"], "student-1@campus.test");
    assert_eq!(me["is_tenant_admin"], false);
    assert_eq!(me["csrf_token"], session.csrf);
}

#[tokio::test]
async fn login_state_cannot_be_replayed_and_expires() {
    let harness = Harness::new().await;
    let start = harness.start_login().await;
    let first = harness
        .key
        .sign(&base_claims("s1", &start.nonce, harness.now()));
    let code = harness.code_for(&start, first);
    let ok = harness
        .callback(&start.state, &code, Some(&start.login_cookie))
        .await;
    assert_eq!(ok.status(), StatusCode::SEE_OTHER);

    let again = harness
        .key
        .sign(&base_claims("s1", &start.nonce, harness.now()));
    let replay_code = harness.code_for(&start, again);
    let replay = harness
        .callback(&start.state, &replay_code, Some(&start.login_cookie))
        .await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert!(issued_cookie(replay.headers(), SESSION_COOKIE).is_none());
    assert_eq!(
        body_json(replay).await["error"],
        "login attempt was already used"
    );
    assert_eq!(
        harness.idp.exchanges.lock().unwrap().len(),
        1,
        "a replayed state never redeems a code"
    );

    let expired = harness.start_login().await;
    harness.clock.advance(601);
    let token = harness
        .key
        .sign(&base_claims("s1", &expired.nonce, harness.now()));
    let code = harness.code_for(&expired, token);
    let response = harness
        .callback(&expired.state, &code, Some(&expired.login_cookie))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(issued_cookie(response.headers(), SESSION_COOKIE).is_none());
    assert_eq!(body_json(response).await["error"], "login attempt expired");
}

#[tokio::test]
async fn login_must_finish_in_the_browser_that_started_it() {
    let harness = Harness::new().await;
    let attacker = harness.start_login().await;
    let victim = harness.start_login().await;
    let token = harness
        .key
        .sign(&base_claims("victim", &victim.nonce, harness.now()));
    let code = harness.code_for(&victim, token);
    for cookies in [
        None,
        Some(attacker.login_cookie.clone()),
        Some(format!("{LOGIN_COOKIE}=short")),
        Some(format!(
            "{}; {}",
            victim.login_cookie, attacker.login_cookie
        )),
    ] {
        let response = harness
            .callback(&victim.state, &code, cookies.as_deref())
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{cookies:?}");
        assert!(issued_cookie(response.headers(), SESSION_COOKIE).is_none());
        assert_eq!(
            body_json(response).await["error"],
            "login attempt was not started by this browser"
        );
    }
    assert!(
        harness.idp.exchanges.lock().unwrap().is_empty(),
        "no code is redeemed without the matching sealed cookie"
    );
    // Without the cookie nobody can burn the state, so the browser that
    // started the login can still finish it.
    let response = harness
        .callback(&victim.state, &code, Some(&victim.login_cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

fn flip_bit(cookie: &str, index: usize) -> String {
    let (name, value) = cookie.split_once('=').unwrap();
    let mut raw = URL_SAFE_NO_PAD.decode(value).unwrap();
    raw[index] ^= 0x01;
    format!("{name}={}", b64(&raw))
}

#[tokio::test]
async fn tampered_and_foreign_login_cookies_are_rejected() {
    let harness = Harness::new().await;
    let start = harness.start_login().await;
    let raw_len = URL_SAFE_NO_PAD
        .decode(start.login_cookie.split_once('=').unwrap().1)
        .unwrap()
        .len();
    let token = harness
        .key
        .sign(&base_claims("s1", &start.nonce, harness.now()));
    let code = harness.code_for(&start, token);
    // Version byte, AEAD nonce, ciphertext and tag.
    for index in [0, 1, 12, 13, 40, raw_len - 17, raw_len - 1] {
        let response = harness
            .callback(
                &start.state,
                &code,
                Some(&flip_bit(&start.login_cookie, index)),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "byte {index}");
        assert_eq!(
            body_json(response).await["error"],
            "login attempt was not started by this browser"
        );
    }

    // A cookie the attacker seals for a state of their choosing, under any
    // key other than the deployment's, or for another tenant or issuer.
    let now = harness.now();
    let forged = LoginAttempt {
        state: "F".repeat(43),
        nonce: "N".repeat(43),
        pkce_verifier: "V".repeat(43),
        issued_at: now,
        expires_at: now + 600,
    };
    let real_key = *harness.config.login_cookie_key.expose_secret();
    for (label, cookie) in [
        (
            "different key",
            harness.seal_login_cookie(&random_login_cookie_key(), TENANT, ISSUER, &forged),
        ),
        (
            "other tenant",
            harness.seal_login_cookie(&real_key, "tenant-b", ISSUER, &forged),
        ),
        (
            "other issuer",
            harness.seal_login_cookie(&real_key, TENANT, "https://evil.test", &forged),
        ),
    ] {
        let response = harness.callback(&forged.state, "c", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{label}");
        assert_eq!(
            body_json(response).await["error"],
            "login attempt was not started by this browser",
            "{label}"
        );
    }
    assert!(harness.idp.exchanges.lock().unwrap().is_empty());

    // Control: the same forgery sealed under the deployment key reaches the
    // token exchange, so the rejections above came from the seal.
    let control = harness.seal_login_cookie(&real_key, TENANT, ISSUER, &forged);
    let response = harness.callback(&forged.state, "c", Some(&control)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(harness.idp.exchanges.lock().unwrap().len(), 1);

    // The untampered cookie still completes the original login.
    let response = harness
        .callback(&start.state, &code, Some(&start.login_cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn login_writes_nothing_to_the_database() {
    let harness = Harness::on_disk().await;
    let before = harness.row_counts().await;
    for _ in 0..25 {
        let response = harness.get("/api/campus/auth/login", None).await;
        assert_eq!(response.status(), StatusCode::FOUND);
    }
    assert_eq!(harness.row_counts().await, before);

    // Control: the counter does see the callback's single-use record.
    let session = harness.login("s1").await;
    let after = harness.row_counts().await;
    assert_eq!(after["campus_consumed_login_states"], 1);
    assert_eq!(after["campus_sessions"], before["campus_sessions"] + 1);
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn idp_errors_mixups_and_bad_tokens_never_create_sessions() {
    let harness = Harness::new().await;
    for query in [
        "error=access_denied&state=x",
        "code=c",
        "state=s",
        "code=c&state=not-a-state",
    ] {
        let response = harness
            .get(&format!("/api/campus/auth/callback?{query}"), None)
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }

    let start = harness.start_login().await;
    let response = harness
        .send(
            request(
                "GET",
                &format!(
                    "/api/campus/auth/callback?code=c&state={}&iss=https%3A%2F%2Fevil.test",
                    start.state
                ),
            )
            .header(header::COOKIE, &start.login_cookie)
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let now = harness.now();
    let bad_claims = [
        json!({"iss": "https://evil.test"}),
        json!({"aud": "other"}),
        json!({"exp": now - 3600}),
        json!({"nonce": "wrong"}),
    ];
    for overrides in bad_claims {
        let response = harness
            .login_response(|start| {
                let mut claims = base_claims("s1", &start.nonce, now);
                for (name, value) in overrides.as_object().unwrap() {
                    claims[name] = value.clone();
                }
                claims
            })
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{overrides}");
        assert!(issued_cookie(response.headers(), SESSION_COOKIE).is_none());
    }

    // The IdP rejects a code redeemed without the matching PKCE verifier.
    let start = harness.start_login().await;
    let other = harness.start_login().await;
    let token = harness.key.sign(&base_claims("s1", &start.nonce, now));
    let code = harness.code_for(&other, token);
    let response = harness
        .callback(&start.state, &code, Some(&start.login_cookie))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sessions_expire_when_idle() {
    let harness = Harness::new().await;
    let session = harness.login("s1").await;
    harness.clock.advance(29 * 60);
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::OK
    );
    harness.clock.advance(30 * 60);
    let response = harness.get("/api/campus/me", Some(&session)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        set_cookies(response.headers(), SESSION_COOKIE),
        vec![format!(
            "{SESSION_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0"
        )]
    );
}

#[tokio::test]
async fn sessions_expire_absolutely_despite_activity() {
    let harness = Harness::new().await;
    let session = harness.login("s1").await;
    let mut elapsed = 0;
    while elapsed + 25 * 60 < 8 * 60 * 60 {
        harness.clock.advance(25 * 60);
        elapsed += 25 * 60;
        assert_eq!(
            harness.get("/api/campus/me", Some(&session)).await.status(),
            StatusCode::OK,
            "active at {elapsed}s"
        );
    }
    harness.clock.advance(8 * 60 * 60 - elapsed);
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn logout_revokes_server_side_and_requires_csrf() {
    let harness = Harness::new().await;
    let session = harness.login("s1").await;
    let no_csrf = harness
        .send(
            request("POST", "/api/campus/auth/logout")
                .header(header::COOKIE, &session.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::OK
    );

    let response = harness
        .mutate("POST", "/api/campus/auth/logout", &session, json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        set_cookies(response.headers(), SESSION_COOKIE),
        vec![format!(
            "{SESSION_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0"
        )]
    );
    // Replaying the old cookie after logout fails.
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(!harness
        .store
        .revoke_campus_session(&session_hash(&session), harness.now())
        .await
        .unwrap());
}

#[tokio::test]
async fn a_new_login_revokes_the_session_the_browser_presented() {
    let harness = Harness::new().await;
    let first = harness.login("s1").await;
    let start = harness.start_login().await;
    let token = harness
        .key
        .sign(&base_claims("s1", &start.nonce, harness.now()));
    let code = harness.code_for(&start, token);
    let cookies = format!("{}; {}", start.login_cookie, first.cookie);
    let response = harness.callback(&start.state, &code, Some(&cookies)).await;
    let second = harness.session_from(response).await;
    assert_ne!(first.cookie, second.cookie);
    assert_ne!(first.csrf, second.csrf);
    assert_eq!(
        harness.get("/api/campus/me", Some(&first)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn state_changes_require_the_session_csrf_token_and_campus_origin() {
    let harness = Harness::new().await;
    harness.make_admins("tenant-a", &["admin"]).await;
    let admin = harness.login("admin").await;
    let other = harness.login("other").await;
    let path = "/api/campus/courses/cs101/members";
    let body = json!({"issuer": super::ISSUER, "subject": "student", "role": "student"});

    let put = |csrf: Option<&str>, origin: Option<&str>, fetch_site: Option<&str>| {
        let mut builder = request("PUT", path)
            .header(header::COOKIE, &admin.cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(csrf) = csrf {
            builder = builder.header("x-campus-csrf", csrf);
        }
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if let Some(site) = fetch_site {
            builder = builder.header("sec-fetch-site", site);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    };
    let wrong_csrf = "A".repeat(43);
    for (label, request) in [
        ("missing csrf", put(None, Some(ORIGIN), None)),
        ("wrong csrf", put(Some(&wrong_csrf), Some(ORIGIN), None)),
        (
            "another session's csrf",
            put(Some(&other.csrf), Some(ORIGIN), None),
        ),
        ("malformed csrf", put(Some("short"), Some(ORIGIN), None)),
        (
            "foreign origin",
            put(Some(&admin.csrf), Some("https://evil.test"), None),
        ),
        ("null origin", put(Some(&admin.csrf), Some("null"), None)),
        (
            "lookalike origin",
            put(
                Some(&admin.csrf),
                Some("https://campus.pharness.test.evil.test"),
                None,
            ),
        ),
        (
            "cross-site fetch",
            put(Some(&admin.csrf), None, Some("cross-site")),
        ),
    ] {
        let response = harness.send(request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{label}");
    }
    assert!(harness
        .store
        .list_campus_course_members("tenant-a", "cs101")
        .await
        .unwrap()
        .is_empty());

    let response = harness
        .send(put(Some(&admin.csrf), None, Some("same-origin")))
        .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "same-origin without Origin header"
    );
    let response = harness
        .send(put(Some(&admin.csrf), Some(ORIGIN), None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["created"], false);
}

#[tokio::test]
async fn unauthenticated_requests_never_touch_session_state() {
    let harness = Harness::on_disk().await;
    let real = harness.login("s1").await;
    let before = harness.row_counts().await;
    assert_eq!(before["campus_sessions"], 1);
    let forged = Session {
        cookie: format!("{SESSION_COOKIE}={}", "B".repeat(43)),
        csrf: "B".repeat(43),
    };
    let mut responses = Vec::new();
    for path in ["/api/campus/me", "/api/campus/courses/cs101"] {
        responses.push(harness.get(path, Some(&forged)).await);
    }
    responses.push(
        harness
            .mutate("POST", "/api/campus/auth/logout", &forged, json!({}))
            .await,
    );
    // A forged cookie carrying the real session's CSRF token.
    let borrowed = Session {
        csrf: real.csrf.clone(),
        ..forged.clone()
    };
    responses.push(
        harness
            .mutate("POST", "/api/campus/auth/logout", &borrowed, json!({}))
            .await,
    );
    for response in responses {
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(
            issued_cookie(response.headers(), SESSION_COOKIE).is_none(),
            "no session cookie is issued"
        );
        assert!(set_cookies(response.headers(), SESSION_COOKIE)
            .iter()
            .all(|cookie| cookie.starts_with(&format!("{SESSION_COOKIE}=;"))));
    }
    assert_eq!(harness.row_counts().await, before, "no row was written");
    assert_eq!(
        harness.get("/api/campus/me", Some(&real)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_rejected_csrf_request_does_not_extend_the_session() {
    let harness = Harness::new().await;
    let session = harness.login("s1").await;
    harness.clock.advance(20 * 60);
    for csrf in [None, Some("A".repeat(43))] {
        let mut builder = request("POST", "/api/campus/auth/logout")
            .header(header::COOKIE, &session.cookie)
            .header(header::ORIGIN, ORIGIN);
        if let Some(csrf) = csrf {
            builder = builder.header("x-campus-csrf", csrf);
        }
        let response = harness.send(builder.body(Body::empty()).unwrap()).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = harness
        .send(
            request("POST", "/api/campus/auth/logout")
                .header(header::COOKIE, &session.cookie)
                .header("x-campus-csrf", &session.csrf)
                .header(header::ORIGIN, "https://evil.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // 31 minutes after the last accepted request the idle window has closed;
    // the rejected requests at minute 20 did not extend it.
    harness.clock.advance(11 * 60);
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_non_ascii_cookie_from_a_sibling_domain_does_not_lock_users_out() {
    let harness = Harness::new().await;
    let session = harness.login("s1").await;
    let mut header_value = b"theme=caf\xc3\xa9; ".to_vec();
    header_value.extend_from_slice(session.cookie.as_bytes());
    header_value.extend_from_slice(b"; lang=\xff\xfe");
    let header_value = HeaderValue::from_bytes(&header_value).unwrap();
    assert!(
        header_value.to_str().is_err(),
        "the header is not visible ASCII"
    );
    let response = harness
        .send(
            request("GET", "/api/campus/me")
                .header(header::COOKIE, header_value.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // Duplicate session cookies are still rejected around non-ASCII pairs.
    let mut duplicated = header_value.as_bytes().to_vec();
    duplicated.extend_from_slice(format!("; {SESSION_COOKIE}={}", "C".repeat(43)).as_bytes());
    let response = harness
        .send(
            request("GET", "/api/campus/me")
                .header(
                    header::COOKIE,
                    HeaderValue::from_bytes(&duplicated).unwrap(),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // A login still completes with a non-ASCII sibling cookie present.
    let start = harness.start_login().await;
    let token = harness
        .key
        .sign(&base_claims("s2", &start.nonce, harness.now()));
    let code = harness.code_for(&start, token);
    let mut cookies = b"theme=caf\xc3\xa9; ".to_vec();
    cookies.extend_from_slice(start.login_cookie.as_bytes());
    let response = harness
        .send(
            request(
                "GET",
                &format!(
                    "/api/campus/auth/callback?code={code}&state={}",
                    start.state
                ),
            )
            .header(header::COOKIE, HeaderValue::from_bytes(&cookies).unwrap())
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}
