use crate::campus_journal::test_support::{profile, student, ISSUER, TENANT};
use crate::{
    advance_campus_lab_session, AdmitCampusLabLaunch, CampusDriverContext, CampusJournalError,
    CampusLabAdmission, CampusLabLimits, IssueCampusBrokerToken, JournalCampusBrokerTokenIssuer,
    SqliteStore,
};
use pharness_core::campus_broker_token::{
    CampusBrokerAudience as Audience, CampusBrokerDenial as Denial, CampusBrokerTokenError,
    CampusBrokerTokenIssuer, CampusIssuedBrokerToken, CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS,
};
use pharness_core::campus_capability::CampusBrokerClock;
use pharness_core::{
    CampusLabDesiredState, CampusLabSessionState as State, CampusWorkloadRef, FakeCampusProvider,
    FakeFault, FakeFaultPoint,
};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

mod migration;

const NOW: i64 = 10_000;
const SESSION_SECONDS: i64 = 7_200;

struct Clock(AtomicI64);

impl Clock {
    fn set(&self, now: i64) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl CampusBrokerClock for Clock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Fixture {
    store: SqliteStore,
    provider: FakeCampusProvider,
    clock: Arc<Clock>,
    issuer: JournalCampusBrokerTokenIssuer,
}

impl Fixture {
    async fn new() -> Self {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        for index in 0..3 {
            store
                .register_campus_lab_slot(&format!("slot-{index}"), TENANT, NOW)
                .await
                .unwrap();
        }
        let clock = Arc::new(Clock(AtomicI64::new(NOW)));
        let issuer =
            JournalCampusBrokerTokenIssuer::new(store.clone(), clock.clone(), "test-provider");
        Self {
            store,
            provider: FakeCampusProvider::new(),
            clock,
            issuer,
        }
    }

    fn now(&self) -> i64 {
        self.clock.now()
    }

    async fn admit(&self, subject: &str, reset_from: Option<String>) -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        student(&self.store, subject).await;
        let admission = self
            .store
            .admit_campus_lab_launch(AdmitCampusLabLaunch {
                tenant_id: TENANT.into(),
                course_id: "course-101".into(),
                assignment_id: "lab-1".into(),
                issuer: ISSUER.into(),
                subject: subject.into(),
                idempotency_key: None,
                reset_from_session_id: reset_from,
                candidate_lab_id: format!("lab-{id}"),
                candidate_budget_id: format!("budget-{id}"),
                candidate_session_id: format!("session-{id}"),
                profile: profile(),
                effective_policy_hash: "sha256:policy".into(),
                expires_at: self.now() + SESSION_SECONDS,
                actor: format!("student:{subject}"),
                now: self.now(),
                limits: CampusLabLimits {
                    lab_slots: 3,
                    max_active_per_identity: 1,
                    max_active_per_course: 3,
                },
            })
            .await
            .unwrap();
        match admission {
            CampusLabAdmission::Created(record) => record.session.session_id,
            other => panic!("expected a new session, got {other:?}"),
        }
    }

    async fn step(&self, session_id: &str) {
        let context = CampusDriverContext {
            worker_id: "test-reconciler",
            now: self.now(),
            lease_seconds: 60,
            central_authority: true,
        };
        advance_campus_lab_session(&self.store, &self.provider, &context, session_id)
            .await
            .unwrap();
    }

    /// Step a session until it is ready, terminal or quarantined.
    async fn settle(&self, session_id: &str) {
        for _ in 0..8 {
            self.step(session_id).await;
            let session = self.session(session_id).await;
            if session.state.is_terminal()
                || session.state == State::CleanupBlocked
                || (session.state == State::Ready
                    && session.desired_state == CampusLabDesiredState::Ready)
            {
                return;
            }
        }
    }

    async fn session(&self, session_id: &str) -> crate::StoredCampusLabSession {
        self.store
            .get_campus_lab_session(session_id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn ready(&self, subject: &str) -> CampusWorkloadRef {
        let session_id = self.admit(subject, None).await;
        self.settle(&session_id).await;
        let session = self.session(&session_id).await;
        assert_eq!(session.state, State::Ready);
        session.workload().unwrap()
    }

    async fn issue(
        &self,
        workload: &CampusWorkloadRef,
        audience: Audience,
    ) -> CampusIssuedBrokerToken {
        self.issuer.issue(workload, audience).await.unwrap()
    }

    async fn check(
        &self,
        token: &CampusIssuedBrokerToken,
        audience: Audience,
    ) -> Result<String, Denial> {
        self.store
            .authenticate_campus_broker_token(&token.secret.hash(), audience, self.now())
            .await
            .unwrap()
            .map(|grant| grant.session_id().to_string())
    }

    async fn stop(&self, session_id: &str, reset: bool) {
        self.store
            .request_campus_lab_stop(session_id, "student", "stop requested", reset, self.now())
            .await
            .unwrap();
    }
}

fn not_issuable(result: Result<CampusIssuedBrokerToken, CampusBrokerTokenError>) -> String {
    match result {
        Err(CampusBrokerTokenError::NotIssuable(reason)) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn tokens_are_issued_only_for_the_current_occupancy_of_a_live_session() {
    let fixture = Fixture::new().await;
    let requested = fixture.admit("alice", None).await;
    let unslotted = CampusWorkloadRef {
        session_id: requested.clone(),
        slot_id: "slot-0".into(),
        slot_generation: 1,
    };
    assert!(
        not_issuable(fixture.issuer.issue(&unslotted, Audience::Inference).await)
            .contains("requested")
    );

    // A provider issues the token while provisioning, before the Pod exists;
    // it does not authorize anything until the session is ready.
    fixture
        .provider
        .inject(FakeFaultPoint::AfterProvision, FakeFault::InProgress);
    let mut provisioning = fixture.session(&requested).await;
    for _ in 0..8 {
        fixture.step(&requested).await;
        provisioning = fixture.session(&requested).await;
        if provisioning.state == State::Provisioning && provisioning.workload().is_some() {
            break;
        }
    }
    assert_eq!(provisioning.state, State::Provisioning);
    let workload = provisioning.workload().unwrap();
    let early = fixture.issue(&workload, Audience::Inference).await;
    assert_eq!(
        fixture.check(&early, Audience::Inference).await,
        Err(Denial::SessionInactive)
    );
    fixture.settle(&requested).await;
    assert_eq!(fixture.session(&requested).await.state, State::Ready);
    assert_eq!(
        fixture.check(&early, Audience::Inference).await.as_deref(),
        Ok(requested.as_str())
    );

    for stale in [
        CampusWorkloadRef {
            slot_generation: workload.slot_generation + 1,
            ..workload.clone()
        },
        CampusWorkloadRef {
            slot_id: "slot-2".into(),
            ..workload.clone()
        },
    ] {
        assert!(
            not_issuable(fixture.issuer.issue(&stale, Audience::Inference).await)
                .contains("slot occupancy")
        );
    }
    let unknown = CampusWorkloadRef {
        session_id: "no-such-session".into(),
        ..workload.clone()
    };
    not_issuable(fixture.issuer.issue(&unknown, Audience::Inference).await);

    let invalid = |ttl_seconds, token_hash: &str| IssueCampusBrokerToken {
        token_id: "cbt_manual".into(),
        token_hash: token_hash.into(),
        workload: workload.clone(),
        audience: Audience::Inference,
        actor: "test".into(),
        now: fixture.now(),
        ttl_seconds,
    };
    for request in [
        invalid(0, &"a".repeat(64)),
        invalid(CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS + 1, &"a".repeat(64)),
        invalid(60, "pcbt1_raw-token-instead-of-a-hash"),
    ] {
        assert!(matches!(
            fixture.store.issue_campus_broker_token(request).await,
            Err(CampusJournalError::Invalid(_))
        ));
    }
}

#[tokio::test]
async fn only_a_hash_is_stored_and_the_ttl_is_capped_by_the_session() {
    let fixture = Fixture::new().await;
    let workload = fixture.ready("alice").await;
    let token = fixture.issue(&workload, Audience::Inference).await;
    assert_eq!(token.issued_at, NOW);
    assert_eq!(token.expires_at, NOW + CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS);
    assert!(token.token_id.starts_with("cbt_"));

    let raw = token.secret.expose();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&fixture.store.pool)
    .await
    .unwrap();
    let mut hash_seen = false;
    for table in tables {
        let columns: Vec<String> =
            sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .fetch_all(&fixture.store.pool)
                .await
                .unwrap();
        let select = columns
            .iter()
            .map(|column| {
                format!(
                    "CASE WHEN typeof(\"{column}\") IN ('text', 'integer', 'real') \
                     THEN CAST(\"{column}\" AS TEXT) ELSE hex(\"{column}\") END"
                )
            })
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let rows: Vec<String> = sqlx::query_scalar(&format!("SELECT {select} FROM \"{table}\""))
            .fetch_all(&fixture.store.pool)
            .await
            .unwrap();
        for row in rows {
            assert!(!row.contains(raw), "the raw token is stored in {table}");
            assert!(
                !row.contains(&raw[6..]),
                "the token secret is stored in {table}"
            );
            hash_seen |= row.contains(&token.secret.hash());
        }
    }
    assert!(hash_seen, "the hash is what is stored");

    fixture.clock.set(NOW + SESSION_SECONDS - 100);
    let late = fixture.issue(&workload, Audience::Inference).await;
    assert_eq!(
        late.expires_at,
        NOW + SESSION_SECONDS,
        "never past the session"
    );
}

#[tokio::test]
async fn a_token_is_bound_to_one_audience_and_one_session() {
    let fixture = Fixture::new().await;
    let alice = fixture.ready("alice").await;
    let bob = fixture.ready("bob").await;
    let alice_token = fixture.issue(&alice, Audience::Inference).await;
    let bob_token = fixture.issue(&bob, Audience::Inference).await;
    assert_eq!(
        fixture
            .check(&alice_token, Audience::Inference)
            .await
            .as_deref(),
        Ok(alice.session_id.as_str())
    );
    assert_eq!(
        fixture
            .check(&bob_token, Audience::Inference)
            .await
            .as_deref(),
        Ok(bob.session_id.as_str())
    );
    assert_eq!(
        fixture.check(&alice_token, Audience::Tools).await,
        Err(Denial::WrongAudience)
    );
    let tools = fixture.issue(&alice, Audience::Tools).await;
    assert_eq!(
        fixture.check(&tools, Audience::Inference).await,
        Err(Denial::WrongAudience)
    );
    assert_eq!(
        fixture
            .store
            .authenticate_campus_broker_token(&"0".repeat(64), Audience::Inference, NOW)
            .await
            .unwrap(),
        Err(Denial::UnknownToken)
    );
    let grant = fixture
        .store
        .authenticate_campus_broker_token(&alice_token.secret.hash(), Audience::Inference, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(grant.workload, alice);
    assert_eq!(grant.tenant_id, TENANT);
    assert_eq!(grant.course_id, "course-101");
    assert_eq!(grant.owner_identity_id, "cid_alice");
}

#[tokio::test]
async fn a_stop_revokes_every_token_at_once_and_teardown_keeps_them_revoked() {
    let fixture = Fixture::new().await;
    let workload = fixture.ready("alice").await;
    let inference = fixture.issue(&workload, Audience::Inference).await;
    let tools = fixture.issue(&workload, Audience::Tools).await;

    fixture.stop(&workload.session_id, false).await;
    // Revoked in the stop request's own transaction, before any reconciler
    // has moved the session.
    assert_eq!(
        fixture.session(&workload.session_id).await.state,
        State::Ready
    );
    for token in fixture
        .store
        .list_campus_broker_tokens(&workload.session_id)
        .await
        .unwrap()
    {
        assert_eq!(token.revoked_at, Some(NOW));
        assert_eq!(
            token.revoked_reason.as_deref(),
            Some("session_stop_requested")
        );
    }
    assert_eq!(
        fixture.check(&inference, Audience::Inference).await,
        Err(Denial::TokenRevoked)
    );
    assert_eq!(
        fixture.check(&tools, Audience::Tools).await,
        Err(Denial::TokenRevoked)
    );
    assert!(
        not_issuable(fixture.issuer.issue(&workload, Audience::Inference).await)
            .contains("cannot be given")
    );

    fixture.settle(&workload.session_id).await;
    assert_eq!(
        fixture.session(&workload.session_id).await.state,
        State::Terminated
    );
    assert_eq!(
        fixture.check(&inference, Audience::Inference).await,
        Err(Denial::TokenRevoked)
    );
    let reasons = fixture
        .store
        .list_campus_broker_tokens(&workload.session_id)
        .await
        .unwrap()
        .into_iter()
        .map(|token| token.revoked_reason.unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        reasons,
        ["session_stop_requested", "session_stop_requested"]
    );
}

#[tokio::test]
async fn a_lost_workload_revokes_its_tokens_with_the_state_change() {
    let fixture = Fixture::new().await;
    let workload = fixture.ready("alice").await;
    let token = fixture.issue(&workload, Audience::Inference).await;
    // The provider reports the workload gone: the controller moves the
    // session to stopping without any stop request.
    for resource in fixture.provider.resources_for(&workload) {
        assert!(fixture.provider.lose_resource(&resource.resource_id));
    }
    fixture.settle(&workload.session_id).await;
    assert_eq!(
        fixture.session(&workload.session_id).await.state,
        State::Failed
    );
    let tokens = fixture
        .store
        .list_campus_broker_tokens(&workload.session_id)
        .await
        .unwrap();
    assert_eq!(
        tokens[0].revoked_reason.as_deref(),
        Some("session_stopping")
    );
    assert_eq!(
        fixture.check(&token, Audience::Inference).await,
        Err(Denial::TokenRevoked)
    );
}

#[tokio::test]
async fn tokens_expire_after_fifteen_minutes_and_with_the_session() {
    let fixture = Fixture::new().await;
    let workload = fixture.ready("alice").await;
    let token = fixture.issue(&workload, Audience::Inference).await;
    fixture
        .clock
        .set(NOW + CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS - 1);
    assert!(fixture.check(&token, Audience::Inference).await.is_ok());
    fixture.clock.set(NOW + CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS);
    assert_eq!(
        fixture.check(&token, Audience::Inference).await,
        Err(Denial::TokenExpired)
    );

    fixture.clock.set(NOW + SESSION_SECONDS - 10);
    let last = fixture.issue(&workload, Audience::Inference).await;
    assert!(fixture.check(&last, Audience::Inference).await.is_ok());
    fixture.clock.set(NOW + SESSION_SECONDS);
    assert_eq!(
        fixture.check(&last, Audience::Inference).await,
        Err(Denial::TokenExpired)
    );
    not_issuable(fixture.issuer.issue(&workload, Audience::Inference).await);
}

#[tokio::test]
async fn a_reset_generation_never_honours_the_previous_generations_token() {
    let fixture = Fixture::new().await;
    let first = fixture.ready("alice").await;
    let old = fixture.issue(&first, Audience::Inference).await;

    fixture.stop(&first.session_id, true).await;
    fixture.settle(&first.session_id).await;
    let second_id = fixture.admit("alice", Some(first.session_id.clone())).await;
    fixture.settle(&second_id).await;
    let second = fixture.session(&second_id).await.workload().unwrap();
    assert_ne!(second.session_id, first.session_id);
    let new = fixture.issue(&second, Audience::Inference).await;
    assert_eq!(new.lab_generation, old.lab_generation + 1);

    assert_eq!(
        fixture.check(&old, Audience::Inference).await,
        Err(Denial::TokenRevoked)
    );
    assert_eq!(
        fixture.check(&new, Audience::Inference).await.as_deref(),
        Ok(second_id.as_str())
    );
    not_issuable(fixture.issuer.issue(&first, Audience::Inference).await);
}

#[tokio::test]
async fn rotation_keeps_the_current_and_previous_token_only() {
    let fixture = Fixture::new().await;
    let workload = fixture.ready("alice").await;
    let first = fixture.issue(&workload, Audience::Inference).await;
    fixture.clock.set(NOW + 450);
    let second = fixture.issue(&workload, Audience::Inference).await;
    let tools = fixture.issue(&workload, Audience::Tools).await;
    assert!(fixture.check(&first, Audience::Inference).await.is_ok());
    assert!(fixture.check(&second, Audience::Inference).await.is_ok());
    fixture.clock.set(NOW + 900);
    let third = fixture.issue(&workload, Audience::Inference).await;
    // The first one had expired anyway; it is also marked superseded.
    let first_row = fixture
        .store
        .list_campus_broker_tokens(&workload.session_id)
        .await
        .unwrap()
        .into_iter()
        .find(|token| token.token_id == first.token_id)
        .unwrap();
    assert_eq!(first_row.revoked_reason.as_deref(), Some("superseded"));
    assert!(fixture.check(&second, Audience::Inference).await.is_ok());
    assert!(fixture.check(&third, Audience::Inference).await.is_ok());
    assert!(
        fixture.check(&tools, Audience::Tools).await.is_ok(),
        "audiences rotate independently"
    );
    let fourth = fixture.issue(&workload, Audience::Inference).await;
    assert_eq!(
        fixture.check(&second, Audience::Inference).await,
        Err(Denial::TokenRevoked)
    );
    assert!(fixture.check(&fourth, Audience::Inference).await.is_ok());
}

#[tokio::test]
async fn operators_can_revoke_and_tokens_cannot_be_edited_or_deleted() {
    let fixture = Fixture::new().await;
    let workload = fixture.ready("alice").await;
    let token = fixture.issue(&workload, Audience::Inference).await;
    assert!(fixture
        .store
        .revoke_campus_broker_tokens(&workload.session_id, "", NOW)
        .await
        .is_err());

    for statement in [
        "UPDATE campus_broker_tokens SET expires_at = expires_at + 3600",
        "UPDATE campus_broker_tokens SET session_id = 'other'",
        "UPDATE campus_broker_tokens SET audience = 'tools'",
        "DELETE FROM campus_broker_tokens",
    ] {
        assert!(
            sqlx::query(statement)
                .execute(&fixture.store.pool)
                .await
                .is_err(),
            "{statement}"
        );
    }
    assert!(sqlx::query(
        "UPDATE campus_broker_tokens SET expires_at = issued_at + 901, revoked_at = 1, \
         revoked_reason = 'x'"
    )
    .execute(&fixture.store.pool)
    .await
    .is_err());

    assert_eq!(
        fixture
            .store
            .revoke_campus_broker_tokens(&workload.session_id, "suspected leak", NOW)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        fixture.check(&token, Audience::Inference).await,
        Err(Denial::TokenRevoked)
    );
    assert!(
        sqlx::query("UPDATE campus_broker_tokens SET revoked_reason = 'again'")
            .execute(&fixture.store.pool)
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .store
            .revoke_campus_broker_tokens(&workload.session_id, "suspected leak", NOW)
            .await
            .unwrap(),
        0
    );
}
