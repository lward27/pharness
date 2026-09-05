use super::{
    fetch_internal_context_with_retry, github_observer_json, observe_github_required_checks,
    parse_github_repository, post_git_delivery_outcome, required_env,
    GitDeliveryObservationContext,
};
use pharness_core::hosted_sdlc::HostedSourceMergeAuthority;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeContext {
    authority: HostedSourceMergeAuthority,
    authority_hash: String,
    github_api_url: String,
}

pub(super) async fn execute() -> anyhow::Result<()> {
    let api = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_owned();
    let intent = required_env("PHARNESS_SOURCE_DELIVERY_INTENT_ID")?;
    let execution = required_env("PHARNESS_GIT_DELIVERY_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_WRITER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let context_url = format!(
        "{api}/api/internal/source-delivery-intents/{intent}/merge-context?execution_id={execution}"
    );
    let context: MergeContext =
        fetch_internal_context_with_retry(&client, &context_url, &worker_token).await?;
    validate_context(&context, &intent, &execution)?;
    let result =
        merge_exact_source(&client, &context, &git_token, &context_url, &worker_token).await;
    let mut outcome = match result {
        Ok(receipt) => receipt,
        Err(error) => {
            // Errors before the PUT cannot have performed a source merge. All
            // uncertain results after the PUT are returned as unknown below.
            let code = error.to_string();
            let code =
                if code.len() <= 100 && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                    code.as_str()
                } else {
                    "source_merge_precondition_unavailable"
                };
            json!({"status":"failed","error_code":code})
        }
    };
    outcome["execution_id"] = json!(execution);
    outcome["authority_hash"] = json!(context.authority_hash);
    outcome["checked_at_ms"] = json!(now_ms());
    post_git_delivery_outcome(
        &client,
        &format!("{api}/api/internal/source-delivery-intents/{intent}/merge-outcome"),
        &worker_token,
        &outcome,
    )
    .await
}

fn validate_context(context: &MergeContext, intent: &str, execution: &str) -> anyhow::Result<()> {
    context
        .authority
        .validate(now_ms())
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        context.github_api_url == "https://api.github.com"
            && context.authority.source_delivery_intent_id == intent
            && context.authority.execution_id == execution
            && context
                .authority
                .material_hash()
                .map_err(anyhow::Error::msg)?
                == context.authority_hash,
        "source_merge_authority_mismatch"
    );
    Ok(())
}

async fn merge_exact_source(
    client: &reqwest::Client,
    context: &MergeContext,
    token: &str,
    context_url: &str,
    worker_token: &str,
) -> anyhow::Result<Value> {
    let a = &context.authority;
    let (owner, repo) = parse_github_repository(&a.repository)?;
    let root = format!("{}/repos/{owner}/{repo}", context.github_api_url);
    let pull_url = format!("{root}/pulls/{}", a.pull_request_number);
    let pull = query(client, &pull_url, token).await?;
    validate_pull_identity(&pull, a)?;
    if pull["merged"] == true {
        return confirmed_merge(client, &root, token, &pull, a, None).await;
    }
    validate_open_pull(&pull, a)?;
    let protection = query(client, &format!("{root}/branches/main/protection"), token).await?;
    validate_protection(&protection, a)?;
    let branch = query(client, &format!("{root}/branches/main"), token).await?;
    anyhow::ensure!(
        branch["protected"] == true && branch["commit"]["sha"] == a.base_commit_sha,
        "source_merge_base_changed"
    );
    let observation = GitDeliveryObservationContext {
        expected_base_commit_sha: Some(a.base_commit_sha.clone()),
        execution_id: a.execution_id.clone(),
        repository: a.repository.clone(),
        base_ref: Some(a.base_ref.clone()),
        head_branch: a.head_branch.clone(),
        source_commit_sha: a.head_commit_sha.clone(),
        pull_request_url: a.pull_request_url.clone(),
        pull_request_number: a.pull_request_number,
        github_api_url: context.github_api_url.clone(),
    };
    let checks = observe_github_required_checks(client, &observation, token, None).await?;
    validate_source_integrity(&checks, a)?;

    // Revalidate the recorded authority after provider reads and immediately
    // before the sole write. A pause, cancellation, changed source or expired
    // operation must make the API withhold this context.
    let fresh: MergeContext =
        fetch_internal_context_with_retry(client, context_url, worker_token).await?;
    validate_context(&fresh, &a.source_delivery_intent_id, &a.execution_id)?;
    anyhow::ensure!(
        fresh.authority == *a && fresh.authority_hash == context.authority_hash,
        "source_merge_authority_changed"
    );
    let latest = query(client, &pull_url, token).await?;
    validate_pull_identity(&latest, a)?;
    if latest["merged"] == true {
        return confirmed_merge(client, &root, token, &latest, a, None).await;
    }
    validate_open_pull(&latest, a)?;
    a.validate(now_ms()).map_err(anyhow::Error::msg)?;

    // Admission is durable before the provider write. A lost admission response
    // spends this identity without sending a PUT; recovery only observes. Do
    // not retry admission or reinterpret an existing record as new permission.
    let admission_url = context_url
        .split("/merge-context")
        .next()
        .ok_or_else(|| anyhow::anyhow!("source_merge_context_invalid"))?;
    let admission = client
        .post(format!("{admission_url}/merge-attempt"))
        .bearer_auth(worker_token)
        .json(&json!({"execution_id":a.execution_id,"authority_hash":context.authority_hash}))
        .send()
        .await;
    let admitted = match admission {
        Ok(response) if response.status().is_success() => {
            response.json::<Value>().await.is_ok_and(|value| {
                value["admitted"] == true && value["authority_hash"] == context.authority_hash
            })
        }
        _ => false,
    };
    if !admitted {
        return Ok(json!({"status":"unknown","error_code":"source_merge_admission_not_confirmed"}));
    }
    if a.validate(now_ms()).is_err() {
        return Ok(
            json!({"status":"failed","error_code":"source_merge_authority_expired_before_write"}),
        );
    }

    // GitHub's sha argument compares the PR head. The strict, admin-enforced
    // required check supplies the provider's up-to-date-base enforcement.
    // Never retry this PUT after an uncertain response.
    let response = client
        .put(format!("{pull_url}/merge"))
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .header("user-agent", "pharness-source-merge")
        .json(&json!({"sha":a.head_commit_sha,"merge_method":"merge"}))
        .send()
        .await;
    let status = response.as_ref().ok().map(|r| r.status().as_u16());
    let observed = query(client, &pull_url, token).await;
    let mut receipt = match observed {
        Ok(pull) if validate_pull_identity(&pull, a).is_ok() && pull["merged"] == true => {
            match confirmed_merge(client, &root, token, &pull, a, status).await {
                Ok(value) => value,
                Err(_) => {
                    json!({"status":"unknown","error_code":"source_merge_provenance_unconfirmed"})
                }
            }
        }
        _ if matches!(status, Some(405 | 409 | 422)) => {
            json!({"status":"failed","error_code":"source_merge_rejected"})
        }
        _ => json!({"status":"unknown","error_code":"source_merge_acknowledgement_unknown"}),
    };
    receipt["merge_http_status"] = json!(status);
    receipt["required_checks"] = checks.required_checks;
    Ok(receipt)
}

async fn confirmed_merge(
    client: &reqwest::Client,
    root: &str,
    token: &str,
    pull: &Value,
    authority: &HostedSourceMergeAuthority,
    http_status: Option<u16>,
) -> anyhow::Result<Value> {
    let sha = pull["merge_commit_sha"]
        .as_str()
        .filter(|sha| super::is_git_sha(sha))
        .ok_or_else(|| anyhow::anyhow!("source_merge_commit_missing"))?;
    let commit = query(client, &format!("{root}/git/commits/{sha}"), token).await?;
    validate_merge_commit(&commit, sha, authority)?;
    Ok(json!({
        "status":"merged",
        "merge_commit_sha":sha,
        "base_commit_sha":authority.base_commit_sha,
        "head_commit_sha":authority.head_commit_sha,
        "merge_tree_sha":commit["tree"]["sha"],
        "merge_http_status":http_status,
        "origin":if http_status == Some(200) {"api_acknowledged"} else {"observed_existing_merge"},
    }))
}

async fn query(client: &reqwest::Client, url: &str, token: &str) -> anyhow::Result<Value> {
    github_observer_json(
        client,
        url,
        Some(token),
        false,
        "source_merge_provider_read_unavailable",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("source_merge_provider_read_unavailable"))
}

fn validate_pull_identity(pull: &Value, a: &HostedSourceMergeAuthority) -> anyhow::Result<()> {
    let repository = a
        .repository
        .trim_start_matches("https://github.com/")
        .trim_end_matches(".git");
    anyhow::ensure!(
        pull["number"].as_u64() == Some(a.pull_request_number)
            && pull["html_url"] == a.pull_request_url
            && pull["base"]["ref"] == a.base_ref
            && pull["base"]["repo"]["full_name"] == repository
            && pull["head"]["repo"]["full_name"] == repository
            && pull["head"]["ref"] == a.head_branch
            && pull["head"]["sha"] == a.head_commit_sha,
        "source_merge_pull_request_changed"
    );
    Ok(())
}

fn validate_open_pull(pull: &Value, a: &HostedSourceMergeAuthority) -> anyhow::Result<()> {
    anyhow::ensure!(
        pull["state"] == "open" && pull["merged"] == false && pull["draft"] == false,
        "source_merge_pull_request_not_open"
    );
    anyhow::ensure!(
        pull["base"]["sha"] == a.base_commit_sha,
        "source_merge_base_changed"
    );
    anyhow::ensure!(
        pull["mergeable"] == true && pull["mergeable_state"] == "clean",
        "source_merge_pull_request_not_ready"
    );
    Ok(())
}

fn validate_protection(value: &Value, a: &HostedSourceMergeAuthority) -> anyhow::Result<()> {
    let check_bound = value["required_status_checks"]["checks"]
        .as_array()
        .is_some_and(|checks| {
            checks.iter().any(|check| {
                check["context"] == a.required_check_context
                    && check["app_id"].as_u64() == Some(a.required_check_app_id)
            })
        });
    anyhow::ensure!(
        value["enforce_admins"]["enabled"] == true
            && value["required_status_checks"]["strict"] == true
            && value["allow_force_pushes"]["enabled"] == false
            && value["allow_deletions"]["enabled"] == false
            && value["required_pull_request_reviews"].is_object()
            && check_bound,
        "source_merge_required_protection_missing"
    );
    Ok(())
}

fn validate_source_integrity(
    checks: &super::GitHubRequiredCheckObservation,
    a: &HostedSourceMergeAuthority,
) -> anyhow::Result<()> {
    let bound = checks
        .required_checks
        .as_array()
        .is_some_and(|requirements| {
            requirements.iter().any(|r| {
                r["name"] == a.required_check_context
                    && r["app_id"].as_u64() == Some(a.required_check_app_id)
            })
        });
    let runs = checks
        .check_runs
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("source_merge_required_checks_unavailable"))?
        .iter()
        .filter(|r| {
            r["name"] == a.required_check_context
                && r["app_id"].as_u64() == Some(a.required_check_app_id)
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        checks.status == "passing"
            && bound
            && !runs.is_empty()
            && runs
                .iter()
                .all(|r| r["status"] == "completed" && r["conclusion"] == "success"),
        "source_merge_required_checks_not_passed"
    );
    Ok(())
}

fn validate_merge_commit(
    commit: &Value,
    sha: &str,
    a: &HostedSourceMergeAuthority,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        commit["sha"] == sha
            && commit["parents"].as_array().is_some_and(|p| p.len() == 2
                && p[0]["sha"] == a.base_commit_sha
                && p[1]["sha"] == a.head_commit_sha)
            && commit["tree"]["sha"]
                .as_str()
                .is_some_and(super::is_git_sha),
        "source_merge_parent_or_tree_mismatch"
    );
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharness_core::hosted_sdlc::HOSTED_SOURCE_MERGE_SCHEMA;

    fn authority() -> HostedSourceMergeAuthority {
        serde_json::from_value(json!({
            "schema_version":HOSTED_SOURCE_MERGE_SCHEMA,
            "operation_id":"workflowop_recorded","execution_id":"srcmerge_recorded",
            "work_item_id":"witem_recorded","source_delivery_intent_id":"srcintent_recorded",
            "workflow_policy_hash":format!("sha256:{}","d".repeat(64)),
            "change_set_material_hash":format!("sha256:{}","c".repeat(64)),
            "repository":"https://github.com/lward27/yfinance_wrapper.git","base_ref":"main",
            "base_commit_sha":"a".repeat(40),"head_commit_sha":"b".repeat(40),
            "head_branch":"pharness/witem_recorded/cccccccccccc",
            "pull_request_number":7,"pull_request_url":"https://github.com/lward27/yfinance_wrapper/pull/7",
            "required_check_context":"Source integrity","required_check_app_id":15368,
            "expires_at_ms":now_ms()+60_000,
        })).unwrap()
    }

    fn pull(a: &HostedSourceMergeAuthority) -> Value {
        json!({
            "number":a.pull_request_number,"html_url":a.pull_request_url,
            "base":{"ref":"main","sha":a.base_commit_sha,"repo":{"full_name":"lward27/yfinance_wrapper"}},
            "head":{"ref":a.head_branch,"sha":a.head_commit_sha,"repo":{"full_name":"lward27/yfinance_wrapper"}},
            "state":"open","merged":false,"draft":false,"mergeable":true,"mergeable_state":"clean",
        })
    }

    #[test]
    fn source_merge_refuses_stale_heads_bases_forks_and_unready_pull_requests() {
        let a = authority();
        validate_pull_identity(&pull(&a), &a).unwrap();
        validate_open_pull(&pull(&a), &a).unwrap();
        for (pointer, replacement) in [
            ("/head/sha", json!("e".repeat(40))),
            ("/base/sha", json!("e".repeat(40))),
            ("/base/ref", json!("production")),
            ("/head/repo/full_name", json!("another/yfinance_wrapper")),
            ("/draft", json!(true)),
            ("/merged", json!(true)),
            ("/state", json!("closed")),
            ("/mergeable", Value::Null),
            ("/mergeable_state", json!("behind")),
            ("/number", json!(8)),
        ] {
            let mut changed = pull(&a);
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_pull_identity(&changed, &a)
                    .and_then(|_| validate_open_pull(&changed, &a))
                    .is_err(),
                "{pointer}"
            );
        }
    }

    #[test]
    fn source_merge_requires_strict_admin_enforced_app_bound_protection() {
        let a = authority();
        let protection = json!({
            "enforce_admins":{"enabled":true},
            "required_status_checks":{"strict":true,"checks":[{"context":"Source integrity","app_id":15368}]},
            "allow_force_pushes":{"enabled":false},"allow_deletions":{"enabled":false},
            "required_pull_request_reviews":{"required_approving_review_count":0},
        });
        validate_protection(&protection, &a).unwrap();
        for (pointer, replacement) in [
            ("/enforce_admins/enabled", json!(false)),
            ("/required_status_checks/strict", json!(false)),
            ("/required_status_checks/checks", json!([])),
            ("/required_status_checks/checks/0/app_id", json!(-1)),
            ("/allow_force_pushes/enabled", json!(true)),
            ("/allow_deletions/enabled", json!(true)),
            ("/required_pull_request_reviews", Value::Null),
        ] {
            let mut changed = protection.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(validate_protection(&changed, &a).is_err(), "{pointer}");
        }
    }

    #[test]
    fn source_merge_needs_successful_execution_of_the_actual_required_source_check() {
        let a = authority();
        let mut checks = super::super::GitHubRequiredCheckObservation {
            required_checks: json!([{"name":"Source integrity","app_id":15368,"status":"passing"}]),
            check_runs: json!([{"name":"Source integrity","app_id":15368,"status":"completed","conclusion":"success"}]),
            commit_statuses: json!([]),
            status: "passing".into(),
        };
        validate_source_integrity(&checks, &a).unwrap();
        for conclusion in ["skipped", "neutral", "failure", "cancelled"] {
            checks.check_runs[0]["conclusion"] = json!(conclusion);
            assert!(
                validate_source_integrity(&checks, &a).is_err(),
                "{conclusion}"
            );
        }
        checks.check_runs[0]["conclusion"] = json!("success");
        checks.status = "pending".into();
        assert!(validate_source_integrity(&checks, &a).is_err());
        checks.status = "passing".into();
        checks.check_runs[0]["app_id"] = json!(1);
        assert!(validate_source_integrity(&checks, &a).is_err());
    }

    #[test]
    fn source_merge_receipt_requires_the_exact_merged_parent_pair_and_real_tree() {
        let a = authority();
        let sha = "e".repeat(40);
        let commit = json!({"sha":sha,"parents":[{"sha":a.base_commit_sha},{"sha":a.head_commit_sha}],"tree":{"sha":"f".repeat(40)}});
        validate_merge_commit(&commit, &sha, &a).unwrap();
        for (pointer, replacement) in [
            ("/sha", json!("f".repeat(40))),
            ("/parents", json!([{"sha":a.head_commit_sha}])),
            ("/parents/0/sha", json!("f".repeat(40))),
            ("/parents/1/sha", json!("f".repeat(40))),
            ("/tree/sha", Value::Null),
        ] {
            let mut changed = commit.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_merge_commit(&changed, &sha, &a).is_err(),
                "{pointer}"
            );
        }
    }

    #[test]
    fn source_merge_context_hash_and_execution_binding_are_checked_before_provider_access() {
        let a = authority();
        let mut context = MergeContext {
            authority_hash: a.material_hash().unwrap(),
            authority: a,
            github_api_url: "https://api.github.com".into(),
        };
        validate_context(&context, "srcintent_recorded", "srcmerge_recorded").unwrap();
        assert!(validate_context(&context, "srcintent_recorded", "another_execution").is_err());
        context.github_api_url = "https://example.com".into();
        assert!(validate_context(&context, "srcintent_recorded", "srcmerge_recorded").is_err());
        context.github_api_url = "https://api.github.com".into();
        context.authority.pull_request_number = 8;
        context.authority.pull_request_url =
            "https://github.com/lward27/yfinance_wrapper/pull/8".into();
        assert!(validate_context(&context, "srcintent_recorded", "srcmerge_recorded").is_err());
    }
}
