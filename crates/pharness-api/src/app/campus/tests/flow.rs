use super::super::session::{LOGIN_COOKIE, SESSION_COOKIE};
use super::super::state::CampusMount;
use super::{
    base_claims, body_json, body_text, full_app, issued_cookie, request, send, session_hash,
    set_cookies, Harness, Session, CLIENT_ID, ORIGIN,
};
use axum::body::Body;
use axum::http::{header, StatusCode};
use pharness_store::SqliteStore;
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
    for cookies in [
        None,
        Some(attacker.login_cookie.clone()),
        Some(format!("{LOGIN_COOKIE}=short")),
    ] {
        let victim = harness.start_login().await;
        let token = harness
            .key
            .sign(&base_claims("attacker", &victim.nonce, harness.now()));
        let code = harness.code_for(&victim, token);
        let response = harness
            .callback(&victim.state, &code, cookies.as_deref())
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{cookies:?}");
        assert!(issued_cookie(response.headers(), SESSION_COOKIE).is_none());
        assert_eq!(
            body_json(response).await["error"],
            "login attempt was not started by this browser"
        );
        // The failed attempt was consumed, so the right cookie no longer helps.
        let response = harness
            .callback(&victim.state, &code, Some(&victim.login_cookie))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(issued_cookie(response.headers(), SESSION_COOKIE).is_none());
    }
    assert!(
        harness.idp.exchanges.lock().unwrap().is_empty(),
        "no code is redeemed for an unbound attempt"
    );

    let duplicate = harness.start_login().await;
    let both = format!("{}; {}", duplicate.login_cookie, attacker.login_cookie);
    let response = harness.callback(&duplicate.state, "c", Some(&both)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
    let harness = Harness::new().await;
    let forged = Session {
        cookie: format!("{SESSION_COOKIE}={}", "B".repeat(43)),
        csrf: "B".repeat(43),
    };
    for path in ["/api/campus/me", "/api/campus/courses/cs101"] {
        assert_eq!(
            harness.get(path, Some(&forged)).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
