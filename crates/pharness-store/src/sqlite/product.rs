use super::{now_string, SqliteStore, StoreError};
use crate::{
    BootstrapOrganization, CreateProductAggregate, CreateRepositoryOnboarding,
    RegisterRepositoryAggregate, RegisteredRepositoryAggregate, StoredOrganization, StoredProduct,
    StoredProductModelSnapshot, StoredRepository, StoredRepositoryBinding,
    StoredRepositoryBindingRevision, StoredService, UpdateProductAggregate,
};
use sqlx::{Row, Sqlite, Transaction};

impl SqliteStore {
    pub async fn ensure_bootstrap_organization(
        &self,
        organization: &BootstrapOrganization,
    ) -> Result<StoredOrganization, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO organizations (id, organization_key, display_name, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            ON CONFLICT(id) DO UPDATE SET
              display_name = excluded.display_name,
              updated_at = excluded.updated_at
            "#,
        )
        .bind(&organization.id)
        .bind(&organization.organization_key)
        .bind(&organization.display_name)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_constraint("organization"))?;
        self.get_organization(&organization.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "organization".into(),
                id: organization.id.clone(),
            })
    }

    pub async fn get_organization(
        &self,
        id: &str,
    ) -> Result<Option<StoredOrganization>, StoreError> {
        let row = sqlx::query(
            "SELECT id, organization_key, display_name, created_at, updated_at FROM organizations WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_organization).transpose()
    }

    pub async fn create_product(
        &self,
        product: CreateProductAggregate,
    ) -> Result<StoredProduct, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO products (
              id, organization_id, product_key, display_name, description,
              owner_principal, state_version, current_model_snapshot_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, NULL, ?7, ?7)
            "#,
        )
        .bind(&product.id)
        .bind(&product.organization_id)
        .bind(&product.product_key)
        .bind(&product.display_name)
        .bind(&product.description)
        .bind(&product.owner_principal)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint("product"))?;
        insert_product_snapshot(
            &mut tx,
            &product.snapshot_id,
            &product.id,
            1,
            &product.snapshot_json,
            &product.snapshot_hash,
            &product.actor,
            &product.reason,
            &now,
        )
        .await?;
        sqlx::query("UPDATE products SET current_model_snapshot_id = ?2 WHERE id = ?1")
            .bind(&product.id)
            .bind(&product.snapshot_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get_product(&product.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "product".into(),
                id: product.id,
            })
    }

    pub async fn list_products(
        &self,
        organization_id: &str,
    ) -> Result<Vec<StoredProduct>, StoreError> {
        let rows = sqlx::query(&product_select_sql(
            "WHERE organization_id = ?1 ORDER BY display_name, id",
        ))
        .bind(organization_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_product).collect()
    }

    pub async fn get_product(&self, id: &str) -> Result<Option<StoredProduct>, StoreError> {
        let row = sqlx::query(&product_select_sql("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_product).transpose()
    }

    pub async fn update_product(
        &self,
        product: UpdateProductAggregate,
    ) -> Result<StoredProduct, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        insert_product_snapshot(
            &mut tx,
            &product.snapshot_id,
            &product.id,
            product.expected_state_version + 1,
            &product.snapshot_json,
            &product.snapshot_hash,
            &product.actor,
            &product.reason,
            &now,
        )
        .await?;
        let updated = sqlx::query(
            r#"
            UPDATE products
            SET product_key = ?3,
                display_name = ?4,
                description = ?5,
                owner_principal = ?6,
                state_version = state_version + 1,
                current_model_snapshot_id = ?7,
                updated_at = ?8
            WHERE id = ?1 AND state_version = ?2
            "#,
        )
        .bind(&product.id)
        .bind(product.expected_state_version as i64)
        .bind(&product.product_key)
        .bind(&product.display_name)
        .bind(&product.description)
        .bind(&product.owner_principal)
        .bind(&product.snapshot_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint("product"))?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "product changed after update preview".into(),
            ));
        }
        tx.commit().await?;
        self.get_product(&product.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "product".into(),
                id: product.id,
            })
    }

    pub async fn get_product_model_snapshot(
        &self,
        id: &str,
    ) -> Result<Option<StoredProductModelSnapshot>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, product_id, version, model_json, content_hash,
                   created_by, creation_reason, created_at
            FROM product_model_snapshots WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_product_snapshot).transpose()
    }

    pub async fn get_repository_by_provider_identity(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<StoredRepository>, StoreError> {
        let row = sqlx::query(&repository_select_sql(
            "WHERE provider = ?1 AND external_id = ?2",
        ))
        .bind(provider)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_repository).transpose()
    }

    pub async fn get_repository(&self, id: &str) -> Result<Option<StoredRepository>, StoreError> {
        let row = sqlx::query(&repository_select_sql("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_repository).transpose()
    }

    pub async fn list_product_repositories(
        &self,
        product_id: &str,
    ) -> Result<Vec<StoredRepository>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT r.id, r.provider, r.external_id, r.canonical_url, r.default_branch,
                   r.registered_commit, r.state_version, r.created_at, r.updated_at
            FROM repositories r
            JOIN repository_bindings b ON b.repository_id = r.id
            WHERE b.product_id = ?1 AND b.status = 'active'
            ORDER BY r.external_id, r.id
            "#,
        )
        .bind(product_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_repository).collect()
    }

    pub async fn list_product_services(
        &self,
        product_id: &str,
    ) -> Result<Vec<StoredService>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, product_id, service_key, display_name, description, status,
                   created_at, updated_at
            FROM services WHERE product_id = ?1 ORDER BY display_name, id
            "#,
        )
        .bind(product_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_service).collect()
    }

    pub async fn get_repository_binding(
        &self,
        product_id: &str,
        repository_id: &str,
    ) -> Result<Option<StoredRepositoryBinding>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, product_id, repository_id, status, current_revision_id,
                   created_at, updated_at
            FROM repository_bindings
            WHERE product_id = ?1 AND repository_id = ?2
            "#,
        )
        .bind(product_id)
        .bind(repository_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_binding).transpose()
    }

    pub async fn list_product_repository_bindings(
        &self,
        product_id: &str,
    ) -> Result<Vec<StoredRepositoryBinding>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, product_id, repository_id, status, current_revision_id,
                   created_at, updated_at
            FROM repository_bindings
            WHERE product_id = ?1
            ORDER BY created_at, id
            "#,
        )
        .bind(product_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_binding).collect()
    }

    pub async fn register_repository(
        &self,
        registration: RegisterRepositoryAggregate,
    ) -> Result<RegisteredRepositoryAggregate, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query(&repository_select_sql(
            "WHERE provider = ?1 AND external_id = ?2",
        ))
        .bind(&registration.repository.provider)
        .bind(&registration.repository.external_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(row_to_repository)
        .transpose()?;
        let repository = if let Some(existing) = existing {
            if existing.canonical_url != registration.repository.canonical_url {
                return Err(StoreError::Conflict(
                    "provider repository identity resolved to a different canonical URL".into(),
                ));
            }
            existing
        } else {
            sqlx::query(
                r#"
                INSERT INTO repositories (
                  id, provider, external_id, canonical_url, default_branch,
                  registered_commit, state_version, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)
                "#,
            )
            .bind(&registration.repository.id)
            .bind(&registration.repository.provider)
            .bind(&registration.repository.external_id)
            .bind(&registration.repository.canonical_url)
            .bind(&registration.repository.default_branch)
            .bind(&registration.repository.registered_commit)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(map_constraint("repository"))?;
            StoredRepository {
                id: registration.repository.id.clone(),
                provider: registration.repository.provider.clone(),
                external_id: registration.repository.external_id.clone(),
                canonical_url: registration.repository.canonical_url.clone(),
                default_branch: registration.repository.default_branch.clone(),
                registered_commit: registration.repository.registered_commit.clone(),
                state_version: 1,
                created_at: now.clone(),
                updated_at: now.clone(),
            }
        };

        sqlx::query(
            r#"
            INSERT INTO repository_bindings (
              id, product_id, repository_id, status, current_revision_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?5)
            "#,
        )
        .bind(&registration.binding_id)
        .bind(&registration.product_id)
        .bind(&repository.id)
        .bind(&registration.binding_revision_id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint("repository binding"))?;
        sqlx::query(
            r#"
            INSERT INTO repository_binding_revisions (
              id, binding_id, revision, service_ids_json, scopes_json, status,
              evidence_json, content_hash, reviewed_by, review_reason, created_at
            ) VALUES (?1, ?2, 1, '[]', '["**"]', 'reviewed', ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&registration.binding_revision_id)
        .bind(&registration.binding_id)
        .bind(serde_json::to_string(&registration.evidence_json)?)
        .bind(&registration.binding_content_hash)
        .bind(&registration.actor)
        .bind(&registration.reason)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        let snapshot_version = registration.expected_product_state_version + 1;
        insert_product_snapshot(
            &mut tx,
            &registration.snapshot_id,
            &registration.product_id,
            snapshot_version,
            &registration.snapshot_json,
            &registration.snapshot_hash,
            &registration.actor,
            &registration.reason,
            &now,
        )
        .await?;
        let updated = sqlx::query(
            r#"
            UPDATE products
            SET state_version = state_version + 1,
                current_model_snapshot_id = ?3,
                updated_at = ?4
            WHERE id = ?1 AND state_version = ?2
            "#,
        )
        .bind(&registration.product_id)
        .bind(registration.expected_product_state_version as i64)
        .bind(&registration.snapshot_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "product changed after repository registration preview".into(),
            ));
        }
        super::onboarding::insert_repository_onboarding(
            &mut tx,
            &CreateRepositoryOnboarding {
                id: registration.onboarding_id.clone(),
                product_id: registration.product_id.clone(),
                repository_id: repository.id.clone(),
                binding_id: registration.binding_id.clone(),
                onboarding_kind: "initial".into(),
                registered_commit: registration.repository.registered_commit.clone(),
                actor: registration.actor.clone(),
                reason: registration.reason.clone(),
            },
            &now,
        )
        .await?;
        tx.commit().await?;

        let repository_id = repository.id.clone();

        Ok(RegisteredRepositoryAggregate {
            repository,
            binding: self
                .get_repository_binding(&registration.product_id, &repository_id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "repository_binding".into(),
                    id: registration.binding_id.clone(),
                })?,
            binding_revision: self
                .get_repository_binding_revision(&registration.binding_revision_id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "repository_binding_revision".into(),
                    id: registration.binding_revision_id.clone(),
                })?,
            snapshot: self
                .get_product_model_snapshot(&registration.snapshot_id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "product_model_snapshot".into(),
                    id: registration.snapshot_id,
                })?,
            onboarding: self
                .get_repository_onboarding(&registration.onboarding_id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "repository_onboarding".into(),
                    id: registration.onboarding_id,
                })?,
        })
    }

    pub async fn get_repository_binding_revision(
        &self,
        id: &str,
    ) -> Result<Option<StoredRepositoryBindingRevision>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, binding_id, revision, service_ids_json, scopes_json, status,
                   evidence_json, content_hash, reviewed_by, review_reason, created_at
            FROM repository_binding_revisions WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_binding_revision).transpose()
    }
}

#[allow(clippy::too_many_arguments)]
async fn insert_product_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    product_id: &str,
    version: u64,
    model_json: &serde_json::Value,
    content_hash: &str,
    actor: &str,
    reason: &str,
    now: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        r#"
        INSERT INTO product_model_snapshots (
          id, product_id, version, model_json, content_hash,
          created_by, creation_reason, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
    )
    .bind(id)
    .bind(product_id)
    .bind(version as i64)
    .bind(serde_json::to_string(model_json)?)
    .bind(content_hash)
    .bind(actor)
    .bind(reason)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_constraint("product model snapshot"))?;
    Ok(())
}

fn map_constraint(
    entity: &'static str,
) -> impl FnOnce(sqlx::Error) -> StoreError + Send + Sync + 'static {
    move |error| match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            StoreError::Conflict(format!("{entity} already exists"))
        }
        _ => StoreError::Sqlx(error),
    }
}

fn product_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, organization_id, product_key, display_name, description, owner_principal, \
         state_version, current_model_snapshot_id, created_at, updated_at \
         FROM products {where_clause}"
    )
}

fn repository_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, provider, external_id, canonical_url, default_branch, registered_commit, \
         state_version, created_at, updated_at FROM repositories {where_clause}"
    )
}

fn row_to_organization(row: sqlx::sqlite::SqliteRow) -> Result<StoredOrganization, StoreError> {
    Ok(StoredOrganization {
        id: row.try_get("id")?,
        organization_key: row.try_get("organization_key")?,
        display_name: row.try_get("display_name")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_product(row: sqlx::sqlite::SqliteRow) -> Result<StoredProduct, StoreError> {
    let snapshot: Option<String> = row.try_get("current_model_snapshot_id")?;
    Ok(StoredProduct {
        id: row.try_get("id")?,
        organization_id: row.try_get("organization_id")?,
        product_key: row.try_get("product_key")?,
        display_name: row.try_get("display_name")?,
        description: row.try_get("description")?,
        owner_principal: row.try_get("owner_principal")?,
        state_version: row.try_get::<i64, _>("state_version")? as u64,
        current_model_snapshot_id: snapshot.ok_or_else(|| {
            StoreError::InvalidData("product has no current model snapshot".into())
        })?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_product_snapshot(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredProductModelSnapshot, StoreError> {
    let model: String = row.try_get("model_json")?;
    Ok(StoredProductModelSnapshot {
        id: row.try_get("id")?,
        product_id: row.try_get("product_id")?,
        version: row.try_get::<i64, _>("version")? as u64,
        model_json: serde_json::from_str(&model)?,
        content_hash: row.try_get("content_hash")?,
        created_by: row.try_get("created_by")?,
        creation_reason: row.try_get("creation_reason")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_repository(row: sqlx::sqlite::SqliteRow) -> Result<StoredRepository, StoreError> {
    Ok(StoredRepository {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        external_id: row.try_get("external_id")?,
        canonical_url: row.try_get("canonical_url")?,
        default_branch: row.try_get("default_branch")?,
        registered_commit: row.try_get("registered_commit")?,
        state_version: row.try_get::<i64, _>("state_version")? as u64,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_service(row: sqlx::sqlite::SqliteRow) -> Result<StoredService, StoreError> {
    Ok(StoredService {
        id: row.try_get("id")?,
        product_id: row.try_get("product_id")?,
        service_key: row.try_get("service_key")?,
        display_name: row.try_get("display_name")?,
        description: row.try_get("description")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_binding(row: sqlx::sqlite::SqliteRow) -> Result<StoredRepositoryBinding, StoreError> {
    let revision: Option<String> = row.try_get("current_revision_id")?;
    Ok(StoredRepositoryBinding {
        id: row.try_get("id")?,
        product_id: row.try_get("product_id")?,
        repository_id: row.try_get("repository_id")?,
        status: row.try_get("status")?,
        current_revision_id: revision.ok_or_else(|| {
            StoreError::InvalidData("repository binding has no current revision".into())
        })?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_binding_revision(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRepositoryBindingRevision, StoreError> {
    let services: String = row.try_get("service_ids_json")?;
    let scopes: String = row.try_get("scopes_json")?;
    let evidence: String = row.try_get("evidence_json")?;
    Ok(StoredRepositoryBindingRevision {
        id: row.try_get("id")?,
        binding_id: row.try_get("binding_id")?,
        revision: row.try_get::<i64, _>("revision")? as u64,
        service_ids: serde_json::from_str(&services)?,
        scopes: serde_json::from_str(&scopes)?,
        status: row.try_get("status")?,
        evidence_json: serde_json::from_str(&evidence)?,
        content_hash: row.try_get("content_hash")?,
        reviewed_by: row.try_get("reviewed_by")?,
        review_reason: row.try_get("review_reason")?,
        created_at: row.try_get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateProductAggregate, RegisterRepositoryAggregate, StoredRepositoryDraft};
    use serde_json::json;

    async fn seeded_store() -> SqliteStore {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        store
            .ensure_bootstrap_organization(&BootstrapOrganization {
                id: "org_test".into(),
                organization_key: "test".into(),
                display_name: "Test Organization".into(),
            })
            .await
            .unwrap();
        store
    }

    #[tokio::test]
    async fn product_creation_is_atomic_and_name_unique() {
        let store = seeded_store().await;
        let product = store
            .create_product(CreateProductAggregate {
                id: "prod_one".into(),
                organization_id: "org_test".into(),
                product_key: "orion".into(),
                display_name: "Orion".into(),
                description: "A product".into(),
                owner_principal: "operator".into(),
                snapshot_id: "pmodel_one".into(),
                snapshot_json: json!({"schema_version":"pharness.dev/product-model/v1alpha1"}),
                snapshot_hash: "sha256:one".into(),
                actor: "operator".into(),
                reason: "create product".into(),
            })
            .await
            .unwrap();
        assert_eq!(product.state_version, 1);
        assert_eq!(product.current_model_snapshot_id, "pmodel_one");
        assert!(store
            .get_product_model_snapshot("pmodel_one")
            .await
            .unwrap()
            .is_some());

        let duplicate = store
            .create_product(CreateProductAggregate {
                id: "prod_two".into(),
                organization_id: "org_test".into(),
                product_key: "orion".into(),
                display_name: "Orion duplicate".into(),
                description: "Another product".into(),
                owner_principal: "operator".into(),
                snapshot_id: "pmodel_two".into(),
                snapshot_json: json!({}),
                snapshot_hash: "sha256:two".into(),
                actor: "operator".into(),
                reason: "duplicate".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(duplicate, StoreError::Conflict(_)));
        assert!(store
            .get_product_model_snapshot("pmodel_two")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn repository_registration_versions_binding_and_product_model() {
        let store = seeded_store().await;
        store
            .create_product(CreateProductAggregate {
                id: "prod_one".into(),
                organization_id: "org_test".into(),
                product_key: "orion".into(),
                display_name: "Orion".into(),
                description: "A product".into(),
                owner_principal: "operator".into(),
                snapshot_id: "pmodel_one".into(),
                snapshot_json: json!({"repositories":[]}),
                snapshot_hash: "sha256:one".into(),
                actor: "operator".into(),
                reason: "create product".into(),
            })
            .await
            .unwrap();
        let registered = store
            .register_repository(RegisterRepositoryAggregate {
                repository: StoredRepositoryDraft {
                    id: "repo_one".into(),
                    provider: "github".into(),
                    external_id: "example/orion".into(),
                    canonical_url: "https://github.com/example/orion.git".into(),
                    default_branch: "main".into(),
                    registered_commit: "a".repeat(40),
                },
                binding_id: "rbind_one".into(),
                binding_revision_id: "rbindrev_one".into(),
                onboarding_id: "onbd_one".into(),
                binding_content_hash: "sha256:binding".into(),
                evidence_json: json!({"source_commit":"a".repeat(40)}),
                product_id: "prod_one".into(),
                expected_product_state_version: 1,
                snapshot_id: "pmodel_two".into(),
                snapshot_json: json!({"repositories":["repo_one"]}),
                snapshot_hash: "sha256:two".into(),
                actor: "operator".into(),
                reason: "register repository".into(),
            })
            .await
            .unwrap();
        assert_eq!(registered.repository.id, "repo_one");
        assert_eq!(registered.binding_revision.scopes, vec!["**"]);
        assert_eq!(registered.snapshot.version, 2);
        let product = store.get_product("prod_one").await.unwrap().unwrap();
        assert_eq!(product.state_version, 2);
        assert_eq!(product.current_model_snapshot_id, "pmodel_two");
    }
}
