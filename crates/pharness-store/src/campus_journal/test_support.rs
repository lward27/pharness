//! Shared fixtures for journal and driver tests.

use crate::{
    CampusIdentityKey, CreateCampusLabSession, SqliteStore, StoredCampusLabSession,
    UpsertCampusIdentity,
};
use pharness_workspaces::CampusProfileRef;

pub const TENANT: &str = "tenant-a";
pub const ISSUER: &str = "https://idp.example.test";
pub const TTL: i64 = 3_600;

pub fn profile() -> CampusProfileRef {
    CampusProfileRef {
        id: "campus-coding".into(),
        version: "2026.1".into(),
        profile_hash: "a".repeat(64),
    }
}

pub async fn student(store: &SqliteStore, subject: &str) {
    store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: format!("cid_{subject}"),
            key: CampusIdentityKey {
                tenant_id: TENANT.into(),
                issuer: ISSUER.into(),
                subject: subject.into(),
            },
            email: None,
            display_name: None,
            now: 1,
        })
        .await
        .unwrap();
}

pub fn request(session_id: &str, subject: &str, now: i64) -> CreateCampusLabSession {
    CreateCampusLabSession {
        session_id: session_id.into(),
        tenant_id: TENANT.into(),
        course_id: "course-101".into(),
        issuer: ISSUER.into(),
        subject: subject.into(),
        profile: profile(),
        expires_at: now + TTL,
        actor: format!("student:{subject}"),
        now,
    }
}

/// Register `slots` and request one session per id, each for its own
/// student.
pub async fn seed(
    store: &SqliteStore,
    slots: &[&str],
    sessions: &[&str],
    now: i64,
) -> Vec<StoredCampusLabSession> {
    for slot in slots {
        store
            .register_campus_lab_slot(slot, TENANT, now)
            .await
            .unwrap();
    }
    let mut created = Vec::new();
    for session in sessions {
        let subject = format!("student-{session}");
        student(store, &subject).await;
        created.push(
            store
                .create_campus_lab_session(request(session, &subject, now))
                .await
                .unwrap()
                .0,
        );
    }
    created
}

/// A file-backed store, so that several pooled connections really run
/// concurrently. The files are removed on drop.
pub struct DiskStore {
    pub store: SqliteStore,
    path: std::path::PathBuf,
}

impl DiskStore {
    pub async fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "pharness-campus-journal-{label}-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteStore::connect(&path).await.unwrap();
        Self { store, path }
    }
}

impl Drop for DiskStore {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
        }
    }
}
