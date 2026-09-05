//! Deployment records remain scoped to their workflow and promotion stage.
use super::*;
use crate::DeliveryStage;

impl SqliteStore {
    pub async fn create_deployment_intent(
        &self,
        intent: CreateDeploymentIntent,
    ) -> Result<StoredDeploymentIntent, StoreError> {
        self.create_deployment_intent_for_stage(intent, DeliveryStage::Legacy)
            .await
    }

    /// Creates state only. The hosted controller must supply the sealed policy,
    /// source/build evidence and authority before admitting an external effect.
    pub async fn create_deployment_intent_for_stage(
        &self,
        intent: CreateDeploymentIntent,
        stage: DeliveryStage,
    ) -> Result<StoredDeploymentIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&intent.intent_json)?;
        sqlx::query(
            r#"
            INSERT INTO deployment_intents (
              id, pipeline_intent_id, change_set_id, work_plan_id, remediation_plan_id,
              incident_id, session_id, run_id, status, title, summary, risk_level, intent_kind,
              target_environment, target_namespace, argo_application, resource_namespace,
              resource_kind, resource_name, intent_json, created_at, updated_at, status_changed_at,
              delivery_stage
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
            "#,
        )
        .bind(&intent.id)
        .bind(&intent.pipeline_intent_id)
        .bind(&intent.change_set_id)
        .bind(&intent.work_plan_id)
        .bind(&intent.remediation_plan_id)
        .bind(&intent.incident_id)
        .bind(intent.session_id.as_str())
        .bind(intent.run_id.as_ref().map(RunId::as_str))
        .bind(&intent.status)
        .bind(&intent.title)
        .bind(&intent.summary)
        .bind(&intent.risk_level)
        .bind(&intent.intent_kind)
        .bind(intent.target_environment)
        .bind(intent.target_namespace)
        .bind(intent.argo_application)
        .bind(intent.resource_namespace)
        .bind(intent.resource_kind)
        .bind(intent.resource_name)
        .bind(intent_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .bind(stage.as_str())
        .execute(&self.pool)
        .await?;

        self.get_deployment_intent(&intent.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_intent".to_string(),
                id: intent.id,
            })
    }

    pub async fn get_deployment_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<StoredDeploymentIntent>, StoreError> {
        let row = sqlx::query(deployment_intent_select_sql("WHERE id = ?1"))
            .bind(intent_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_deployment_intent).transpose()
    }

    /// Historical source-only read. Hosted readers must select a stage or use
    /// the paginated list; this never silently picks staging or production.
    pub async fn get_deployment_intent_by_pipeline_intent(
        &self,
        pipeline_intent_id: &str,
    ) -> Result<Option<StoredDeploymentIntent>, StoreError> {
        self.get_deployment_intent_for_stage(pipeline_intent_id, DeliveryStage::Legacy)
            .await
    }

    pub async fn get_deployment_intent_for_stage(
        &self,
        pipeline_intent_id: &str,
        stage: DeliveryStage,
    ) -> Result<Option<StoredDeploymentIntent>, StoreError> {
        let row = sqlx::query(deployment_intent_select_sql(
            "WHERE pipeline_intent_id = ?1 AND delivery_stage = ?2",
        ))
        .bind(pipeline_intent_id)
        .bind(stage.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_deployment_intent).transpose()
    }

    pub async fn list_deployment_intents(
        &self,
        filter: DeploymentIntentListFilter,
    ) -> Result<Vec<StoredDeploymentIntent>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, pipeline_intent_id, delivery_stage, change_set_id, work_plan_id, remediation_plan_id,
                   incident_id, session_id, run_id, status, title, summary, risk_level,
                   intent_kind, target_environment, target_namespace, argo_application,
                   resource_namespace, resource_kind, resource_name, intent_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM deployment_intents
            WHERE (?1 IS NULL OR pipeline_intent_id = ?1)
              AND (?2 IS NULL OR change_set_id = ?2)
              AND (?3 IS NULL OR work_plan_id = ?3)
              AND (?4 IS NULL OR remediation_plan_id = ?4)
              AND (?5 IS NULL OR incident_id = ?5)
              AND (?6 IS NULL OR run_id = ?6)
              AND (?7 IS NULL OR status = ?7)
              AND (?8 IS NULL OR intent_kind = ?8)
              AND (?9 IS NULL OR risk_level = ?9)
              AND (?10 IS NULL OR target_environment = ?10)
              AND (?11 IS NULL OR target_namespace = ?11)
              AND (?12 IS NULL OR argo_application = ?12)
              AND (?13 IS NULL OR resource_namespace = ?13)
              AND (?14 IS NULL OR resource_kind = ?14)
              AND (?15 IS NULL OR resource_name = ?15)
              AND (?16 IS NULL OR CAST(created_at AS INTEGER) >= ?16)
              AND (?17 IS NULL OR CAST(created_at AS INTEGER) <= ?17)
            ORDER BY created_at DESC, id DESC
            LIMIT ?18 OFFSET ?19
            "#,
        )
        .bind(filter.pipeline_intent_id)
        .bind(filter.change_set_id)
        .bind(filter.work_plan_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.intent_kind)
        .bind(filter.risk_level)
        .bind(filter.target_environment)
        .bind(filter.target_namespace)
        .bind(filter.argo_application)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_deployment_intent).collect()
    }
}

pub(super) fn read_stage(row: &sqlx::sqlite::SqliteRow) -> Result<DeliveryStage, StoreError> {
    match row.try_get::<&str, _>("delivery_stage")? {
        "legacy" => Ok(DeliveryStage::Legacy),
        "staging" => Ok(DeliveryStage::Staging),
        "production" => Ok(DeliveryStage::Production),
        _ => Err(StoreError::InvalidData("unknown deployment stage".into())),
    }
}

#[cfg(test)]
mod tests;
