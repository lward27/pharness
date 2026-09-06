use super::*;
use pharness_core::InferenceEvaluationScope;
use serde_json::json;

fn qualification(e: &StoredInferenceEvaluation) -> CreateInferencePolicyQualification {
    CreateInferencePolicyQualification {
        id: format!("inferqual_{}", e.id),
        policy_id: e.policy_id.clone(),
        policy_revision: e.policy_revision.clone(),
        policy_hash: e.policy_hash.clone(),
        target_id: e.target_id.clone(),
        target_revision: e.target_revision.clone(),
        target_hash: e.target_hash.clone(),
        agent_profile_id: e.agent_profile_id.clone(),
        agent_profile_hash: e.agent_profile_hash.clone(),
        suite_id: e.suite_id.clone(),
        suite_hash: e.suite_hash.clone(),
        runtime_revision: e.runtime_revision.clone(),
        attempts: e.attempts,
        metrics: json!({"gate_passed":true}),
        verdict: "passed".into(),
        evidence_artifact_id: None,
        actor: e.actor.clone(),
        reason: e.reason.clone(),
    }
}

#[tokio::test]
async fn diagnostic_completion_is_durable_without_qualification_and_rejects_duplicates() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let mut request = evaluation("infeval_diagnostic");
    request.suite_id = "planner-v2".into();
    request.attempts = 1;
    request.scope = InferenceEvaluationScope::Diagnostic {
        case_ids: vec!["acceptance-boundary".into()],
        reference_evaluation_id: None,
    };
    let saved = store
        .create_inference_evaluation(request.clone())
        .await
        .unwrap();
    assert_eq!(saved.scope, request.scope);
    store
        .mark_inference_evaluation_running(&saved.id, "job")
        .await
        .unwrap();
    let report = json!({"scope":saved.scope,"diagnostic":{"passed":true},"gate_passed":false});
    assert!(store
        .complete_inference_evaluation(&saved.id, &report, "hash", Some(qualification(&saved)))
        .await
        .is_err());
    assert_eq!(
        store
            .get_inference_evaluation(&saved.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    let done = store
        .complete_inference_evaluation(&saved.id, &report, "hash", None)
        .await
        .unwrap();
    assert_eq!(done.report, Some(report.clone()));
    assert_eq!(done.status, "completed");
    assert_eq!(done.qualification_id, None);
    assert!(store
        .complete_inference_evaluation(&saved.id, &json!({"changed":true}), "other", None)
        .await
        .is_err());
    assert_eq!(
        store
            .get_inference_evaluation(&saved.id)
            .await
            .unwrap()
            .unwrap()
            .report,
        Some(report)
    );
    assert!(store
        .list_inference_policy_qualifications(&saved.policy_id, &saved.policy_revision)
        .await
        .unwrap()
        .is_empty());
    // A later full run still has to attach its own qualification atomically.
    let full = store
        .create_inference_evaluation(evaluation("infeval_full"))
        .await
        .unwrap();
    store
        .mark_inference_evaluation_running(&full.id, "full-job")
        .await
        .unwrap();
    assert!(store
        .complete_inference_evaluation(&full.id, &json!({}), "full", None)
        .await
        .is_err());
    let done = store
        .complete_inference_evaluation(&full.id, &json!({}), "full", Some(qualification(&full)))
        .await
        .unwrap();
    assert!(done.qualification_id.is_some());
    assert_eq!(
        store
            .list_inference_policy_qualifications(&full.policy_id, &full.policy_revision)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn schema_54_evaluation_history_survives_additive_scope_migration_and_reopen() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::borrow::Cow;
    let path = std::env::temp_dir().join(format!(
        "pharness-evaluation-scope-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(false),
        )
        .await
        .unwrap();
    let all = sqlx::migrate!("./migrations");
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 54).cloned().collect()),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
    .run(&pool)
    .await
    .unwrap();
    let binding = serde_json::to_string(&evaluation("seed").resolved_binding).unwrap();
    for status in ["completed", "failed", "running"] {
        sqlx::query("INSERT INTO inference_evaluations (id,status,suite_id,suite_hash,attempts,agent_profile_id,agent_profile_hash,target_id,target_revision,target_hash,policy_id,policy_revision,policy_hash,resolved_binding_json,binding_hash,runtime_revision,actor,reason,config_hash,created_at,report_json,report_hash,failure) VALUES (?1,?1,'planner-v1','suite',2,'repo-planner','profile','target','v1','target','policy','v1','policy',?2,'binding','historical-runtime','original-actor','original reason','config','original time','{\"original\":true}','original-hash','original detail')").bind(status).bind(&binding).execute(&pool).await.unwrap();
    }
    let columns = sqlx::query("PRAGMA table_info(inference_evaluations)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| format!("\"{}\"", r.get::<String, _>("name")))
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("SELECT json_array({columns}) FROM inference_evaluations ORDER BY id");
    let before: Vec<String> = sqlx::query_scalar(&query).fetch_all(&pool).await.unwrap();
    pool.close().await;
    for _ in 0..2 {
        let store = SqliteStore::connect(&path).await.unwrap();
        let after: Vec<String> = sqlx::query_scalar(&query)
            .fetch_all(&store.pool)
            .await
            .unwrap();
        assert_eq!(before, after);
        for id in ["completed", "failed", "running"] {
            assert_eq!(
                store
                    .get_inference_evaluation(id)
                    .await
                    .unwrap()
                    .unwrap()
                    .scope,
                InferenceEvaluationScope::FullQualification {}
            );
        }
        assert!(sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&store.pool)
            .await
            .unwrap()
            .is_empty());
        let previous_reader = sqlx::migrate::Migrator {
            migrations: Cow::Owned(all.iter().filter(|m| m.version <= 54).cloned().collect()),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        };
        assert!(matches!(
            previous_reader.run(&store.pool).await,
            Err(sqlx::migrate::MigrateError::VersionMissing(55))
        ));
        store.pool.close().await;
    }
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
