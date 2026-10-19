//! One admitted GitOps update, followed only by bounded read-only observation.
//! The transport is shared by staging and production; each environment supplies
//! its own authority, plan and admission rules.
use anyhow::{Context as _, Result};
use pharness_core::hosted_sdlc::gitops_delivery::{GitOpsAuthority, GitOpsPlan};
use pharness_core::hosted_sdlc::staging::StagingGitOpsPlan;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) mod admission;
pub(crate) mod github;
use admission::Admission;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, bound = "")]
pub(crate) struct DeliveryContext<P: GitOpsPlan> {
    pub(crate) authority: P::Authority,
    pub(crate) authority_hash: String,
    pub(crate) plan: Option<P>,
    pub(crate) plan_hash: Option<String>,
    pub(crate) may_advance: bool,
    pub(crate) admission_recorded: bool,
}
#[cfg(test)]
pub(crate) type StagingContext = DeliveryContext<StagingGitOpsPlan>;

pub(crate) struct Transport {
    pub(crate) client: reqwest::Client,
    pub(crate) url: String,
    pub(crate) token: String,
    pub(crate) execution: String,
    pub(crate) deployment: String,
    pub(crate) observe_only: bool,
}

impl Transport {
    fn env<P: GitOpsPlan>() -> &'static str {
        P::Authority::ENVIRONMENT
    }

    pub(crate) async fn context<P: GitOpsPlan>(&self) -> Result<DeliveryContext<P>> {
        let env = Self::env::<P>();
        let context: DeliveryContext<P> = super::fetch_internal_context_with_retry(
            &self.client,
            &format!(
                "{}/context?execution_id={}&observe_only={}",
                self.url, self.execution, self.observe_only
            ),
            &self.token,
        )
        .await?;
        context
            .authority
            .validate_identity()
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            context.authority.execution_id() == self.execution
                && context.authority.deployment_intent_id() == self.deployment
                && context
                    .authority
                    .material_hash()
                    .map_err(anyhow::Error::msg)?
                    == context.authority_hash,
            "{env}_authority_mismatch"
        );
        if let Some(plan) = &context.plan {
            plan.validate(&context.authority)
                .map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                context.plan_hash.as_deref()
                    == Some(plan.material_hash().map_err(anyhow::Error::msg)?.as_str()),
                "{env}_plan_hash_mismatch"
            );
        } else {
            anyhow::ensure!(
                context.plan_hash.is_none() && !context.admission_recorded,
                "{env}_plan_missing"
            );
        }
        Ok(context)
    }

    pub(crate) async fn post_once(&self, env: &str, action: &str, body: &Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/{action}", self.url))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("{env}_api_acknowledgement_unknown"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "{env}_api_admission_not_acknowledged"
        );
        github::bounded_json(response, env).await
    }

    pub(crate) async fn run<P>(
        &self,
        git: &github::GitHub,
        initial: &DeliveryContext<P>,
    ) -> Result<Value>
    where
        P: GitOpsPlan,
        P::Authority: Admission<P>,
    {
        let env = Self::env::<P>();
        if self.observe_only || initial.admission_recorded {
            let plan = initial
                .plan
                .as_ref()
                .with_context(|| format!("{env}_plan_missing"))?;
            return observe(git, &initial.authority, plan, None).await;
        }
        if initial.plan.is_none() {
            let plan: P = git.plan(&initial.authority).await?;
            plan.validate_for_dispatch(&initial.authority, now())
                .map_err(anyhow::Error::msg)?;
            let response = self.post_once(env, "plan", &json!({"execution_id":self.execution,"authority_hash":initial.authority_hash,"plan":plan})).await?;
            anyhow::ensure!(
                response["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?,
                "{env}_plan_not_acknowledged"
            );
        }
        // Pausing cannot renew the saved deadline. No GitHub mutation occurs
        // while waiting, and cancellation is represented by may_advance=false.
        let (context, plan) = loop {
            let context = self.context::<P>().await?;
            anyhow::ensure!(
                context.authority_hash == initial.authority_hash,
                "{env}_authority_changed"
            );
            let plan = context
                .plan
                .clone()
                .with_context(|| format!("{env}_plan_missing"))?;
            plan.validate_for_dispatch(&context.authority, now())
                .map_err(anyhow::Error::msg)?;
            if context.admission_recorded {
                return observe(git, &context.authority, &plan, None).await;
            }
            if context.may_advance {
                break (context, plan);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        };
        anyhow::ensure!(
            git.branch().await? == plan.base_commit_sha(),
            "{env}_gitops_base_changed"
        );
        let admission = self.post_once(env, "attempt", &json!({"execution_id":self.execution,"authority_hash":context.authority_hash,"plan_hash":context.plan_hash})).await?;
        anyhow::ensure!(
            admission["admitted"] == true
                && admission["authority_hash"] == context.authority_hash
                && admission["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?,
            "{env}_admission_not_acknowledged"
        );
        // Never retry the mutation, including after an unknown response.
        context
            .authority
            .validate_admission(&admission, &plan, now())?;
        let known = git.commit_once(&context.authority, &plan, now()).await.ok();
        observe(git, &context.authority, &plan, known.as_deref()).await
    }
}

async fn observe<P: GitOpsPlan>(
    git: &github::GitHub,
    authority: &P::Authority,
    plan: &P,
    known: Option<&str>,
) -> Result<Value> {
    let mut last = None;
    for attempt in 0..12 {
        if now() >= authority.expires_at_ms() {
            break;
        }
        match git.observe(authority, plan, known).await {
            Ok(mut result) => {
                result["observation_attempts"] = json!(attempt + 1);
                return Ok(result);
            }
            Err(error) => last = Some(error),
        }
        if attempt < 11 {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    Err(last.unwrap_or_else(|| {
        anyhow::anyhow!("{}_observation_window_expired", P::Authority::ENVIRONMENT)
    }))
}

/// Shared tail: run the delivery, then report exactly one outcome record whose
/// error code is a bounded identifier rather than a provider message.
pub(crate) async fn execute_delivery<P>(transport: Transport, git: github::GitHub) -> Result<()>
where
    P: GitOpsPlan,
    P::Authority: Admission<P>,
{
    let env = P::Authority::ENVIRONMENT;
    let initial = transport.context::<P>().await?;
    let mut outcome = match transport.run(&git, &initial).await {
        Ok(mut result) => {
            result
                .as_object_mut()
                .with_context(|| format!("{env}_observation_invalid"))?
                .remove("status");
            json!({"status":"committed","observation":result})
        }
        Err(error) => json!({"status":"unconfirmed","error_code":error_code(env, &error)}),
    };
    outcome["execution_id"] = json!(transport.execution);
    outcome["authority_hash"] = json!(initial.authority_hash);
    outcome["observe_only"] = json!(transport.observe_only);
    outcome["checked_at_ms"] = json!(now());
    super::post_git_delivery_outcome(
        &transport.client,
        &format!("{}/outcome", transport.url),
        &transport.token,
        &outcome,
    )
    .await
}

pub(crate) fn error_code(env: &str, error: &anyhow::Error) -> String {
    let message = error.to_string();
    if !message.is_empty()
        && message.len() <= 100
        && message.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        message
    } else {
        format!("{env}_observation_unconfirmed")
    }
}

pub(super) async fn execute() -> Result<()> {
    let api = super::required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let deployment = super::required_env("PHARNESS_DEPLOYMENT_INTENT_ID")?;
    let execution = super::required_env("PHARNESS_EXECUTION_ID")?;
    let token = super::required_env("PHARNESS_WORKER_TOKEN")?;
    let observe_only = std::env::var("PHARNESS_STAGING_OBSERVE_ONLY").as_deref() == Ok("true");
    let git = github::GitHub::new(
        super::required_env(if observe_only {
            "PHARNESS_GIT_OBSERVER_TOKEN"
        } else {
            "PHARNESS_GIT_WRITER_TOKEN"
        })?,
        "staging",
    )?;
    let transport = Transport {
        client: git.client.clone(),
        url: format!("{api}/api/internal/deployment-intents/{deployment}/staging"),
        token,
        execution,
        deployment,
        observe_only,
    };
    execute_delivery::<StagingGitOpsPlan>(transport, git).await
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
