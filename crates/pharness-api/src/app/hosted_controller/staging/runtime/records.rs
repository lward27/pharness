//! Durable records for one bounded Finance runtime window. The same records
//! serve staging (five minutes) and production (ten minutes); the prefix keeps
//! each environment's artifacts, operation refs and projections apart.
use super::{artifact_id, evidence, now, preparation, ApiError, AppState};
use pharness_core::tools::{
    FinanceApplication, FinanceDeploymentExpectation, FinanceEnvironment, FinanceRuntimeWindow,
    FinanceVerificationPhase,
};
use pharness_store::{
    StoredArtifact, StoredDeploymentIntent, StoredPipelineIntent, StoredWorkflowOperation,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(in crate::app::hosted_controller) struct Context {
    pub operation: StoredWorkflowOperation,
    pub deployment: StoredDeploymentIntent,
    pub pipeline: StoredPipelineIntent,
    pub work_item_id: String,
    pub execution_id: String,
    /// Latest instant any window may still complete, including ingestion.
    pub expires_at_ms: i64,
    pub expected: FinanceDeploymentExpectation,
    pub phase: FinanceVerificationPhase,
    /// Artifact, operation-ref and evidence-kind prefix (`staging`, `production`
    /// or `observe`).
    pub prefix: &'static str,
    /// Deployment intent_json object and key that receive the projection.
    pub projection: (&'static str, &'static str),
    pub binding: Value,
    /// Observations that started before this instant cannot count; it is the
    /// accepted time of the GitOps receipt (or of the previous verified window).
    pub commit_checked_at_ms: i64,
}

pub(in crate::app::hosted_controller) fn finance_application(
    application_repository: &str,
) -> Result<FinanceApplication, ApiError> {
    match application_repository {
        "https://github.com/lward27/yfinance_wrapper.git" => Ok(FinanceApplication::Yfinance),
        "https://github.com/lward27/finance-frontend.git" => Ok(FinanceApplication::Frontend),
        _ => Err(ApiError::conflict(
            "runtime verification requires a registered Finance application",
        )),
    }
}

impl Context {
    pub async fn new(
        state: &AppState,
        saved: &preparation::Saved,
        receipt: &StoredArtifact,
    ) -> Result<Self, ApiError> {
        let body = receipt.content_json.as_ref().ok_or_else(|| {
            ApiError::conflict("staging runtime requires its original commit receipt")
        })?;
        let plan = saved
            .plan
            .as_ref()
            .ok_or_else(|| ApiError::conflict("staging runtime requires its original plan"))?;
        let identity = evidence::validate_observation(
            &saved.authority,
            plan,
            &body["observation"],
            body["checked_at_ms"].as_i64().unwrap_or_default(),
        )?;
        if receipt.id != artifact_id("commit", &saved.authority.execution_id)
            || receipt.kind != "hosted_staging_commit"
            || receipt.session_id != saved.pipeline.session_id
            || receipt.run_id != saved.pipeline.run_id
            || body["identity"] != identity
            || body["authority_hash"]
                != saved
                    .authority
                    .material_hash()
                    .map_err(ApiError::conflict)?
        {
            return Err(ApiError::conflict(
                "staging runtime commit receipt has different authority or origin",
            ));
        }
        // The first accepted GitHub observation proves inclusion of the admitted
        // commit and unchanged file at this revision. No later revision is guessed.
        let expected = FinanceDeploymentExpectation {
            application: finance_application(&saved.authority.application_repository)?,
            environment: FinanceEnvironment::Staging,
            gitops_commit_sha: body["observation"]["current_gitops_revision"]
                .as_str()
                .unwrap()
                .into(),
            image_digest: saved.authority.image_digest.clone(),
        };
        expected.validate().map_err(ApiError::conflict)?;
        let baseline = super::super::baseline::historical(state, saved).await?;
        Ok(Self {
            operation: saved.operation.clone(),
            deployment: saved.deployment.clone(),
            pipeline: saved.pipeline.clone(),
            work_item_id: saved.authority.work_item_id.clone(),
            execution_id: saved.authority.execution_id.clone(),
            expires_at_ms: saved.authority.expires_at_ms,
            expected,
            phase: FinanceVerificationPhase::Staging,
            prefix: "staging",
            projection: ("hosted_staging", "runtime_observation"),
            binding: json!({"authority_hash":saved.authority.material_hash().map_err(ApiError::conflict)?,"plan_hash":plan.material_hash().map_err(ApiError::conflict)?,"commit_artifact_id":receipt.id,"commit_artifact_sha256":super::super::hash(body)?,"admitted_gitops_commit_sha":identity["gitops_commit_sha"],"baseline":baseline}),
            commit_checked_at_ms: body["checked_at_ms"].as_i64().unwrap(),
        })
    }

    pub fn duration_seconds(&self) -> u64 {
        match self.phase {
            FinanceVerificationPhase::Production => 600,
            FinanceVerificationPhase::Staging | FinanceVerificationPhase::Baseline => 300,
        }
    }

    pub fn id(&self, step: &str) -> String {
        format!("{}_runtime_{step}_{}", self.prefix, self.execution_id)
    }

    pub fn ref_key(&self, step: &str) -> String {
        format!("{}_runtime_{step}", self.prefix)
    }

    pub fn reason(&self, suffix: &str) -> String {
        format!("{}_{suffix}", self.prefix)
    }

    pub async fn read(&self, state: &AppState, step: &str) -> Result<Option<Value>, ApiError> {
        let Some(record) = state.store.get_artifact(&self.id(step)).await? else {
            return Ok(None);
        };
        if record.kind != format!("hosted_{}_runtime_{step}", self.prefix)
            || record.session_id != self.pipeline.session_id
            || record.run_id != self.pipeline.run_id
        {
            return Err(ApiError::conflict(
                "runtime observation has different native origin",
            ));
        }
        let body = record
            .content_json
            .ok_or_else(|| ApiError::conflict("runtime observation has no content"))?;
        if body["binding"] != self.binding || body["expected"] != json!(self.expected) {
            return Err(ApiError::conflict(
                "runtime observation is bound to another commit, image or authority",
            ));
        }
        Ok(Some(body))
    }

    pub async fn write(
        &self,
        state: &AppState,
        step: &str,
        mut body: Value,
    ) -> Result<(), ApiError> {
        body["binding"] = self.binding.clone();
        body["expected"] = json!(self.expected);
        evidence::artifact(
            state,
            &self.pipeline,
            &self.id(step),
            &format!("hosted_{}_runtime_{step}", self.prefix),
            body,
        )
        .await?;
        Ok(())
    }

    pub async fn begin(&self, state: &AppState, step: &str) -> Result<bool, ApiError> {
        let marker = format!("{step}_started");
        if self.read(state, &marker).await?.is_some() {
            return Ok(false);
        }
        self.write(state, &marker, json!({"started_at_ms":now()}))
            .await?;
        Ok(true)
    }

    pub async fn fail(&self, state: &AppState, reason: &str) -> Result<(), ApiError> {
        self.write(state,"result",json!({"completed_at_ms":now(),"runtime_evidence":null,"assessment":{"runtime_verification":"inconclusive","reasons":[reason],"work_item_completion":"not_evaluated"}})).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::hosted_controller) struct Window {
    pub created_at_ms: i64,
    pub window: FinanceRuntimeWindow,
    pub initial_step: String,
}

impl Window {
    pub fn new(context: &Context, created: i64, initial_step: String) -> Result<Self, ApiError> {
        let window = FinanceRuntimeWindow::starting_after(
            created.saturating_add(30_000) as u64,
            context.duration_seconds(),
        )
        .map_err(|_| ApiError::conflict("runtime window is invalid"))?;
        let value = Self {
            created_at_ms: created,
            window,
            initial_step,
        };
        value.validate(context)?;
        Ok(value)
    }
    pub fn expires_at(&self) -> i64 {
        (self.window.end_unix_seconds as i64) * 1000 + 60_000
    }
    pub fn validate(&self, context: &Context) -> Result<(), ApiError> {
        let window = FinanceRuntimeWindow::starting_after(
            self.created_at_ms.saturating_add(30_000) as u64,
            context.duration_seconds(),
        )
        .map_err(|_| ApiError::conflict("runtime window is invalid"))?;
        let step = self
            .initial_step
            .strip_prefix("identity_")
            .and_then(|s| s.parse::<u32>().ok());
        if self.created_at_ms < context.operation.created_at
            || self.created_at_ms < context.commit_checked_at_ms
            || self.created_at_ms > now()
            || self.window != window
            || self.expires_at() >= context.expires_at_ms
            || !step.is_some_and(|n| (1..=crate::app::CONTROLLER_WAIT_MAX_CHECKS).contains(&n))
            || step.is_some_and(|n| self.initial_step != format!("identity_{n}"))
        {
            return Err(ApiError::conflict(
                "runtime window exceeds its original operation or identity allowance",
            ));
        }
        Ok(())
    }
}
