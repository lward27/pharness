use super::super::session::SESSION_COOKIE;
use super::{
    body_json, request, Harness, Session, ISSUER, OPERATOR_TOKEN, ORIGIN, TENANT, WORKER_TOKEN,
};
use axum::body::Body;
use axum::http::{header, Method, StatusCode};
use pharness_store::{
    CampusCourseRole, CampusIdentityKey, CreateCampusSession, UpsertCampusIdentity,
};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CampusRouteClass {
    /// Public entry point; creates only a one-time login attempt.
    Login,
    /// Public; guarded by state, browser binding and id_token validation.
    Callback,
    /// Requires a valid Campus session (and CSRF when state-changing).
    Session,
}

/// Every Campus route. Adding a route without listing it here fails
/// `campus_route_inventory_matches_source_registrations`.
const CAMPUS_ROUTES: &[(&str, &str, CampusRouteClass)] = &[
    ("GET", "/api/campus/auth/login", CampusRouteClass::Login),
    (
        "GET",
        "/api/campus/auth/callback",
        CampusRouteClass::Callback,
    ),
    ("POST", "/api/campus/auth/logout", CampusRouteClass::Session),
    ("GET", "/api/campus/me", CampusRouteClass::Session),
    (
        "GET",
        "/api/campus/courses/:course_id",
        CampusRouteClass::Session,
    ),
    (
        "GET",
        "/api/campus/courses/:course_id/members",
        CampusRouteClass::Session,
    ),
    (
        "PUT",
        "/api/campus/courses/:course_id/members",
        CampusRouteClass::Session,
    ),
    (
        "DELETE",
        "/api/campus/courses/:course_id/members",
        CampusRouteClass::Session,
    ),
];

const OPERATOR_INVENTORY: &str = include_str!("../../../../tests/fixtures/app-route-inventory.tsv");

fn materialize(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with(':') {
                "cs101"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// `(method, path)` pairs registered with `.route(...)` in the Campus router.
fn campus_routes_in_source() -> Vec<(String, String)> {
    let source = include_str!("../mod.rs");
    let mut routes = Vec::new();
    let mut remaining = source;
    while let Some(offset) = remaining.find(".route(") {
        remaining = remaining[offset + ".route(".len()..].trim_start();
        let Some(quoted) = remaining.strip_prefix('"') else {
            continue;
        };
        remaining = quoted;
        let end = remaining.find('"').unwrap();
        let path = &remaining[..end];
        let mut depth = 1_i32;
        let mut body_end = 0;
        for (index, character) in remaining.char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = index;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &remaining[end..body_end];
        // `any(` registrations are the closed-router catch-alls used when
        // Campus is disabled or unavailable; they serve only errors.
        if !path.starts_with("/api/campus/") || body.contains("any(") {
            continue;
        }
        for (needle, method) in [
            ("get(", "GET"),
            ("post(", "POST"),
            ("put(", "PUT"),
            ("patch(", "PATCH"),
            ("delete(", "DELETE"),
        ] {
            if body.contains(needle) {
                routes.push((method.to_string(), path.to_string()));
            }
        }
    }
    routes.sort();
    routes
}

#[test]
fn campus_route_inventory_matches_source_registrations() {
    let mut expected = CAMPUS_ROUTES
        .iter()
        .map(|(method, path, _)| (method.to_string(), path.to_string()))
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(campus_routes_in_source(), expected);
    assert!(
        !OPERATOR_INVENTORY.contains("/api/campus"),
        "campus routes must not appear in the operator inventory"
    );
}

#[tokio::test]
async fn every_campus_route_fails_closed_without_a_valid_session() {
    let harness = Harness::new().await;
    let session = harness.login("student").await;
    let credentials: Vec<(&str, Vec<(header::HeaderName, String)>)> = vec![
        ("no credentials", vec![]),
        (
            "operator bearer token",
            vec![(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))],
        ),
        (
            "worker bearer token",
            vec![(header::AUTHORIZATION, format!("Bearer {WORKER_TOKEN}"))],
        ),
        (
            "malformed session cookie",
            vec![(header::COOKIE, format!("{SESSION_COOKIE}=not-a-token"))],
        ),
        (
            "unknown session cookie",
            vec![(
                header::COOKIE,
                format!("{SESSION_COOKIE}={}", "C".repeat(43)),
            )],
        ),
        (
            "duplicate session cookies",
            vec![(
                header::COOKIE,
                format!("{}; {SESSION_COOKIE}={}", session.cookie, "C".repeat(43)),
            )],
        ),
        (
            "session id in another cookie name",
            vec![(
                header::COOKIE,
                format!(
                    "pharness-campus={}",
                    session.cookie.split_once('=').unwrap().1
                ),
            )],
        ),
    ];

    for (method, path, class) in CAMPUS_ROUTES {
        let uri = materialize(path);
        match class {
            CampusRouteClass::Login => {
                let response = harness.get(&uri, None).await;
                assert_eq!(response.status(), StatusCode::FOUND);
                assert!(response
                    .headers()
                    .get_all(header::SET_COOKIE)
                    .iter()
                    .all(|cookie| !cookie
                        .to_str()
                        .unwrap()
                        .starts_with(&format!("{SESSION_COOKIE}="))));
            }
            CampusRouteClass::Callback => {
                for (label, headers) in &credentials {
                    let mut builder = request(method, &uri);
                    for (name, value) in headers {
                        builder = builder.header(name, value);
                    }
                    let response = harness.send(builder.body(Body::empty()).unwrap()).await;
                    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{label}");
                }
            }
            CampusRouteClass::Session => {
                for (label, headers) in &credentials {
                    let mut builder = request(method, &uri)
                        .header("x-campus-csrf", &session.csrf)
                        .header(header::ORIGIN, ORIGIN)
                        .header(header::CONTENT_TYPE, "application/json");
                    for (name, value) in headers {
                        builder = builder.header(name, value);
                    }
                    let body = json!({"issuer": ISSUER, "subject": "x", "role": "student"});
                    let response = harness
                        .send(builder.body(Body::from(body.to_string())).unwrap())
                        .await;
                    assert_eq!(
                        response.status(),
                        StatusCode::UNAUTHORIZED,
                        "{method} {path} with {label}"
                    );
                }
                let state_changing =
                    !matches!(Method::from_bytes(method.as_bytes()).unwrap(), Method::GET);
                if state_changing {
                    let response = harness
                        .send(
                            request(method, &uri)
                                .header(header::COOKIE, &session.cookie)
                                .header(header::CONTENT_TYPE, "application/json")
                                .body(Body::from("{}"))
                                .unwrap(),
                        )
                        .await;
                    assert_eq!(
                        response.status(),
                        StatusCode::FORBIDDEN,
                        "{method} {path} without CSRF"
                    );
                }
            }
        }
    }
    // Nothing above revoked or replaced the real session.
    assert_eq!(
        harness.get("/api/campus/me", Some(&session)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn campus_sessions_never_authorize_operator_sdlc_or_internal_routes() {
    let harness = Harness::new().await;
    harness.make_admins(TENANT, &["admin"]).await;
    let admin = harness.login("admin").await;
    let mut checked = 0;
    for line in OPERATOR_INVENTORY
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
    {
        let columns = line.split('\t').collect::<Vec<_>>();
        let (method, path, class) = (columns[0], columns[1], columns[2]);
        if class == "open" {
            continue;
        }
        let response = harness
            .send(
                request(method, &materialize(path))
                    .header(header::COOKIE, &admin.cookie)
                    .header("x-campus-csrf", &admin.csrf)
                    .header(header::ORIGIN, ORIGIN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path} ({class}) accepted a campus session"
        );
        checked += 1;
    }
    assert!(checked > 200, "the operator inventory was exercised");

    // The operator token itself still works on operator routes.
    let response = harness
        .send(
            request("GET", "/api/runs")
                .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
}

async fn status(harness: &Harness, path: &str, session: &Session) -> StatusCode {
    harness.get(path, Some(session)).await.status()
}

#[tokio::test]
async fn course_access_is_membership_gated_and_cross_course_denied() {
    let harness = Harness::new().await;
    harness.enroll(TENANT, "cs101", "student", "student").await;
    harness
        .enroll(TENANT, "cs101", "teacher", "instructor")
        .await;
    harness
        .enroll(TENANT, "cs102", "other-student", "student")
        .await;
    harness.make_admins(TENANT, &["admin"]).await;
    let student = harness.login("student").await;
    let teacher = harness.login("teacher").await;
    let outsider = harness.login("outsider").await;
    let admin = harness.login("admin").await;

    let cases = [
        (&student, "/api/campus/courses/cs101", StatusCode::OK),
        (&student, "/api/campus/courses/cs102", StatusCode::FORBIDDEN),
        (
            &student,
            "/api/campus/courses/cs101/members",
            StatusCode::FORBIDDEN,
        ),
        (&teacher, "/api/campus/courses/cs101", StatusCode::OK),
        (
            &teacher,
            "/api/campus/courses/cs101/members",
            StatusCode::OK,
        ),
        (&teacher, "/api/campus/courses/cs102", StatusCode::FORBIDDEN),
        (
            &teacher,
            "/api/campus/courses/cs102/members",
            StatusCode::FORBIDDEN,
        ),
        (
            &outsider,
            "/api/campus/courses/cs101",
            StatusCode::FORBIDDEN,
        ),
        (
            &outsider,
            "/api/campus/courses/missing",
            StatusCode::FORBIDDEN,
        ),
        (&admin, "/api/campus/courses/cs102/members", StatusCode::OK),
        (&admin, "/api/campus/courses/missing", StatusCode::NOT_FOUND),
        (
            &admin,
            "/api/campus/courses/bad%20id",
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (session, path, expected) in cases {
        assert_eq!(status(&harness, path, session).await, expected, "{path}");
    }

    let roster = body_json(
        harness
            .get("/api/campus/courses/cs101/members", Some(&teacher))
            .await,
    )
    .await;
    let members = roster["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    assert!(members
        .iter()
        .any(|member| member["subject"] == "student" && member["email"] == "student@campus.test"));

    let body = json!({"issuer": ISSUER, "subject": "new", "role": "student"});
    for session in [&student, &teacher, &outsider] {
        let response = harness
            .mutate(
                "PUT",
                "/api/campus/courses/cs101/members",
                session,
                body.clone(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = harness
            .mutate(
                "DELETE",
                "/api/campus/courses/cs101/members",
                session,
                json!({"issuer": ISSUER, "subject": "student"}),
            )
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = harness
        .mutate("PUT", "/api/campus/courses/cs101/members", &admin, body)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["created"], true);
}

#[tokio::test]
async fn tenant_scope_is_taken_from_the_session_only() {
    let harness = Harness::new().await;
    // Grants in another tenant for the same issuer and subject are invisible.
    harness
        .enroll("tenant-b", "cs999", "student", "instructor")
        .await;
    harness.make_admins("tenant-b", &["student"]).await;
    let student = harness.login("student").await;
    let me = body_json(harness.get("/api/campus/me", Some(&student)).await).await;
    assert_eq!(me["tenant_id"], TENANT);
    assert_eq!(me["is_tenant_admin"], false);
    assert_eq!(me["memberships"], json!([]));
    assert_eq!(
        status(&harness, "/api/campus/courses/cs999", &student).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&harness, "/api/campus/courses/cs999/members", &student).await,
        StatusCode::FORBIDDEN
    );

    // A tenant-a admin cannot reach into tenant-b through the request body.
    harness.make_admins(TENANT, &["admin"]).await;
    let admin = harness.login("admin").await;
    for body in [
        json!({"issuer": ISSUER, "subject": "student", "role": "student", "tenant_id": "tenant-b"}),
        json!({"issuer": ISSUER, "subject": "student", "role": "student", "tenant": "tenant-b"}),
        json!({"issuer": ISSUER, "subject": "student", "role": "student", "actor": "someone"}),
        json!({"issuer": ISSUER, "subject": "student", "role": "student", "email": "student@campus.test"}),
        json!({"issuer": ISSUER, "subject": "student", "role": "admin"}),
    ] {
        let response = harness
            .mutate(
                "PUT",
                "/api/campus/courses/cs999/members",
                &admin,
                body.clone(),
            )
            .await;
        assert!(response.status().is_client_error(), "{body}");
    }
    let tenant_b_key = CampusIdentityKey {
        tenant_id: "tenant-b".into(),
        issuer: ISSUER.into(),
        subject: "student".into(),
    };
    assert_eq!(
        harness
            .store
            .get_campus_course_role(&tenant_b_key, "cs999")
            .await
            .unwrap(),
        Some(CampusCourseRole::Instructor),
        "tenant-b grant unchanged"
    );
    assert!(harness
        .store
        .list_campus_course_members(TENANT, "cs999")
        .await
        .unwrap()
        .is_empty());

    let response = harness
        .mutate(
            "PUT",
            "/api/campus/courses/cs999/members",
            &admin,
            json!({"issuer": "https://evil.test", "subject": "student", "role": "student"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A correctly formed request lands in the session's tenant.
    let response = harness
        .mutate(
            "PUT",
            "/api/campus/courses/cs999/members",
            &admin,
            json!({"issuer": ISSUER, "subject": "student", "role": "student"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let tenant_a_key = CampusIdentityKey {
        tenant_id: TENANT.into(),
        ..tenant_b_key.clone()
    };
    assert_eq!(
        harness
            .store
            .get_campus_course_role(&tenant_a_key, "cs999")
            .await
            .unwrap(),
        Some(CampusCourseRole::Student)
    );
}

#[tokio::test]
async fn a_session_row_for_another_tenant_is_rejected() {
    let harness = Harness::new().await;
    let identity = harness
        .store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: "cid_tenant_b".into(),
            key: CampusIdentityKey {
                tenant_id: "tenant-b".into(),
                issuer: ISSUER.into(),
                subject: "student".into(),
            },
            email: None,
            display_name: None,
            now: harness.now(),
        })
        .await
        .unwrap();
    let token = "D".repeat(43);
    harness
        .store
        .create_campus_session(CreateCampusSession {
            id_hash: super::super::tokens::sha256_hex(&token),
            tenant_id: "tenant-b".into(),
            identity_id: identity.id,
            csrf_hash: "unused".into(),
            now: harness.now(),
            idle_expires_at: harness.now() + 600,
            absolute_expires_at: harness.now() + 600,
        })
        .await
        .unwrap();
    let session = Session {
        cookie: format!("{SESSION_COOKIE}={token}"),
        csrf: String::new(),
    };
    assert_eq!(
        status(&harness, "/api/campus/me", &session).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn role_changes_take_effect_on_the_next_request() {
    let harness = Harness::new().await;
    harness.make_admins(TENANT, &["admin"]).await;
    harness.enroll(TENANT, "cs101", "student", "student").await;
    let student = harness.login("student").await;
    let admin = harness.login("admin").await;
    let roster = "/api/campus/courses/cs101/members";
    assert_eq!(
        status(&harness, roster, &student).await,
        StatusCode::FORBIDDEN
    );

    let promote = json!({"issuer": ISSUER, "subject": "student", "role": "instructor"});
    assert_eq!(
        harness
            .mutate("PUT", roster, &admin, promote)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(status(&harness, roster, &student).await, StatusCode::OK);

    let remove = json!({"issuer": ISSUER, "subject": "student"});
    let response = harness.mutate("DELETE", roster, &admin, remove).await;
    assert_eq!(body_json(response).await["removed"], true);
    assert_eq!(
        status(&harness, "/api/campus/courses/cs101", &student).await,
        StatusCode::FORBIDDEN
    );

    // Revoking administration also applies to the existing admin session.
    harness.make_admins(TENANT, &[]).await;
    let promote = json!({"issuer": ISSUER, "subject": "student", "role": "student"});
    assert_eq!(
        harness
            .mutate("PUT", roster, &admin, promote)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn email_is_display_only_and_never_selects_an_identity() {
    let harness = Harness::new().await;
    harness.enroll(TENANT, "cs101", "sub-1", "student").await;
    let first = harness.login_with("sub-1", "alice@campus.test").await;
    let first_me = body_json(harness.get("/api/campus/me", Some(&first)).await).await;

    let changed = harness.login_with("sub-1", "alice.new@campus.test").await;
    let changed_me = body_json(harness.get("/api/campus/me", Some(&changed)).await).await;
    assert_eq!(changed_me["identity"]["id"], first_me["identity"]["id"]);
    assert_eq!(changed_me["identity"]["email"], "alice.new@campus.test");
    assert_eq!(changed_me["memberships"], first_me["memberships"]);

    // A different subject that claims the old email is a different person.
    let impostor = harness.login_with("sub-2", "alice@campus.test").await;
    let impostor_me = body_json(harness.get("/api/campus/me", Some(&impostor)).await).await;
    assert_ne!(impostor_me["identity"]["id"], first_me["identity"]["id"]);
    assert_eq!(impostor_me["memberships"], json!([]));
    assert_eq!(
        status(&harness, "/api/campus/courses/cs101", &impostor).await,
        StatusCode::FORBIDDEN
    );
}
