use crate::{
    CampusCourseRole, CampusIdentityKey, CreateCampusLoginAttempt, CreateCampusSession,
    PutCampusMembership, SqliteStore, StoreError, UpsertCampusIdentity,
};

fn key(tenant: &str, subject: &str) -> CampusIdentityKey {
    CampusIdentityKey {
        tenant_id: tenant.into(),
        issuer: "https://idp.example.test".into(),
        subject: subject.into(),
    }
}

fn attempt(state_hash: &str, now: i64) -> CreateCampusLoginAttempt {
    CreateCampusLoginAttempt {
        state_hash: state_hash.into(),
        tenant_id: "tenant-a".into(),
        browser_binding_hash: "binding".into(),
        nonce: "nonce".into(),
        pkce_verifier: "verifier".into(),
        now,
        expires_at: now + 600,
    }
}

async fn identity(store: &SqliteStore, subject: &str, email: &str, now: i64) -> String {
    store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: format!("cid_{subject}_{now}"),
            key: key("tenant-a", subject),
            email: Some(email.into()),
            display_name: None,
            now,
        })
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn login_attempts_are_consumed_exactly_once_and_bounded() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    store
        .create_campus_login_attempt(attempt("state-1", 100), 2)
        .await
        .unwrap();
    let consumed = store
        .consume_campus_login_attempt("state-1", 101)
        .await
        .unwrap()
        .expect("first consumption succeeds");
    assert_eq!(consumed.nonce, "nonce");
    assert_eq!(consumed.expires_at, 700);
    assert!(store
        .consume_campus_login_attempt("state-1", 102)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .consume_campus_login_attempt("unknown", 102)
        .await
        .unwrap()
        .is_none());

    store
        .create_campus_login_attempt(attempt("state-2", 100), 2)
        .await
        .unwrap();
    store
        .create_campus_login_attempt(attempt("state-3", 100), 2)
        .await
        .unwrap();
    assert!(matches!(
        store
            .create_campus_login_attempt(attempt("state-4", 100), 2)
            .await,
        Err(StoreError::Conflict(_))
    ));
    // Expired attempts are purged before counting.
    store
        .create_campus_login_attempt(attempt("state-5", 800), 2)
        .await
        .unwrap();
    assert!(store
        .consume_campus_login_attempt("state-2", 801)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn identity_key_is_the_tuple_and_email_is_display_only() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let first = identity(&store, "sub-1", "old@example.test", 10).await;
    let again = identity(&store, "sub-1", "new@example.test", 20).await;
    assert_eq!(first, again);
    let other = identity(&store, "sub-2", "new@example.test", 30).await;
    assert_ne!(first, other);

    let other_tenant = store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: "cid_other_tenant".into(),
            key: key("tenant-b", "sub-1"),
            email: Some("new@example.test".into()),
            display_name: None,
            now: 40,
        })
        .await
        .unwrap();
    assert_ne!(other_tenant.id, first);
}

#[tokio::test]
async fn sessions_expire_on_idle_absolute_and_revocation() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let identity_id = identity(&store, "sub-1", "a@example.test", 0).await;
    store
        .create_campus_session(CreateCampusSession {
            id_hash: "session".into(),
            tenant_id: "tenant-a".into(),
            identity_id: identity_id.clone(),
            csrf_hash: "csrf".into(),
            now: 0,
            idle_expires_at: 100,
            absolute_expires_at: 250,
        })
        .await
        .unwrap();

    assert!(store
        .touch_campus_session("session", "tenant-b", 50, 100)
        .await
        .unwrap()
        .is_none());
    let active = store
        .touch_campus_session("session", "tenant-a", 99, 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.identity.id, identity_id);
    assert_eq!(active.idle_expires_at, 199);
    let capped = store
        .touch_campus_session("session", "tenant-a", 198, 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(capped.idle_expires_at, 250);
    assert!(store
        .touch_campus_session("session", "tenant-a", 250, 100)
        .await
        .unwrap()
        .is_none());

    store
        .create_campus_session(CreateCampusSession {
            id_hash: "idle".into(),
            tenant_id: "tenant-a".into(),
            identity_id: identity_id.clone(),
            csrf_hash: "csrf".into(),
            now: 0,
            idle_expires_at: 100,
            absolute_expires_at: 1000,
        })
        .await
        .unwrap();
    assert!(store
        .touch_campus_session("idle", "tenant-a", 100, 100)
        .await
        .unwrap()
        .is_none());

    store
        .create_campus_session(CreateCampusSession {
            id_hash: "revoked".into(),
            tenant_id: "tenant-a".into(),
            identity_id,
            csrf_hash: "csrf".into(),
            now: 0,
            idle_expires_at: 100,
            absolute_expires_at: 1000,
        })
        .await
        .unwrap();
    assert!(store.revoke_campus_session("revoked", 5).await.unwrap());
    assert!(!store.revoke_campus_session("revoked", 6).await.unwrap());
    assert!(store
        .touch_campus_session("revoked", "tenant-a", 6, 100)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memberships_and_admins_are_tenant_scoped() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let admin = identity(&store, "admin", "admin@example.test", 0).await;
    store
        .sync_campus_bootstrap_admins(
            "tenant-a",
            &[("https://idp.example.test".into(), "admin".into())],
            0,
        )
        .await
        .unwrap();
    assert!(store
        .is_campus_tenant_admin(&key("tenant-a", "admin"))
        .await
        .unwrap());
    assert!(!store
        .is_campus_tenant_admin(&key("tenant-b", "admin"))
        .await
        .unwrap());

    let created = store
        .put_campus_course_membership(PutCampusMembership {
            key: key("tenant-a", "student"),
            course_id: "cs101".into(),
            role: CampusCourseRole::Student,
            granted_by_identity_id: admin.clone(),
            now: 1,
        })
        .await
        .unwrap();
    assert!(created);
    let changed = store
        .put_campus_course_membership(PutCampusMembership {
            key: key("tenant-a", "student"),
            course_id: "cs101".into(),
            role: CampusCourseRole::Instructor,
            granted_by_identity_id: admin,
            now: 2,
        })
        .await
        .unwrap();
    assert!(!changed);
    assert_eq!(
        store
            .get_campus_course_role(&key("tenant-a", "student"), "cs101")
            .await
            .unwrap(),
        Some(CampusCourseRole::Instructor)
    );
    assert_eq!(
        store
            .get_campus_course_role(&key("tenant-b", "student"), "cs101")
            .await
            .unwrap(),
        None
    );
    assert!(!store
        .campus_course_exists("tenant-b", "cs101")
        .await
        .unwrap());
    assert_eq!(
        store
            .list_campus_course_members("tenant-a", "cs101")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .delete_campus_course_membership(&key("tenant-a", "student"), "cs101")
        .await
        .unwrap());
    assert!(store
        .list_campus_memberships_for_identity(&key("tenant-a", "student"))
        .await
        .unwrap()
        .is_empty());

    store
        .sync_campus_bootstrap_admins("tenant-a", &[], 3)
        .await
        .unwrap();
    assert!(!store
        .is_campus_tenant_admin(&key("tenant-a", "admin"))
        .await
        .unwrap());
}
