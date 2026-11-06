//! Campus lab launch, status, stop and reset through the HTTP boundary, on
//! the fake execution provider and a local journal.

use super::super::lab_catalog::{
    load_campus_labs, CampusLabCatalog, CampusLabProviderKind, CAMPUS_LAB_CATALOG_SCHEMA,
    ENV_LABS_FILE, ENV_LAB_PROVIDER,
};
use super::{body_json, request, send, test_config, Harness, Session, ISSUER, ORIGIN, TENANT};
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use pharness_core::campus_capability::{
    CampusAuthoritySource, CampusAuthorityStatus, CampusBrokerClock,
};
use pharness_core::{FakeFault, FakeFaultPoint};
use pharness_store::{
    retry_campus_lab_cleanup, CampusDriverContext, CampusIdentityKey, JournalCampusAuthoritySource,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const CODING: &str =
    include_str!("../../../../../pharness-core/src/campus_profile/fixtures/coding.json");
const RESEARCH: &str =
    include_str!("../../../../../pharness-core/src/campus_profile/fixtures/research.json");
const SECURITY: &str =
    include_str!("../../../../../pharness-core/src/campus_profile/fixtures/security.json");

fn layer(authority: &str) -> Value {
    json!({
        "schema_version": "pharness.dev/campus-policy-layer/v1alpha1",
        "authority": authority,
        "id": format!("{authority}-policy"),
        "version": "v1",
        "constraints": {},
        "mandatory_denies": {
            "tools": [], "egress_hosts": [], "model_aliases": [], "approval_kinds": []
        },
    })
}

fn profile(document: &str) -> Value {
    serde_json::from_str(document).unwrap()
}

/// A coding profile allowed to run for four hours.
fn long_profile() -> Value {
    let mut long = profile(CODING);
    long["id"] = json!("campus-long");
    long["ttl_seconds"] = json!(14_400);
    long
}

fn assignment(course: &str, assignment: &str, profile_id: &str) -> Value {
    json!({
        "course_id": course,
        "assignment_id": assignment,
        "title": format!("{course} {assignment}"),
        "profile_id": profile_id,
    })
}

pub(super) fn catalog_value(lab_slots: i64) -> Value {
    json!({
        "schema_version": CAMPUS_LAB_CATALOG_SCHEMA,
        "lab_slots": lab_slots,
        "profiles": [profile(CODING), profile(RESEARCH), long_profile()],
        "policy_layers": [layer("platform"), layer("institution")],
        "assignments": [
            assignment("cs101", "lab-1", "campus-coding"),
            assignment("cs101", "lab-2", "campus-research"),
            assignment("cs101", "lab-long", "campus-long"),
            assignment("cs102", "lab-1", "campus-coding"),
        ],
    })
}

pub(super) fn parse(value: &Value) -> anyhow::Result<CampusLabCatalog> {
    CampusLabCatalog::parse(TENANT, &value.to_string())
}

pub(super) async fn labs_harness(lab_slots: i64) -> Harness {
    Harness::with_labs(parse(&catalog_value(lab_slots)).unwrap(), true, false).await
}

fn launch_path(course: &str, assignment: &str) -> String {
    format!("/api/campus/courses/{course}/assignments/{assignment}/launch")
}

pub(super) async fn launch(
    harness: &Harness,
    session: &Session,
    assignment: &str,
    body: Value,
) -> Response {
    harness
        .mutate("POST", &launch_path("cs101", assignment), session, body)
        .await
}

/// Launch and expect a new session; returns its view.
pub(super) async fn launched(harness: &Harness, session: &Session, assignment: &str) -> Value {
    let response = launch(harness, session, assignment, json!({})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response).await;
    assert_eq!(body["admission"], "created");
    body["session"].clone()
}

async fn view(harness: &Harness, session: &Session, id: &str) -> (StatusCode, Value) {
    let response = harness
        .get(&format!("/api/campus/lab-sessions/{id}"), Some(session))
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

async fn session_view(harness: &Harness, session: &Session, id: &str) -> Value {
    let (status, body) = view(harness, session, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["session"].clone()
}

pub(super) async fn act(harness: &Harness, session: &Session, id: &str, action: &str) -> Response {
    harness
        .mutate(
            "POST",
            &format!("/api/campus/lab-sessions/{id}/{action}"),
            session,
            json!({}),
        )
        .await
}

pub(super) fn id(view: &Value) -> String {
    view["session_id"].as_str().unwrap().to_string()
}

/// Nothing internal appears in a student-facing response.
fn assert_safe(body: &Value, secrets: &[&str]) {
    let text = body.to_string();
    for forbidden in [
        "workspace_id",
        "slot_id",
        "slot_generation",
        "budget_id",
        "failure_reason",
        "blocked_reason",
        "op_key",
        "fake-ws",
        "lab-slot",
        "image",
        "namespace",
    ]
    .iter()
    .chain(secrets)
    {
        assert!(!text.contains(forbidden), "{forbidden} leaked in {text}");
    }
}

pub(super) async fn enroll_students(harness: &Harness, course: &str, subjects: &[&str]) {
    for subject in subjects {
        harness.enroll(TENANT, course, subject, "student").await;
    }
}

#[tokio::test]
async fn launch_derives_identity_profile_and_bounds_and_rejects_client_fields() {
    let harness = Harness::with_labs(parse(&catalog_value(5)).unwrap(), true, true).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    harness.make_admins(TENANT, &["admin"]).await;
    let alice = harness.login("alice").await;
    let outsider = harness.login("outsider").await;
    let admin = harness.login("admin").await;

    let before = harness.row_counts().await;
    let smuggled = [
        json!({ "image": "docker.io/attacker/miner:latest" }),
        json!({ "namespace": "kube-system" }),
        json!({ "credentials": { "token": "stolen" } }),
        json!({ "service_account": "default" }),
        json!({ "profile_id": "campus-security" }),
        json!({ "profile": { "id": "campus-coding" } }),
        json!({ "tenant_id": "tenant-b" }),
        json!({ "subject": "bob", "issuer": ISSUER }),
        json!({ "expires_at": 4_102_444_800_i64 }),
        json!({ "ttl_seconds": 86_400 }),
        json!({ "resources": { "cpu_millicores": 64_000 } }),
        json!({ "idempotency_key": "k-1", "image": "evil" }),
        json!("not an object"),
    ];
    for body in &smuggled {
        let response = launch(&harness, &alice, "lab-1", body.clone()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let error = body_json(response).await;
        assert_eq!(error["code"], "invalid_request", "{body}");
        assert_safe(&error, &["attacker", "kube-system", "stolen"]);
    }
    let raw = harness
        .send(
            request("POST", &launch_path("cs101", "lab-1"))
                .header(header::COOKIE, &alice.cookie)
                .header("x-campus-csrf", &alice.csrf)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::from("{\"image\": \"evil\""))
                .unwrap(),
        )
        .await;
    assert_eq!(raw.status(), StatusCode::BAD_REQUEST);

    // Callers outside the course, and administrators who are not members,
    // cannot launch; neither can anyone launch an unknown assignment or one
    // of a course they are not in.
    for (session, course, assignment, expected) in [
        (&outsider, "cs101", "lab-1", StatusCode::FORBIDDEN),
        (&admin, "cs101", "lab-1", StatusCode::FORBIDDEN),
        (&alice, "cs102", "lab-1", StatusCode::FORBIDDEN),
        (&alice, "cs101", "missing", StatusCode::NOT_FOUND),
        (&alice, "cs101", "bad%20id", StatusCode::NOT_FOUND),
    ] {
        let response = harness
            .mutate("POST", &launch_path(course, assignment), session, json!({}))
            .await;
        assert_eq!(response.status(), expected, "{course}/{assignment}");
    }
    assert_eq!(
        harness.row_counts().await,
        before,
        "rejected launches wrote nothing"
    );

    let now = harness.now();
    let created = launched(&harness, &alice, "lab-1").await;
    assert_eq!(created["status"], "provisioning");
    assert_eq!(created["generation"], 1);
    assert_eq!(created["profile"]["id"], "campus-coding");
    assert_eq!(created["expires_at"], now + 5_400);
    assert_safe(&created, &[]);

    let labs = harness.labs.as_ref().unwrap();
    let expected = labs.catalog.assignment("cs101", "lab-1").unwrap();
    let record = harness
        .store
        .get_campus_lab_session_record(&id(&created))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.session.tenant_id, TENANT);
    assert_eq!(record.session.issuer, ISSUER);
    assert_eq!(record.session.subject, "alice");
    assert_eq!(record.session.profile(), *expected.policy.profile());
    assert_eq!(
        record.effective_policy_hash,
        expected.policy.effective_policy_hash()
    );
    assert!(record.session.created_by.starts_with("campus-identity:"));

    // The two-hour server-side cap applies even to a profile that allows
    // four hours.
    harness.reconcile().await;
    act(&harness, &alice, &id(&created), "stop").await;
    harness.reconcile().await;
    let long = launched(&harness, &alice, "lab-long").await;
    assert_eq!(long["expires_at"], harness.now() + 7_200);

    let assignments = body_json(
        harness
            .get("/api/campus/courses/cs101/assignments", Some(&alice))
            .await,
    )
    .await;
    let listed = assignments["assignments"].as_array().unwrap();
    assert_eq!(listed.len(), 3);
    let long_entry = listed
        .iter()
        .find(|entry| entry["assignment_id"] == "lab-long")
        .unwrap();
    assert_eq!(long_entry["session_seconds"], 7_200);
    assert_eq!(long_entry["launchable"], true);
    assert_eq!(long_entry["session"]["session_id"], long["session_id"]);
    assert_safe(&assignments, &[]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_and_repeated_launches_never_duplicate_or_exceed_slots() {
    let harness = Harness::with_labs(parse(&catalog_value(5)).unwrap(), true, true).await;
    let subjects = (0..9).map(|index| format!("s{index}")).collect::<Vec<_>>();
    let mut sessions = Vec::new();
    for subject in &subjects {
        harness.enroll(TENANT, "cs101", subject, "student").await;
        sessions.push((subject.clone(), harness.login(subject).await));
    }

    let mut tasks = Vec::new();
    for (subject, session) in &sessions {
        for attempt in 0..4 {
            let app = harness.app.clone();
            let session = session.clone();
            let subject = subject.clone();
            // Two retries share one key, one has no key and one targets
            // another assignment.
            let (assignment, body) = match attempt {
                0 | 1 => (
                    "lab-1",
                    json!({ "idempotency_key": format!("{subject}-launch") }),
                ),
                2 => ("lab-1", json!({})),
                _ => ("lab-2", json!({})),
            };
            tasks.push(tokio::spawn(async move {
                let response = send(
                    &app,
                    request("POST", &launch_path("cs101", assignment))
                        .header(header::COOKIE, &session.cookie)
                        .header("x-campus-csrf", &session.csrf)
                        .header(header::ORIGIN, ORIGIN)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await;
                let status = response.status();
                (subject, status, body_json(response).await)
            }));
        }
    }

    let mut created = 0;
    let mut per_subject = BTreeMap::<String, BTreeSet<String>>::new();
    for task in tasks {
        let (subject, status, body) = task.await.unwrap();
        match status {
            StatusCode::CREATED => {
                created += 1;
                per_subject
                    .entry(subject)
                    .or_default()
                    .insert(id(&body["session"]));
            }
            StatusCode::OK => {
                per_subject
                    .entry(subject)
                    .or_default()
                    .insert(id(&body["session"]));
            }
            StatusCode::SERVICE_UNAVAILABLE => {
                assert_eq!(body["code"], "no_slots_available", "{body}");
                assert_eq!(body["error"], "no lab slots are available; try again later");
            }
            StatusCode::CONFLICT => {
                assert_eq!(body["code"], "active_session_exists", "{body}");
                per_subject
                    .entry(subject)
                    .or_default()
                    .insert(body["session_id"].as_str().unwrap().to_string());
            }
            other => panic!("unexpected {other}: {body}"),
        }
    }
    assert_eq!(created, 5, "exactly the five slots were admitted");
    assert!(
        per_subject.values().all(|ids| ids.len() == 1),
        "no student got two sessions: {per_subject:?}"
    );
    assert_eq!(
        harness
            .store
            .count_live_campus_lab_sessions(TENANT)
            .await
            .unwrap(),
        5
    );
    assert_eq!(harness.row_counts().await["campus_sessions_journal"], 5);

    // Driving every admitted session never needs a sixth slot.
    harness.reconcile().await;
    assert_eq!(harness.provider.workspaces_created(), 5);
    let ready = harness
        .store
        .list_live_campus_lab_session_records_for_course(TENANT, "cs101", 50)
        .await
        .unwrap();
    assert_eq!(ready.len(), 5);
    assert!(ready
        .iter()
        .all(|record| record.session.state.grants_access()));
}

#[tokio::test]
async fn idempotency_keys_replay_and_conflicts_are_reported_safely() {
    let harness = labs_harness(5).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;

    let first = launch(
        &harness,
        &alice,
        "lab-1",
        json!({ "idempotency_key": "k-1" }),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = body_json(first).await["session"].clone();

    let replay = launch(
        &harness,
        &alice,
        "lab-1",
        json!({ "idempotency_key": "k-1" }),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay = body_json(replay).await;
    assert_eq!(replay["admission"], "replayed");
    assert_eq!(replay["session"]["session_id"], first["session_id"]);

    let header_replay = harness
        .send(
            request("POST", &launch_path("cs101", "lab-1"))
                .header(header::COOKIE, &alice.cookie)
                .header("x-campus-csrf", &alice.csrf)
                .header(header::ORIGIN, ORIGIN)
                .header("idempotency-key", "k-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(header_replay.status(), StatusCode::OK);
    assert_eq!(
        body_json(header_replay).await["session"]["session_id"],
        first["session_id"]
    );

    let existing = launch(&harness, &alice, "lab-1", json!({})).await;
    assert_eq!(existing.status(), StatusCode::OK);
    assert_eq!(body_json(existing).await["admission"], "existing");

    let reused = launch(
        &harness,
        &alice,
        "lab-2",
        json!({ "idempotency_key": "k-1" }),
    )
    .await;
    assert_eq!(reused.status(), StatusCode::CONFLICT);
    assert_eq!(body_json(reused).await["code"], "idempotency_key_reused");

    let other = launch(&harness, &alice, "lab-2", json!({})).await;
    assert_eq!(other.status(), StatusCode::CONFLICT);
    let other = body_json(other).await;
    assert_eq!(other["code"], "active_session_exists");
    assert_eq!(other["session_id"], first["session_id"]);

    for bad in ["", "has space", &"k".repeat(129), "semi;colon"] {
        let response = launch(&harness, &alice, "lab-1", json!({ "idempotency_key": bad })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bad:?}");
        assert_eq!(body_json(response).await["code"], "invalid_idempotency_key");
    }
    let mismatch = harness
        .send(
            request("POST", &launch_path("cs101", "lab-1"))
                .header(header::COOKIE, &alice.cookie)
                .header("x-campus-csrf", &alice.csrf)
                .header(header::ORIGIN, ORIGIN)
                .header("idempotency-key", "k-2")
                .body(Body::from(json!({ "idempotency_key": "k-3" }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        harness
            .store
            .count_live_campus_lab_sessions(TENANT)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn course_capacity_rejects_without_queueing() {
    let mut value = catalog_value(5);
    value["max_active_per_course"] = json!(2);
    let harness = Harness::with_labs(parse(&value).unwrap(), true, false).await;
    enroll_students(&harness, "cs101", &["a", "b", "c"]).await;
    enroll_students(&harness, "cs102", &["d"]).await;
    for subject in ["a", "b"] {
        let session = harness.login(subject).await;
        launched(&harness, &session, "lab-1").await;
    }
    let c = harness.login("c").await;
    let full = launch(&harness, &c, "lab-1", json!({})).await;
    assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(full).await["code"], "course_capacity_reached");
    // Nothing was queued: a later sweep admits nothing for c.
    harness.reconcile().await;
    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&c)).await).await;
    assert!(mine["sessions"].as_array().unwrap().is_empty());

    let d = harness.login("d").await;
    let response = harness
        .mutate("POST", &launch_path("cs102", "lab-1"), &d, json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn status_reports_provisioning_ready_failure_and_cleanup_blocked_safely() {
    let harness = labs_harness(5).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;

    harness
        .provider
        .inject(FakeFaultPoint::AfterProvision, FakeFault::InProgress);
    let first = launched(&harness, &alice, "lab-1").await;
    harness.reconcile().await;
    let provisioning = session_view(&harness, &alice, &id(&first)).await;
    assert_eq!(provisioning["status"], "provisioning");
    assert_eq!(provisioning["state"], "provisioning");
    assert_eq!(provisioning["error"], Value::Null);
    harness.reconcile().await;
    let ready = session_view(&harness, &alice, &id(&first)).await;
    assert_eq!(ready["status"], "ready");
    assert_safe(&ready, &[]);

    assert_eq!(
        act(&harness, &alice, &id(&first), "stop").await.status(),
        StatusCode::ACCEPTED
    );
    let stopping = session_view(&harness, &alice, &id(&first)).await;
    assert_eq!(stopping["stop_requested"], true);
    harness.reconcile().await;
    let stopped = session_view(&harness, &alice, &id(&first)).await;
    assert_eq!(stopped["status"], "stopped");
    assert_eq!(stopped["error"], Value::Null);
    assert_eq!(
        act(&harness, &alice, &id(&first), "stop").await.status(),
        StatusCode::OK,
        "stopping a stopped session is a no-op"
    );

    let secret = "namespace tenant-a-lab-7 quota exceeded on node gpu-03";
    harness.provider.inject(
        FakeFaultPoint::BeforeProvision,
        FakeFault::Reject(secret.into()),
    );
    let second = launched(&harness, &alice, "lab-1").await;
    assert_eq!(second["generation"], 2);
    harness.reconcile().await;
    let failed = session_view(&harness, &alice, &id(&second)).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"]["code"], "provision_failed");
    assert_eq!(failed["error"]["message"], "the lab could not be started");
    assert_safe(&failed, &["tenant-a-lab-7", "quota", "gpu-03"]);

    let third = launched(&harness, &alice, "lab-1").await;
    harness.reconcile().await;
    let blocked_reason = "finalizer kubernetes.io/pvc-protection on pod lab-xyz";
    harness.provider.inject(
        FakeFaultPoint::BeforeDelete,
        FakeFault::Blocked(blocked_reason.into()),
    );
    act(&harness, &alice, &id(&third), "stop").await;
    harness.reconcile().await;
    harness.reconcile().await;
    let blocked = session_view(&harness, &alice, &id(&third)).await;
    assert_eq!(blocked["status"], "cleanup_blocked");
    assert_eq!(blocked["error"]["code"], "cleanup_blocked");
    assert_safe(&blocked, &["finalizer", "pvc-protection", "lab-xyz"]);

    // A quarantined session still holds its slot and blocks a new launch
    // for the student.
    let refused = launch(&harness, &alice, "lab-2", json!({})).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);

    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
    let statuses = mine["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["status"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(statuses, ["cleanup_blocked", "failed", "stopped"]);
    assert_safe(&mine, &[secret, blocked_reason]);
}

#[tokio::test]
async fn stop_reset_and_status_check_ownership() {
    let harness = labs_harness(5).await;
    enroll_students(&harness, "cs101", &["alice", "bob"]).await;
    harness
        .enroll(TENANT, "cs101", "teacher", "instructor")
        .await;
    harness
        .enroll(TENANT, "cs102", "other-teacher", "instructor")
        .await;
    harness.make_admins(TENANT, &["admin"]).await;
    let alice = harness.login("alice").await;
    let bob = harness.login("bob").await;
    let teacher = harness.login("teacher").await;
    let other_teacher = harness.login("other-teacher").await;
    let outsider = harness.login("outsider").await;
    let admin = harness.login("admin").await;

    let session = launched(&harness, &alice, "lab-1").await;
    let session_id = id(&session);
    harness.reconcile().await;

    for (caller, label) in [
        (&bob, "classmate"),
        (&other_teacher, "instructor of another course"),
        (&outsider, "outsider"),
    ] {
        let (status, body) = view(&harness, caller, &session_id).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{label}");
        assert_eq!(body["code"], "session_not_found");
        for action in ["stop", "reset"] {
            let response = act(&harness, caller, &session_id, action).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{label} {action}");
        }
    }
    // Unknown and malformed ids look the same as someone else's.
    for missing in ["cls_missing", "bad%20id"] {
        assert_eq!(
            view(&harness, &alice, missing).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        session_view(&harness, &alice, &session_id).await["status"],
        "ready",
        "nothing above changed the session"
    );

    // Course staff can see and stop, but never reset, a student's lab.
    for staff in [&teacher, &admin] {
        assert_eq!(view(&harness, staff, &session_id).await.0, StatusCode::OK);
        assert_eq!(
            act(&harness, staff, &session_id, "reset").await.status(),
            StatusCode::NOT_FOUND
        );
    }
    let roster = body_json(
        harness
            .get("/api/campus/courses/cs101/lab-sessions", Some(&teacher))
            .await,
    )
    .await;
    assert_eq!(roster["sessions"][0]["owner"]["subject"], "alice");
    assert_eq!(
        harness
            .get("/api/campus/courses/cs101/lab-sessions", Some(&bob))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        act(&harness, &teacher, &session_id, "stop").await.status(),
        StatusCode::ACCEPTED
    );
    harness.reconcile().await;
    assert_eq!(
        session_view(&harness, &alice, &session_id).await["status"],
        "stopped"
    );

    // A student removed from the course can no longer reset.
    harness
        .store
        .delete_campus_course_membership(
            &CampusIdentityKey {
                tenant_id: TENANT.into(),
                issuer: ISSUER.into(),
                subject: "alice".into(),
            },
            "cs101",
        )
        .await
        .unwrap();
    assert_eq!(
        act(&harness, &alice, &session_id, "reset").await.status(),
        StatusCode::FORBIDDEN
    );

    let unknown_field = harness
        .mutate(
            "POST",
            &format!("/api/campus/lab-sessions/{session_id}/stop"),
            &teacher,
            json!({ "force": true }),
        )
        .await;
    assert_eq!(unknown_field.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reset_waits_for_teardown_keeps_the_slot_and_the_budget() {
    let harness = labs_harness(1).await;
    enroll_students(&harness, "cs101", &["alice", "bob"]).await;
    let alice = harness.login("alice").await;
    let bob = harness.login("bob").await;

    let first = launched(&harness, &alice, "lab-1").await;
    let first_id = id(&first);
    harness.reconcile().await;
    let budget = harness
        .store
        .get_campus_lab(TENANT, "cs101", "lab-1", ISSUER, "alice")
        .await
        .unwrap()
        .unwrap()
        .budget_id;

    harness
        .provider
        .inject(FakeFaultPoint::BeforeDelete, FakeFault::InProgress);
    let pending = act(&harness, &alice, &first_id, "reset").await;
    assert_eq!(pending.status(), StatusCode::ACCEPTED);
    let pending = body_json(pending).await;
    assert_eq!(pending["reset"], "pending");
    assert_eq!(pending["session"]["reset_pending"], true);
    assert_eq!(pending["session"]["stop_requested"], true);
    // A repeated reset while teardown runs changes nothing.
    assert_eq!(
        act(&harness, &alice, &first_id, "reset").await.status(),
        StatusCode::ACCEPTED
    );

    harness.reconcile().await;
    let cleaning = session_view(&harness, &alice, &first_id).await;
    assert_eq!(cleaning["status"], "stopping");
    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
    assert_eq!(
        mine["sessions"].as_array().unwrap().len(),
        1,
        "no new generation before teardown completes"
    );
    let full = launch(&harness, &bob, "lab-1", json!({})).await;
    assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);

    let sweep = harness.reconcile().await;
    assert_eq!(sweep.resets_admitted, 1);
    let old = session_view(&harness, &alice, &first_id).await;
    assert_eq!(old["status"], "stopped");
    assert_eq!(old["current"], false);
    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
    let second = mine["sessions"][0].clone();
    assert_eq!(second["generation"], 2);
    assert_eq!(second["lab_id"], first["lab_id"]);
    assert_eq!(second["reset_from_session_id"], first_id.as_str());
    assert_eq!(second["status"], "ready");
    assert!(old["terminated_at"].as_i64().unwrap() <= second["created_at"].as_i64().unwrap());
    // The reset kept the slot: bob is still refused.
    let full = launch(&harness, &bob, "lab-1", json!({})).await;
    assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(full).await["code"], "no_slots_available");

    // Retrying the old reset answers with the generation it produced.
    let replay = act(&harness, &alice, &first_id, "reset").await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(
        body_json(replay).await["session"]["session_id"],
        second["session_id"]
    );

    // Resets are unlimited, and every generation keeps the budget.
    let mut current = id(&second);
    for generation in 3..=6 {
        assert_eq!(
            act(&harness, &alice, &current, "reset").await.status(),
            StatusCode::ACCEPTED
        );
        harness.reconcile().await;
        let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
        let next = mine["sessions"][0].clone();
        assert_eq!(next["generation"], generation);
        assert_eq!(next["status"], "ready");
        current = id(&next);
    }
    let lab = harness
        .store
        .get_campus_lab(TENANT, "cs101", "lab-1", ISSUER, "alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lab.budget_id, budget);
    assert_eq!(lab.generation, 6);
    assert_eq!(harness.provider.workspaces_created(), 6);

    // A stopped session is reset at once.
    act(&harness, &alice, &current, "stop").await;
    harness.reconcile().await;
    let immediate = act(&harness, &alice, &current, "reset").await;
    assert_eq!(immediate.status(), StatusCode::CREATED);
    let immediate = body_json(immediate).await;
    assert_eq!(immediate["reset"], "created");
    assert_eq!(immediate["session"]["generation"], 7);
}

#[tokio::test]
async fn reset_of_a_blocked_cleanup_waits_for_the_retry() {
    let harness = labs_harness(2).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;
    let first = launched(&harness, &alice, "lab-1").await;
    let first_id = id(&first);
    harness.reconcile().await;
    harness.provider.inject(
        FakeFaultPoint::BeforeDelete,
        FakeFault::Blocked("stuck".into()),
    );
    assert_eq!(
        act(&harness, &alice, &first_id, "reset").await.status(),
        StatusCode::ACCEPTED
    );
    harness.reconcile().await;
    harness.reconcile().await;
    let blocked = session_view(&harness, &alice, &first_id).await;
    assert_eq!(blocked["status"], "cleanup_blocked");
    assert_eq!(blocked["reset_pending"], true);
    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
    assert_eq!(mine["sessions"].as_array().unwrap().len(), 1);

    let context = CampusDriverContext {
        worker_id: "operator",
        now: harness.now(),
        lease_seconds: 60,
        central_authority: true,
        teardown_hooks: &[],
    };
    retry_campus_lab_cleanup(
        &harness.store,
        harness.provider.as_ref(),
        &context,
        &first_id,
    )
    .await
    .unwrap();
    let sweep = harness.reconcile().await;
    assert_eq!(sweep.resets_admitted, 1);
    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
    assert_eq!(mine["sessions"][0]["generation"], 2);
    assert_eq!(mine["sessions"][0]["status"], "ready");
}

#[tokio::test]
async fn a_reset_for_a_removed_member_is_cleared_not_admitted() {
    let harness = labs_harness(2).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;
    let first = launched(&harness, &alice, "lab-1").await;
    harness.reconcile().await;
    act(&harness, &alice, &id(&first), "reset").await;
    harness
        .store
        .delete_campus_course_membership(
            &CampusIdentityKey {
                tenant_id: TENANT.into(),
                issuer: ISSUER.into(),
                subject: "alice".into(),
            },
            "cs101",
        )
        .await
        .unwrap();
    let sweep = harness.reconcile().await;
    assert_eq!(sweep.resets_admitted, 0);
    assert_eq!(sweep.resets_cleared, 1);
    let old = session_view(&harness, &alice, &id(&first)).await;
    assert_eq!(old["status"], "stopped");
    assert_eq!(old["reset_pending"], false);
    assert_eq!(
        harness
            .store
            .count_live_campus_lab_sessions(TENANT)
            .await
            .unwrap(),
        0
    );
}

struct BrokerClock(Arc<super::TestClock>);

impl CampusBrokerClock for BrokerClock {
    fn now(&self) -> i64 {
        use super::super::state::CampusClock;
        self.0.now()
    }
}

#[tokio::test]
async fn sessions_expire_server_side_and_lose_authority() {
    let harness = labs_harness(5).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;
    let labs = harness.labs.clone().unwrap();
    let source = JournalCampusAuthoritySource::new(
        (*harness.store).clone(),
        labs.catalog.clone(),
        Arc::new(BrokerClock(harness.clock.clone())),
    );
    let session = launched(&harness, &alice, "lab-2").await;
    let session_id = id(&session);
    assert_eq!(session["expires_at"], harness.now() + 1_800);
    harness.reconcile().await;
    let snapshot = source.session(&session_id).await.unwrap().unwrap();
    assert_eq!(snapshot.status, CampusAuthorityStatus::Active);
    assert_eq!(snapshot.authority.course_id, "cs101");

    harness.clock.advance(1_799);
    harness.reconcile().await;
    assert_eq!(
        session_view(&harness, &alice, &session_id).await["status"],
        "ready"
    );
    harness.clock.advance(1);
    assert_eq!(
        source.session(&session_id).await.unwrap().unwrap().status,
        CampusAuthorityStatus::Revoked,
        "authority ends at expiry even before the reconciler runs"
    );
    harness.reconcile().await;
    let expired = session_view(&harness, &alice, &session_id).await;
    assert_eq!(expired["status"], "stopped");
    assert!(harness.provider.resources().is_empty());
}

#[tokio::test]
async fn labs_fail_closed_without_a_catalog_or_a_provider() {
    let harness = Harness::new().await;
    harness.enroll(TENANT, "cs101", "alice", "student").await;
    let alice = harness.login("alice").await;
    for response in [
        harness
            .get("/api/campus/courses/cs101/assignments", Some(&alice))
            .await,
        launch(&harness, &alice, "lab-1", json!({})).await,
    ] {
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body_json(response).await["code"], "labs_unavailable");
    }

    let harness = Harness::with_labs(parse(&catalog_value(5)).unwrap(), false, true).await;
    harness.enroll(TENANT, "cs101", "alice", "student").await;
    let alice = harness.login("alice").await;
    let before = harness.row_counts().await;
    let response = launch(&harness, &alice, "lab-1", json!({})).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(response).await["code"], "labs_unavailable");
    assert_eq!(harness.row_counts().await, before);
    assert_eq!(harness.reconcile().await, Default::default());
    let listed = body_json(
        harness
            .get("/api/campus/courses/cs101/assignments", Some(&alice))
            .await,
    )
    .await;
    assert_eq!(listed["assignments"][0]["launchable"], false);
    assert_eq!(before["campus_lab_slots"], 5);
}

#[test]
fn catalog_defaults_bounds_and_strict_fields() {
    let mut minimal = catalog_value(5);
    minimal.as_object_mut().unwrap().remove("lab_slots");
    let catalog = parse(&minimal).unwrap();
    assert_eq!(catalog.limits.lab_slots, 5, "M1 default is five slots");
    assert_eq!(catalog.limits.max_active_per_course, 5);
    assert_eq!(catalog.limits.max_active_per_identity, 1);

    let invalid = |change: &dyn Fn(&mut Value)| {
        let mut value = catalog_value(5);
        change(&mut value);
        parse(&value).is_err()
    };
    assert!(invalid(&|v| v["schema_version"] = json!("other")));
    assert!(invalid(&|v| v["lab_slots"] = json!(0)));
    assert!(invalid(&|v| v["lab_slots"] = json!(65)));
    assert!(invalid(&|v| v["max_active_per_course"] = json!(6)));
    assert!(invalid(&|v| v["max_session_seconds"] = json!(7_201)));
    assert!(invalid(&|v| v["namespace"] = json!("kube-system")));
    assert!(invalid(&|v| v["assignments"][0]["image"] = json!("evil")));
    assert!(invalid(
        &|v| v["assignments"][0]["profile_id"] = json!("missing")
    ));
    assert!(invalid(
        &|v| v["assignments"][0]["course_id"] = json!("bad id")
    ));
    assert!(invalid(&|v| v["assignments"][0]["title"] = json!(" ")));
    assert!(invalid(&|v| {
        let duplicate = v["assignments"][0].clone();
        v["assignments"].as_array_mut().unwrap().push(duplicate);
    }));
    assert!(invalid(&|v| {
        v["policy_layers"].as_array_mut().unwrap().remove(1);
    }));
    assert!(invalid(
        &|v| v["profiles"][0]["image"] = json!("nginx:latest")
    ));
    // The security profile is gated until its qualification is recorded.
    assert!(invalid(&|v| {
        v["profiles"]
            .as_array_mut()
            .unwrap()
            .push(profile(SECURITY));
        v["assignments"].as_array_mut().unwrap().push(assignment(
            "cs101",
            "sec",
            "campus-security",
        ));
    }));
    let mut qualified = catalog_value(5);
    qualified["recorded_qualifications"] = json!(["campus-security-profile"]);
    qualified["profiles"]
        .as_array_mut()
        .unwrap()
        .push(profile(SECURITY));
    qualified["assignments"]
        .as_array_mut()
        .unwrap()
        .push(assignment("cs101", "sec", "campus-security"));
    assert!(parse(&qualified).is_ok());

    let mut shorter = catalog_value(5);
    shorter["max_session_seconds"] = json!(900);
    let catalog = parse(&shorter).unwrap();
    assert_eq!(
        catalog
            .assignment("cs101", "lab-1")
            .unwrap()
            .session_seconds,
        900
    );
}

#[test]
fn lab_provider_and_catalog_file_come_from_the_environment() {
    let path = std::env::temp_dir().join(format!(
        "pharness-campus-labs-{}.json",
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::write(&path, catalog_value(5).to_string()).unwrap();
    let file = path.display().to_string();
    let env = |pairs: &[(&str, &str)]| {
        let pairs = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        move |name: &str| pairs.get(name).cloned()
    };
    let config = test_config();
    assert!(load_campus_labs(&config, env(&[])).unwrap().is_none());
    let loaded = load_campus_labs(&config, env(&[(ENV_LABS_FILE, &file)]))
        .unwrap()
        .unwrap();
    assert_eq!(loaded.provider, CampusLabProviderKind::None);
    assert!(load_campus_labs(
        &config,
        env(&[(ENV_LABS_FILE, &file), (ENV_LAB_PROVIDER, "fake")])
    )
    .is_err());
    assert!(load_campus_labs(
        &config,
        env(&[(ENV_LABS_FILE, &file), (ENV_LAB_PROVIDER, "kubernetes")])
    )
    .is_err());
    assert!(
        load_campus_labs(&config, env(&[(ENV_LAB_PROVIDER, "none")]))
            .unwrap()
            .is_none()
    );
    let mut dev = test_config();
    dev.allow_insecure_loopback_http = true;
    assert_eq!(
        load_campus_labs(
            &dev,
            env(&[(ENV_LABS_FILE, &file), (ENV_LAB_PROVIDER, "fake")])
        )
        .unwrap()
        .unwrap()
        .provider,
        CampusLabProviderKind::Fake
    );
    assert!(load_campus_labs(&dev, env(&[(ENV_LAB_PROVIDER, "fake")])).is_err());
    assert!(load_campus_labs(&config, env(&[(ENV_LABS_FILE, "/nonexistent/labs.json")])).is_err());
    std::fs::remove_file(&path).unwrap();
}
