use super::{artifact_id, evidence, now, preparation, ApiError, AppState};
use pharness_core::tools::{
    FinanceApplication, FinanceDeploymentExpectation, FinanceEnvironment, FinanceRuntimeWindow,
};
use pharness_store::StoredArtifact;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(super) struct Context<'a> {
    pub saved: &'a preparation::Saved,
    pub expected: FinanceDeploymentExpectation,
    pub binding: Value,
    pub commit_checked_at_ms: i64,
}

impl<'a> Context<'a> {
    pub async fn new(
        state: &AppState,
        saved: &'a preparation::Saved,
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
        let application = match saved.authority.application_repository.as_str() {
            "https://github.com/lward27/yfinance_wrapper.git" => FinanceApplication::Yfinance,
            "https://github.com/lward27/finance-frontend.git" => FinanceApplication::Frontend,
            _ => {
                return Err(ApiError::conflict(
                    "staging runtime requires a registered Finance application",
                ))
            }
        };
        // The first accepted GitHub observation proves inclusion of the admitted
        // commit and unchanged file at this revision. No later revision is guessed.
        let expected = FinanceDeploymentExpectation {
            application,
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
            saved,
            expected,
            binding: json!({"authority_hash":saved.authority.material_hash().map_err(ApiError::conflict)?,"plan_hash":plan.material_hash().map_err(ApiError::conflict)?,"commit_artifact_id":receipt.id,"commit_artifact_sha256":super::super::hash(body)?,"admitted_gitops_commit_sha":identity["gitops_commit_sha"],"baseline":baseline}),
            commit_checked_at_ms: body["checked_at_ms"].as_i64().unwrap(),
        })
    }

    pub fn id(&self, step: &str) -> String {
        artifact_id(
            &format!("runtime_{step}"),
            &self.saved.authority.execution_id,
        )
    }

    pub async fn read(&self, state: &AppState, step: &str) -> Result<Option<Value>, ApiError> {
        let Some(record) = state.store.get_artifact(&self.id(step)).await? else {
            return Ok(None);
        };
        if record.kind != format!("hosted_staging_runtime_{step}")
            || record.session_id != self.saved.pipeline.session_id
            || record.run_id != self.saved.pipeline.run_id
        {
            return Err(ApiError::conflict(
                "staging runtime observation has different native origin",
            ));
        }
        let body = record
            .content_json
            .ok_or_else(|| ApiError::conflict("staging runtime observation has no content"))?;
        if body["binding"] != self.binding || body["expected"] != json!(self.expected) {
            return Err(ApiError::conflict(
                "staging runtime observation is bound to another commit, image or authority",
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
            &self.saved.pipeline,
            &self.id(step),
            &format!("hosted_staging_runtime_{step}"),
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
pub(super) struct Window {
    pub created_at_ms: i64,
    pub window: FinanceRuntimeWindow,
    pub initial_step: String,
}

impl Window {
    pub fn new(
        context: &Context<'_>,
        created: i64,
        initial_step: String,
    ) -> Result<Self, ApiError> {
        let window =
            FinanceRuntimeWindow::starting_after(created.saturating_add(30_000) as u64, 300)
                .map_err(|_| ApiError::conflict("staging runtime window is invalid"))?;
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
    pub fn validate(&self, context: &Context<'_>) -> Result<(), ApiError> {
        let window = FinanceRuntimeWindow::starting_after(
            self.created_at_ms.saturating_add(30_000) as u64,
            300,
        )
        .map_err(|_| ApiError::conflict("staging runtime window is invalid"))?;
        let step = self
            .initial_step
            .strip_prefix("identity_")
            .and_then(|s| s.parse::<u32>().ok());
        if self.created_at_ms < context.saved.operation.created_at
            || self.created_at_ms < context.commit_checked_at_ms
            || self.created_at_ms > now()
            || self.window != window
            || self.expires_at() >= context.saved.authority.expires_at_ms
            || !step.is_some_and(|n| (1..=crate::app::CONTROLLER_WAIT_MAX_CHECKS).contains(&n))
            || step.is_some_and(|n| self.initial_step != format!("identity_{n}"))
        {
            return Err(ApiError::conflict(
                "staging runtime window exceeds its original operation or identity allowance",
            ));
        }
        Ok(())
    }
}
