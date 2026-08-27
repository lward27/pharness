#![forbid(unsafe_code)]

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("data")
        || args.get(1).map(String::as_str) != Some("archive")
    {
        anyhow::bail!(
            "usage: pharness-admin data archive --database <path> --output-dir <path> [--work-item-id <id>]..."
        );
    }
    let mut database = None;
    let mut output_dir = None;
    let mut work_item_ids = Vec::new();
    let mut index = 2;
    while index < args.len() {
        let value = args
            .get(index + 1)
            .ok_or_else(|| anyhow::anyhow!("{} requires a value", args[index]))?;
        match args[index].as_str() {
            "--database" => database = Some(PathBuf::from(value)),
            "--output-dir" => output_dir = Some(PathBuf::from(value)),
            "--work-item-id" => work_item_ids.push(value.clone()),
            option => anyhow::bail!("unknown archive option {option}"),
        }
        index += 2;
    }
    let database = database.ok_or_else(|| anyhow::anyhow!("--database is required"))?;
    let output_dir = output_dir.ok_or_else(|| anyhow::anyhow!("--output-dir is required"))?;
    archive(&database, &output_dir, &work_item_ids).await
}

async fn archive(
    database: &Path,
    output_dir: &Path,
    work_item_ids: &[String],
) -> anyhow::Result<()> {
    if !database.is_file() {
        anyhow::bail!("database does not exist at {}", database.display());
    }
    std::fs::create_dir_all(output_dir)?;
    let backup_path = output_dir.join("pharness.db");
    let manifest_path = output_dir.join("manifest.json");
    if backup_path.exists() || manifest_path.exists() {
        anyhow::bail!("archive output already exists; use a new empty output directory");
    }
    let options = SqliteConnectOptions::new()
        .filename(database)
        .read_only(true)
        .create_if_missing(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    verify_integrity(&pool, "source").await?;
    let escaped = backup_path.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{escaped}'"))
        .execute(&pool)
        .await?;
    let backup_options = SqliteConnectOptions::from_str(&format!(
        "sqlite://{}?mode=ro",
        backup_path.to_string_lossy()
    ))?;
    let backup_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(backup_options)
        .await?;
    verify_integrity(&backup_pool, "archive").await?;
    let table_counts = table_counts(&backup_pool).await?;
    let migrations = json_rows(
        &backup_pool,
        "SELECT json_object('version',version,'description',description,'installed_on',installed_on,'success',success) AS value FROM _sqlx_migrations ORDER BY version",
    )
    .await?;
    let generation = json_rows(
        &backup_pool,
        "SELECT json_object('id',id,'created_at',created_at,'initializing_revision',initializing_revision,'schema_version',schema_version,'purpose',purpose) AS value FROM database_generations ORDER BY created_at LIMIT 1",
    )
    .await?
    .into_iter()
    .next();
    let mut characterization = BTreeMap::new();
    for work_item_id in work_item_ids {
        characterization.insert(
            work_item_id.clone(),
            export_work_item_characterization(&backup_pool, work_item_id).await?,
        );
    }
    let database_sha256 = sha256_file(&backup_path)?;
    let manifest = json!({
        "schema_version":"pharness.dev/database-archive/v1alpha1",
        "created_at_unix_millis":unix_millis().to_string(),
        "pharness_release_revision":option_env!("PHARNESS_BUILD_REVISION").unwrap_or("unknown"),
        "source_database_file_name":database.file_name().and_then(|value| value.to_str()).unwrap_or("pharness.db"),
        "archive_database_file_name":"pharness.db",
        "archive_database_sha256":database_sha256,
        "integrity_check":"ok",
        "database_generation":generation,
        "migrations":migrations,
        "table_counts":table_counts,
        "accepted_work_item_characterization":characterization,
        "credential_material_included":false,
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    std::fs::write(&manifest_path, &manifest_bytes)?;
    let manifest_sha256 = format!("sha256:{:x}", Sha256::digest(&manifest_bytes));
    println!(
        "{}",
        serde_json::to_string(&json!({
            "database":backup_path,
            "manifest":manifest_path,
            "database_sha256":database_sha256,
            "manifest_sha256":manifest_sha256,
        }))?
    );
    Ok(())
}

async fn verify_integrity(pool: &SqlitePool, label: &str) -> anyhow::Result<()> {
    let result: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?;
    if result != "ok" {
        anyhow::bail!("{label} database integrity_check failed: {result}");
    }
    Ok(())
}

async fn table_counts(pool: &SqlitePool) -> anyhow::Result<BTreeMap<String, i64>> {
    let tables = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    let mut counts = BTreeMap::new();
    for row in tables {
        let table: String = row.try_get("name")?;
        let quoted = table.replace('"', "\"\"");
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{quoted}\""))
            .fetch_one(pool)
            .await?;
        counts.insert(table, count);
    }
    Ok(counts)
}

async fn export_work_item_characterization(
    pool: &SqlitePool,
    work_item_id: &str,
) -> anyhow::Result<Value> {
    let work_item = bound_json_rows(
        pool,
        "SELECT json_object('id',id,'mode',mode,'status',status,'title',title,'product_id',product_id,'repository_id',mutable_repository_id,'source_commit',source_commit,'state_version',state_version,'closed_at',closed_at,'closure_reason',closure_reason) AS value FROM work_items WHERE id=?1",
        work_item_id,
    )
    .await?;
    let outcomes = bound_json_rows(
        pool,
        "SELECT json_object('id',id,'stage_execution_id',stage_execution_id,'stage_key',stage_key,'status',status,'schema_version',schema_version,'outcome',json(outcome_json),'content_hash',content_hash,'state_version',state_version,'supersedes_outcome_id',supersedes_outcome_id,'sealed_by',sealed_by,'sealed_at',sealed_at) AS value FROM stage_outcomes WHERE work_item_id=?1 ORDER BY sealed_at,id",
        work_item_id,
    )
    .await?;
    let validations = bound_json_rows(
        pool,
        "SELECT json_object('id',id,'stage_execution_id',stage_execution_id,'validator_key',validator_key,'schema_version',schema_version,'status',status,'subject',json(subject_json),'facts',json(facts_json),'contradictions',json(contradictions_json),'content_hash',content_hash,'validated_at',validated_at) AS value FROM evidence_validations WHERE work_item_id=?1 ORDER BY validated_at,id",
        work_item_id,
    )
    .await?;
    let references = bound_json_rows(
        pool,
        "SELECT json_object('id',r.id,'evidence_validation_id',r.evidence_validation_id,'reference_kind',r.reference_kind,'reference_id',r.reference_id,'reference_hash',r.reference_hash,'created_at',r.created_at) AS value FROM evidence_validation_references r JOIN evidence_validations v ON v.id=r.evidence_validation_id WHERE v.work_item_id=?1 ORDER BY r.created_at,r.id",
        work_item_id,
    )
    .await?;
    let deliveries = bound_json_rows(
        pool,
        "SELECT json_object('id',id,'repository_id',repository_id,'base_ref',base_ref,'base_commit',base_commit,'head_branch',head_branch,'patch_artifact_id',patch_artifact_id,'patch_hash',patch_hash,'status',status,'state_version',state_version,'pull_request',json(pull_request_json),'merge_provenance',json(merge_provenance_json),'provider_checks',json(provider_checks_json),'created_at',created_at,'updated_at',updated_at,'status_reason',status_reason) AS value FROM source_delivery_intents WHERE subject_kind='work_item' AND subject_id=?1 ORDER BY created_at,id",
        work_item_id,
    )
    .await?;
    let provider_observations = bound_json_rows(
        pool,
        "SELECT json_object('id',o.id,'source_delivery_intent_id',o.source_delivery_intent_id,'phase',o.phase,'repository_id',o.repository_id,'pull_request_number',o.pull_request_number,'head_sha',o.head_sha,'required_set_hash',o.required_set_hash,'authoritative_rules_succeeded',o.authoritative_rules_succeeded,'status',o.status,'required_checks',json(o.required_checks_json),'check_runs',json(o.check_runs_json),'commit_statuses',json(o.commit_statuses_json),'content_hash',o.content_hash,'observed_at',o.observed_at,'expires_at',o.expires_at) AS value FROM provider_check_set_observations o JOIN source_delivery_intents d ON d.id=o.source_delivery_intent_id WHERE d.subject_kind='work_item' AND d.subject_id=?1 ORDER BY o.observed_at,o.id",
        work_item_id,
    )
    .await?;
    let actions = bound_json_rows(
        pool,
        "SELECT json_object('id',id,'kind',kind,'actor',actor,'resource_kind',resource_kind,'resource_id',resource_id,'run_id',run_id,'created_at',created_at) AS value FROM audit_events WHERE resource_id=?1 ORDER BY created_at,id",
        work_item_id,
    )
    .await?;
    Ok(json!({
        "work_item":work_item,
        "controller_actions":actions,
        "stage_outcomes":outcomes,
        "evidence_validations":validations,
        "evidence_references":references,
        "source_deliveries":deliveries,
        "provider_observations":provider_observations,
    }))
}

async fn json_rows(pool: &SqlitePool, query: &str) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(query).fetch_all(pool).await?;
    decode_json_rows(rows)
}

async fn bound_json_rows(
    pool: &SqlitePool,
    query: &str,
    value: &str,
) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(query).bind(value).fetch_all(pool).await?;
    decode_json_rows(rows)
}

fn decode_json_rows(rows: Vec<sqlx::sqlite::SqliteRow>) -> anyhow::Result<Vec<Value>> {
    rows.into_iter()
        .map(|row| {
            let value: String = row.try_get("value")?;
            Ok(serde_json::from_str(&value)?)
        })
        .collect()
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharness_store::SqliteStore;

    #[tokio::test]
    async fn archive_uses_sqlite_backup_and_emits_a_verified_manifest() {
        let root = std::env::temp_dir().join(format!(
            "pharness-admin-archive-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        let database = root.join("source.db");
        let output = root.join("archive");
        std::fs::create_dir_all(&root).unwrap();
        let store = SqliteStore::connect(&database).await.unwrap();
        store
            .ensure_database_generation("dbgen_archive_test", "revision", "archive test")
            .await
            .unwrap();
        drop(store);
        let seed_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&database)
                    .create_if_missing(false),
            )
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO work_items (
              id,status,title,intent,acceptance_criteria_json,source_repo,source_ref,
              target_environment,production_impacting,max_attempts,max_elapsed_seconds,
              attempt_count,created_at,updated_at,status_changed_at,mode,state_version,
              closed_at,closure_reason
            ) VALUES (
              'witem_archive','completed','archive','characterize accepted evidence','[]',
              'https://github.com/example/repo','main','repository',0,2,3600,0,
              '1','1','1','repo',1,'1','merged'
            )
            "#,
        )
        .execute(&seed_pool)
        .await
        .unwrap();
        seed_pool.close().await;

        archive(&database, &output, &["witem_archive".into()])
            .await
            .unwrap();

        let manifest: Value =
            serde_json::from_slice(&std::fs::read(output.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["integrity_check"], "ok");
        assert_eq!(manifest["database_generation"]["id"], "dbgen_archive_test");
        assert_eq!(manifest["credential_material_included"], false);
        assert!(manifest["archive_database_sha256"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert_eq!(
            manifest["accepted_work_item_characterization"]["witem_archive"]["work_item"][0]["id"],
            "witem_archive"
        );
        assert_eq!(
            manifest["accepted_work_item_characterization"]["witem_archive"]["work_item"][0]
                ["repository_id"],
            Value::Null
        );
        assert!(output.join("pharness.db").is_file());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
