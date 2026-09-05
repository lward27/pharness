//! GitHub operations confined to the existing Lucas GitOps repository.
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use pharness_core::hosted_sdlc::staging::{
    HostedStagingAuthority, StagingGitOpsPlan, MAX_FILE_BYTES, PLAN_SCHEMA,
};
use serde_json::{json, Value};
use std::time::Duration;

const REST: &str = "https://api.github.com/repos/lward27/lucas_engineering";
const GRAPHQL: &str = "https://api.github.com/graphql";

pub(super) struct GitHub {
    pub(super) client: reqwest::Client,
    token: String,
    rest: String,
    graphql: String,
}

impl GitHub {
    pub(super) fn new(token: String) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            token,
            rest: REST.into(),
            graphql: GRAPHQL.into(),
        })
    }

    async fn get(&self, suffix: &str) -> Result<Value> {
        bounded_json(
            self.client
                .get(format!("{}{suffix}", self.rest))
                .bearer_auth(&self.token)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "pharness-staging")
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("staging_github_unavailable"))?,
        )
        .await
    }

    pub(super) async fn branch(&self) -> Result<String> {
        self.branch_revision(true).await
    }

    async fn branch_revision(&self, for_write: bool) -> Result<String> {
        let branch = self.get("/branches/main").await?;
        anyhow::ensure!(
            branch["name"] == "main" && (!for_write || branch["protected"] == false),
            "staging_gitops_branch_policy_changed"
        );
        oid(&branch["commit"]["sha"])
    }

    async fn file(
        &self,
        authority: &HostedStagingAuthority,
        revision: &str,
    ) -> Result<(String, String)> {
        let (path, _) = authority.coordinates().map_err(anyhow::Error::msg)?;
        let file = self
            .get(&format!("/contents/{path}?ref={revision}"))
            .await?;
        anyhow::ensure!(
            file["type"] == "file"
                && file["path"] == path
                && file["encoding"] == "base64"
                && file["size"]
                    .as_u64()
                    .is_some_and(|n| n <= MAX_FILE_BYTES as u64),
            "staging_file_identity_invalid"
        );
        let sha = oid(&file["sha"])?;
        let encoded = file["content"]
            .as_str()
            .context("staging_file_content_missing")?;
        anyhow::ensure!(
            encoded.len() <= MAX_FILE_BYTES * 2,
            "staging_file_too_large"
        );
        let content = STANDARD
            .decode(encoded.replace('\n', ""))
            .map_err(|_| anyhow::anyhow!("staging_file_encoding_invalid"))?;
        anyhow::ensure!(
            content.len() <= MAX_FILE_BYTES && file["size"] == content.len(),
            "staging_file_size_mismatch"
        );
        Ok((
            sha,
            String::from_utf8(content)
                .map_err(|_| anyhow::anyhow!("staging_file_encoding_invalid"))?,
        ))
    }

    pub(super) async fn plan(
        &self,
        authority: &HostedStagingAuthority,
    ) -> Result<StagingGitOpsPlan> {
        let base_commit_sha = self.branch().await?;
        let (original_blob_sha, original_content) = self.file(authority, &base_commit_sha).await?;
        let (_, image) = authority.coordinates().map_err(anyhow::Error::msg)?;
        let updated_content = pharness_core::hosted_sdlc::gitops_patch::update_kustomization_image(
            &original_content,
            image,
            &authority.image_ref().map_err(anyhow::Error::msg)?,
        )
        .map_err(anyhow::Error::msg)?;
        let plan = StagingGitOpsPlan {
            schema_version: PLAN_SCHEMA.into(),
            authority_hash: authority.material_hash().map_err(anyhow::Error::msg)?,
            base_commit_sha,
            original_blob_sha,
            original_content,
            updated_content,
        };
        plan.validate(authority).map_err(anyhow::Error::msg)?;
        Ok(plan)
    }

    /// Called only after a newly acknowledged, single-use API admission. Any
    /// error after sending is ambiguous; the caller must observe, never resend.
    pub(super) async fn commit_once(
        &self,
        authority: &HostedStagingAuthority,
        plan: &StagingGitOpsPlan,
        now: i64,
    ) -> Result<String> {
        let body = plan
            .commit_request(authority, now)
            .map_err(anyhow::Error::msg)?;
        let response = self
            .client
            .post(&self.graphql)
            .bearer_auth(&self.token)
            .header("User-Agent", "pharness-staging")
            .json(&body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("staging_commit_unconfirmed"))?;
        let result = bounded_json(response)
            .await
            .map_err(|_| anyhow::anyhow!("staging_commit_unconfirmed"))?;
        anyhow::ensure!(
            result.get("errors").is_none()
                && result["data"]["createCommitOnBranch"]["clientMutationId"]
                    == authority.execution_id,
            "staging_commit_unconfirmed"
        );
        oid(&result["data"]["createCommitOnBranch"]["commit"]["oid"])
    }

    /// Read-only recovery uses the original base and operation marker. An empty
    /// or truncated history is inconclusive, not permission to commit again.
    pub(super) async fn observe(
        &self,
        authority: &HostedStagingAuthority,
        plan: &StagingGitOpsPlan,
        known_commit: Option<&str>,
    ) -> Result<Value> {
        plan.validate(authority).map_err(anyhow::Error::msg)?;
        let current = self.branch_revision(false).await?;
        let expected_message = format!(
            "{}\n\n{}",
            plan.commit_headline(authority),
            plan.commit_body(authority).map_err(anyhow::Error::msg)?
        );
        let candidate = if let Some(known) = known_commit {
            oid(&json!(known))?
        } else {
            let history = self
                .get(&format!("/commits?sha={current}&per_page=20"))
                .await?;
            let commits = history
                .as_array()
                .filter(|v| v.len() <= 20)
                .context("staging_history_unavailable")?;
            let matches = commits
                .iter()
                .filter(|c| matches_commit(c, plan, &expected_message))
                .collect::<Vec<_>>();
            anyhow::ensure!(matches.len() == 1, "staging_commit_not_established");
            oid(&matches[0]["sha"])?
        };
        let commit = self.get(&format!("/commits/{candidate}")).await?;
        anyhow::ensure!(
            oid(&commit["sha"])? == candidate && matches_commit(&commit, plan, &expected_message),
            "staging_commit_identity_mismatch"
        );
        let files = commit["files"]
            .as_array()
            .context("staging_commit_files_unavailable")?;
        let (path, _) = authority.coordinates().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            files.len() == 1
                && files[0]["filename"] == path
                && files[0]["status"] == "modified"
                && files[0].get("previous_filename").is_none(),
            "staging_commit_changed_unapproved_files"
        );
        let (candidate_blob, candidate_content) = self.file(authority, &candidate).await?;
        anyhow::ensure!(
            candidate_content == plan.updated_content && candidate_blob != plan.original_blob_sha,
            "staging_commit_file_mismatch"
        );
        if current != candidate {
            let comparison = self
                .get(&format!("/compare/{candidate}...{current}?per_page=1"))
                .await?;
            anyhow::ensure!(
                comparison["status"] == "ahead"
                    && comparison["base_commit"]["sha"] == candidate
                    && comparison["merge_base_commit"]["sha"] == candidate,
                "staging_commit_not_on_current_branch"
            );
        }
        let (current_blob, current_content) = if current == candidate {
            (candidate_blob.clone(), candidate_content)
        } else {
            self.file(authority, &current).await?
        };
        anyhow::ensure!(
            current_content == plan.updated_content && current_blob == candidate_blob,
            "staging_target_changed_after_commit"
        );
        // Retain only the verified operation's fields, not unrelated history,
        // provider error bodies, author emails, credentials or arbitrary URLs.
        Ok(
            json!({"status":"committed","gitops_commit_sha":candidate,"base_commit_sha":plan.base_commit_sha,
            "current_gitops_revision":current,"file_blob_sha":candidate_blob,"path":path,
            "image_ref":authority.image_ref().map_err(anyhow::Error::msg)?,
            "authority_hash":plan.authority_hash,"plan_hash":plan.material_hash().map_err(anyhow::Error::msg)?,
            "parent_count":1,"changed_file_count":1,"current_file_matches":true}),
        )
    }
}

fn matches_commit(commit: &Value, plan: &StagingGitOpsPlan, message: &str) -> bool {
    commit["parents"]
        .as_array()
        .is_some_and(|p| p.len() == 1 && p[0]["sha"] == plan.base_commit_sha)
        && commit["commit"]["message"]
            .as_str()
            .is_some_and(|m| m.trim_end_matches('\n') == message)
}

fn oid(value: &Value) -> Result<String> {
    value
        .as_str()
        .filter(|v| {
            v.len() == 40
                && v.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        })
        .map(str::to_owned)
        .context("staging_git_object_identity_invalid")
}

pub(super) async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    anyhow::ensure!(
        response.status().is_success(),
        "staging_github_request_rejected"
    );
    const LIMIT: usize = 262_144;
    anyhow::ensure!(
        response
            .content_length()
            .map_or(true, |n| n <= LIMIT as u64),
        "staging_github_response_too_large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("staging_github_response_unavailable"))?
    {
        anyhow::ensure!(
            bytes.len().saturating_add(chunk.len()) <= LIMIT,
            "staging_github_response_too_large"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("staging_github_response_invalid"))
}

#[cfg(test)]
mod tests;
