//! Audit records of Campus browser terminals in the lab-session event log.
//!
//! Terminal opens, closes and refused attaches are appended to
//! `campus_session_events` with kinds under `campus_terminal.`, at the
//! session's current journal generation. They never carry terminal input or
//! output, tickets or cookies: only connection ids, reason codes and counts.

use super::SqliteStore;
use crate::CampusJournalError;
use serde_json::Value;

type CampusResult<T> = Result<T, CampusJournalError>;

pub const CAMPUS_TERMINAL_EVENT_PREFIX: &str = "campus_terminal.";
const MAX_PAYLOAD_BYTES: usize = 4096;

impl SqliteStore {
    /// Append a terminal audit event. Returns its sequence, or `NotFound`
    /// when the session does not exist.
    pub async fn record_campus_terminal_event(
        &self,
        session_id: &str,
        kind: &str,
        actor: &str,
        payload: &Value,
        now: i64,
    ) -> CampusResult<i64> {
        let suffix = kind
            .strip_prefix(CAMPUS_TERMINAL_EVENT_PREFIX)
            .unwrap_or("");
        if suffix.is_empty()
            || kind.len() > 128
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        {
            return Err(CampusJournalError::Invalid(format!(
                "terminal event kinds start with {CAMPUS_TERMINAL_EVENT_PREFIX}"
            )));
        }
        if actor.trim().is_empty() {
            return Err(CampusJournalError::Invalid(
                "a terminal event needs an actor".into(),
            ));
        }
        let payload = payload.to_string();
        if !payload.starts_with('{') || payload.len() > MAX_PAYLOAD_BYTES {
            return Err(CampusJournalError::Invalid(
                "a terminal event payload is a small JSON object".into(),
            ));
        }
        let sequence: Option<i64> = sqlx::query_scalar(
            "INSERT INTO campus_session_events (
               session_id, kind, generation, actor, payload_json, created_at
             )
             SELECT session_id, ?2, generation, ?3, ?4, ?5
             FROM campus_sessions_journal WHERE session_id = ?1
             RETURNING sequence",
        )
        .bind(session_id)
        .bind(kind)
        .bind(actor)
        .bind(payload)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        sequence.ok_or_else(|| CampusJournalError::NotFound(session_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::campus_journal::test_support::seed;
    use crate::{CampusJournalError, SqliteStore};
    use serde_json::json;

    async fn store_with_session() -> SqliteStore {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        seed(&store, &["slot-1"], &["s1"], 1_000).await;
        store
    }

    #[tokio::test]
    async fn terminal_events_append_to_the_session_log() {
        let store = store_with_session().await;
        let before = store.list_campus_lab_session_events("s1").await.unwrap();
        store
            .record_campus_terminal_event(
                "s1",
                "campus_terminal.opened",
                "campus-identity:alice",
                &json!({ "connection_id": "ctc_1" }),
                1_001,
            )
            .await
            .unwrap();
        let events = store.list_campus_lab_session_events("s1").await.unwrap();
        assert_eq!(events.len(), before.len() + 1);
        let event = events.last().unwrap();
        assert_eq!(event.kind, "campus_terminal.opened");
        assert_eq!(event.actor, "campus-identity:alice");
        assert_eq!(event.payload, json!({ "connection_id": "ctc_1" }));
        assert_eq!(event.from_state, None);
    }

    #[tokio::test]
    async fn terminal_events_are_validated() {
        let store = store_with_session().await;
        for (session, kind, actor, payload) in [
            ("s1", "campus_session.ready", "a", json!({})),
            ("s1", "campus_terminal.", "a", json!({})),
            ("s1", "campus_terminal.Opened", "a", json!({})),
            ("s1", "campus_terminal.opened", " ", json!({})),
            ("s1", "campus_terminal.opened", "a", json!("text")),
            (
                "s1",
                "campus_terminal.opened",
                "a",
                json!({ "x": "y".repeat(5000) }),
            ),
        ] {
            assert!(
                matches!(
                    store
                        .record_campus_terminal_event(session, kind, actor, &payload, 1)
                        .await,
                    Err(CampusJournalError::Invalid(_))
                ),
                "{kind} {actor}"
            );
        }
        assert!(matches!(
            store
                .record_campus_terminal_event("nope", "campus_terminal.opened", "a", &json!({}), 1)
                .await,
            Err(CampusJournalError::NotFound(_))
        ));
    }
}
