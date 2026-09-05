//! One admitted staging update, followed only by bounded read-only observation.
use anyhow::{Context as _, Result};
use pharness_core::hosted_sdlc::staging::{HostedStagingAuthority, StagingGitOpsPlan};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod github;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StagingContext {
    authority: HostedStagingAuthority,
    authority_hash: String,
    plan: Option<StagingGitOpsPlan>,
    plan_hash: Option<String>,
    may_advance: bool,
    admission_recorded: bool,
}

struct Transport {
    client: reqwest::Client,
    url: String,
    token: String,
    execution: String,
    deployment: String,
    observe_only: bool,
}

impl Transport {
    async fn context(&self) -> Result<StagingContext> {
        let context: StagingContext = super::fetch_internal_context_with_retry(
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
            context.authority.execution_id == self.execution
                && context.authority.deployment_intent_id == self.deployment
                && context
                    .authority
                    .material_hash()
                    .map_err(anyhow::Error::msg)?
                    == context.authority_hash,
            "staging_authority_mismatch"
        );
        if let Some(plan) = &context.plan {
            plan.validate(&context.authority)
                .map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                context.plan_hash.as_deref()
                    == Some(plan.material_hash().map_err(anyhow::Error::msg)?.as_str()),
                "staging_plan_hash_mismatch"
            );
        } else {
            anyhow::ensure!(
                context.plan_hash.is_none() && !context.admission_recorded,
                "staging_plan_missing"
            );
        }
        Ok(context)
    }

    async fn post_once(&self, action: &str, body: &Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/{action}", self.url))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("staging_api_acknowledgement_unknown"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "staging_api_admission_not_acknowledged"
        );
        github::bounded_json(response).await
    }

    async fn run(&self, git: &github::GitHub, initial: &StagingContext) -> Result<Value> {
        if self.observe_only || initial.admission_recorded {
            let plan = initial.plan.as_ref().context("staging_plan_missing")?;
            return observe(git, &initial.authority, plan, None).await;
        }
        initial
            .authority
            .validate_for_dispatch(now())
            .map_err(anyhow::Error::msg)?;
        if initial.plan.is_none() {
            let plan = git.plan(&initial.authority).await?;
            let response = self.post_once("plan", &json!({"execution_id":self.execution,"authority_hash":initial.authority_hash,"plan":plan})).await?;
            anyhow::ensure!(
                response["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?,
                "staging_plan_not_acknowledged"
            );
        }
        // Pausing cannot renew the saved deadline. No GitHub mutation occurs
        // while waiting, and cancellation is represented by may_advance=false.
        let (context, plan) = loop {
            let context = self.context().await?;
            anyhow::ensure!(
                context.authority_hash == initial.authority_hash,
                "staging_authority_changed"
            );
            context
                .authority
                .validate_for_dispatch(now())
                .map_err(anyhow::Error::msg)?;
            let plan = context.plan.clone().context("staging_plan_missing")?;
            if context.admission_recorded {
                return observe(git, &context.authority, &plan, None).await;
            }
            if context.may_advance {
                break (context, plan);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        };
        anyhow::ensure!(
            git.branch().await? == plan.base_commit_sha,
            "staging_gitops_base_changed"
        );
        let admission = self.post_once("attempt", &json!({"execution_id":self.execution,"authority_hash":context.authority_hash,"plan_hash":context.plan_hash})).await?;
        anyhow::ensure!(
            admission["admitted"] == true
                && admission["authority_hash"] == context.authority_hash
                && admission["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?,
            "staging_admission_not_acknowledged"
        );
        // Never retry the mutation, including after an unknown response.
        let known = git.commit_once(&context.authority, &plan, now()).await.ok();
        observe(git, &context.authority, &plan, known.as_deref()).await
    }
}

async fn observe(
    git: &github::GitHub,
    authority: &HostedStagingAuthority,
    plan: &StagingGitOpsPlan,
    known: Option<&str>,
) -> Result<Value> {
    let mut last = None;
    for attempt in 0..12 {
        if now() >= authority.expires_at_ms {
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
    Err(last.unwrap_or_else(|| anyhow::anyhow!("staging_observation_window_expired")))
}

pub(super) async fn execute() -> Result<()> {
    let api = super::required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let deployment = super::required_env("PHARNESS_DEPLOYMENT_INTENT_ID")?;
    let execution = super::required_env("PHARNESS_EXECUTION_ID")?;
    let token = super::required_env("PHARNESS_WORKER_TOKEN")?;
    let observe_only = std::env::var("PHARNESS_STAGING_OBSERVE_ONLY").as_deref() == Ok("true");
    let git = github::GitHub::new(super::required_env(if observe_only {
        "PHARNESS_GIT_OBSERVER_TOKEN"
    } else {
        "PHARNESS_GIT_WRITER_TOKEN"
    })?)?;
    let transport = Transport {
        client: git.client.clone(),
        url: format!("{api}/api/internal/deployment-intents/{deployment}/staging"),
        token,
        execution,
        deployment,
        observe_only,
    };
    let initial = transport.context().await?;
    let mut outcome = match transport.run(&git, &initial).await {
        Ok(mut result) => {
            result
                .as_object_mut()
                .context("staging_observation_invalid")?
                .remove("status");
            json!({"status":"committed","observation":result})
        }
        Err(error) => {
            let message = error.to_string();
            let code = if !message.is_empty()
                && message.len() <= 100
                && message.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            {
                message.as_str()
            } else {
                "staging_observation_unconfirmed"
            };
            json!({"status":"unconfirmed","error_code":code})
        }
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

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
