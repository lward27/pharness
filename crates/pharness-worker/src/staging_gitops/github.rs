//! GitHub operations confined to the existing Lucas GitOps repository.
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use pharness_core::hosted_sdlc::gitops_delivery::{GitOpsAuthority, GitOpsPlan};
use pharness_core::hosted_sdlc::staging::MAX_FILE_BYTES;
use serde_json::{json, Value};
use std::time::Duration;

const REST: &str = "https://api.github.com/repos/lward27/lucas_engineering";
const GRAPHQL: &str = "https://api.github.com/graphql";

pub(crate) struct GitHub {
    pub(crate) client: reqwest::Client,
    token: String,
    env: &'static str,
    rest: String,
    graphql: String,
}

impl GitHub {
    /// `env` prefixes every error code so recorded evidence names the
    /// environment whose read or write failed.
    pub(crate) fn new(token: String, env: &'static str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            token,
            env,
            rest: REST.into(),
            graphql: GRAPHQL.into(),
        })
    }

    fn code(&self, suffix: &str) -> anyhow::Error {
        anyhow::anyhow!("{}_{suffix}", self.env)
    }

    fn ensure(&self, condition: bool, suffix: &str) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(self.code(suffix))
        }
    }

    async fn get(&self, suffix: &str) -> Result<Value> {
        bounded_json(
            self.client
                .get(format!("{}{suffix}", self.rest))
                .bearer_auth(&self.token)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "pharness-gitops")
                .send()
                .await
                .map_err(|_| self.code("github_unavailable"))?,
            self.env,
        )
        .await
    }

    pub(crate) async fn branch(&self) -> Result<String> {
        self.branch_revision(true).await
    }

    async fn branch_revision(&self, for_write: bool) -> Result<String> {
        let branch = self.get("/branches/main").await?;
        self.ensure(
            branch["name"] == "main" && (!for_write || branch["protected"] == false),
            "gitops_branch_policy_changed",
        )?;
        self.oid(&branch["commit"]["sha"])
    }

    pub(crate) async fn file(&self, path: &str, revision: &str) -> Result<(String, String)> {
        let file = self
            .get(&format!("/contents/{path}?ref={revision}"))
            .await?;
        self.ensure(
            file["type"] == "file"
                && file["path"] == path
                && file["encoding"] == "base64"
                && file["size"]
                    .as_u64()
                    .is_some_and(|n| n <= MAX_FILE_BYTES as u64),
            "file_identity_invalid",
        )?;
        let sha = self.oid(&file["sha"])?;
        let encoded = file["content"]
            .as_str()
            .ok_or_else(|| self.code("file_content_missing"))?;
        self.ensure(encoded.len() <= MAX_FILE_BYTES * 2, "file_too_large")?;
        let content = STANDARD
            .decode(encoded.replace('\n', ""))
            .map_err(|_| self.code("file_encoding_invalid"))?;
        self.ensure(
            content.len() <= MAX_FILE_BYTES && file["size"] == content.len(),
            "file_size_mismatch",
        )?;
        Ok((
            sha,
            String::from_utf8(content).map_err(|_| self.code("file_encoding_invalid"))?,
        ))
    }

    pub(crate) async fn plan<P: GitOpsPlan>(&self, authority: &P::Authority) -> Result<P> {
        let base_commit_sha = self.branch().await?;
        let (path, _) = authority.coordinates().map_err(anyhow::Error::msg)?;
        let (original_blob_sha, original_content) = self.file(path, &base_commit_sha).await?;
        P::from_observed(
            authority,
            base_commit_sha,
            original_blob_sha,
            original_content,
        )
        .map_err(anyhow::Error::msg)
    }

    /// Called only after a newly acknowledged, single-use API admission. Any
    /// error after sending is ambiguous; the caller must observe, never resend.
    pub(crate) async fn commit_once<P: GitOpsPlan>(
        &self,
        authority: &P::Authority,
        plan: &P,
        now: i64,
    ) -> Result<String> {
        let body = plan
            .commit_request(authority, now)
            .map_err(anyhow::Error::msg)?;
        let response = self
            .client
            .post(&self.graphql)
            .bearer_auth(&self.token)
            .header("User-Agent", "pharness-gitops")
            .json(&body)
            .send()
            .await
            .map_err(|_| self.code("commit_unconfirmed"))?;
        let result = bounded_json(response, self.env)
            .await
            .map_err(|_| self.code("commit_unconfirmed"))?;
        self.ensure(
            result.get("errors").is_none()
                && result["data"]["createCommitOnBranch"]["clientMutationId"]
                    == authority.execution_id(),
            "commit_unconfirmed",
        )?;
        self.oid(&result["data"]["createCommitOnBranch"]["commit"]["oid"])
    }

    /// Read-only recovery uses the original base and operation marker. An empty
    /// or truncated history is inconclusive, not permission to commit again.
    pub(crate) async fn observe<P: GitOpsPlan>(
        &self,
        authority: &P::Authority,
        plan: &P,
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
            self.oid(&json!(known))?
        } else {
            let history = self
                .get(&format!("/commits?sha={current}&per_page=20"))
                .await?;
            let commits = history
                .as_array()
                .filter(|v| v.len() <= 20)
                .ok_or_else(|| self.code("history_unavailable"))?;
            let matches = commits
                .iter()
                .filter(|c| matches_commit(c, plan, &expected_message))
                .collect::<Vec<_>>();
            self.ensure(matches.len() == 1, "commit_not_established")?;
            self.oid(&matches[0]["sha"])?
        };
        let commit = self.get(&format!("/commits/{candidate}")).await?;
        self.ensure(
            self.oid(&commit["sha"])? == candidate
                && matches_commit(&commit, plan, &expected_message),
            "commit_identity_mismatch",
        )?;
        let files = commit["files"]
            .as_array()
            .ok_or_else(|| self.code("commit_files_unavailable"))?;
        let (path, _) = authority.coordinates().map_err(anyhow::Error::msg)?;
        self.ensure(
            files.len() == 1
                && files[0]["filename"] == path
                && files[0]["status"] == "modified"
                && files[0].get("previous_filename").is_none(),
            "commit_changed_unapproved_files",
        )?;
        let (candidate_blob, candidate_content) = self.file(path, &candidate).await?;
        self.ensure(
            candidate_content == plan.updated_content()
                && candidate_blob != plan.original_blob_sha(),
            "commit_file_mismatch",
        )?;
        if current != candidate {
            let comparison = self
                .get(&format!("/compare/{candidate}...{current}?per_page=1"))
                .await?;
            self.ensure(
                comparison["status"] == "ahead"
                    && comparison["base_commit"]["sha"] == candidate
                    && comparison["merge_base_commit"]["sha"] == candidate,
                "commit_not_on_current_branch",
            )?;
        }
        let (current_blob, current_content) = if current == candidate {
            (candidate_blob.clone(), candidate_content)
        } else {
            self.file(path, &current).await?
        };
        self.ensure(
            current_content == plan.updated_content() && current_blob == candidate_blob,
            "target_changed_after_commit",
        )?;
        // Retain only the verified operation's fields, not unrelated history,
        // provider error bodies, author emails, credentials or arbitrary URLs.
        Ok(
            json!({"status":"committed","gitops_commit_sha":candidate,"base_commit_sha":plan.base_commit_sha(),
            "current_gitops_revision":current,"file_blob_sha":candidate_blob,"path":path,
            "image_ref":authority.image_ref().map_err(anyhow::Error::msg)?,
            "authority_hash":plan.binding_hash(),"plan_hash":plan.material_hash().map_err(anyhow::Error::msg)?,
            "parent_count":1,"changed_file_count":1,"current_file_matches":true}),
        )
    }

    fn oid(&self, value: &Value) -> Result<String> {
        value
            .as_str()
            .filter(|v| {
                v.len() == 40
                    && v.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
            .map(str::to_owned)
            .ok_or_else(|| self.code("git_object_identity_invalid"))
    }
}

fn matches_commit<P: GitOpsPlan>(commit: &Value, plan: &P, message: &str) -> bool {
    commit["parents"]
        .as_array()
        .is_some_and(|p| p.len() == 1 && p[0]["sha"] == plan.base_commit_sha())
        && commit["commit"]["message"]
            .as_str()
            .is_some_and(|m| m.trim_end_matches('\n') == message)
}

pub(crate) async fn bounded_json(mut response: reqwest::Response, env: &str) -> Result<Value> {
    anyhow::ensure!(
        response.status().is_success(),
        "{env}_github_request_rejected"
    );
    const LIMIT: usize = 262_144;
    anyhow::ensure!(
        response
            .content_length()
            .map_or(true, |n| n <= LIMIT as u64),
        "{env}_github_response_too_large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("{env}_github_response_unavailable"))?
    {
        anyhow::ensure!(
            bytes.len().saturating_add(chunk.len()) <= LIMIT,
            "{env}_github_response_too_large"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("{env}_github_response_invalid"))
}

#[cfg(test)]
mod tests;
