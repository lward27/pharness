use super::{FinanceDeploymentExpectation, FinanceRuntimeWindow};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pod {
    pub name: String,
    pub uid: String,
    pub ip: String,
    pub restarts: u64,
}

pub(super) struct Context {
    pub pods: BTreeMap<String, Pod>,
    pub namespace: &'static str,
    pub workload: &'static str,
}

impl Context {
    pub fn validate(
        expected: &FinanceDeploymentExpectation,
        window: &FinanceRuntimeWindow,
        before: &Value,
        after: &Value,
        now_ms: u64,
    ) -> Result<Self, &'static str> {
        expected
            .validate()
            .map_err(|_| "invalid_expected_identity")?;
        window
            .end_unix_seconds
            .checked_mul(1_000_000_000)
            .ok_or("invalid_window")?;
        let start = window
            .start_unix_seconds
            .checked_mul(1000)
            .ok_or("invalid_window")?;
        let end = window
            .end_unix_seconds
            .checked_mul(1000)
            .ok_or("invalid_window")?;
        if start == 0
            || !matches!(end.checked_sub(start), Some(300_000 | 600_000))
            || now_ms < end
            || now_ms - end > 60_000
        {
            return Err("window_must_be_elapsed_five_or_ten_minutes_and_fresh");
        }
        if window.start_unix_seconds % 30 != 0 {
            return Err("window_must_start_on_query_grid_without_rounding");
        }
        for observation in [before, after] {
            if observation["schema_version"]
                != "pharness.dev/finance-deployment-observation/v1alpha1"
                || observation["expected"] != json!(expected)
                || observation["cluster"] != "lucas_engineering"
                || observation["identity_state"] != "verified"
                || observation["identity"]["image_ref"] != expected.image_ref()
                || observation["identity"]["gitops_revision"] != expected.gitops_commit_sha
            {
                return Err("window_requires_matching_native_identity_observations");
            }
            let began = observation["started_at_unix_ms"]
                .as_u64()
                .ok_or("identity_time_missing")?;
            let completed = observation["completed_at_unix_ms"]
                .as_u64()
                .ok_or("identity_time_missing")?;
            if began > completed || completed > now_ms || completed - began > 60_000 {
                return Err("identity_time_invalid");
            }
        }
        let before_end = before["completed_at_unix_ms"].as_u64().unwrap();
        let after_start = after["started_at_unix_ms"].as_u64().unwrap();
        if before_end > start || start - before_end > 60_000 || after_start < end {
            return Err("identity_observations_do_not_bracket_window");
        }
        for path in [
            "/deployment/uid",
            "/generation",
            "/template_hash",
            "/replicas",
            "/service/uid",
        ] {
            let old = before["identity"]
                .pointer(path)
                .ok_or("identity_binding_missing")?;
            let new = after["identity"]
                .pointer(path)
                .ok_or("identity_binding_missing")?;
            if old.is_null() || old != new {
                return Err("deployment_identity_changed_during_window");
            }
        }
        let pods = Self::pods(before, expected)?;
        if pods != Self::pods(after, expected)? {
            return Err("pod_identity_or_restart_count_changed_during_window");
        }
        Ok(Self {
            pods,
            namespace: expected.namespace(),
            workload: expected.workload(),
        })
    }

    fn pods(
        observation: &Value,
        expected: &FinanceDeploymentExpectation,
    ) -> Result<BTreeMap<String, Pod>, &'static str> {
        let rows = observation["identity"]["pods"]
            .as_array()
            .filter(|rows| !rows.is_empty() && rows.len() <= 32)
            .ok_or("invalid_pod_list")?;
        if observation["identity"]["replicas"].as_u64() != Some(rows.len() as u64) {
            return Err("pod_count_does_not_match_identity");
        }
        let mut pods = BTreeMap::new();
        let mut uids = std::collections::BTreeSet::new();
        for row in rows {
            if row["image_id"]
                .as_str()
                .map(|s| s.strip_prefix("docker-pullable://").unwrap_or(s))
                != Some(expected.image_ref().as_str())
            {
                return Err("pod_digest_does_not_match_expected");
            }
            let name = row["name"]
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 253
                        && s.bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                })
                .ok_or("invalid_pod_name")?;
            let uid = row["uid"]
                .as_str()
                .filter(|s| {
                    s.len() == 36
                        && s.bytes().enumerate().all(|(i, c)| {
                            if [8, 13, 18, 23].contains(&i) {
                                c == b'-'
                            } else {
                                c.is_ascii_hexdigit()
                            }
                        })
                })
                .ok_or("invalid_pod_uid")?;
            let ip = row["ip"]
                .as_str()
                .ok_or("invalid_pod_ip")?
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| "invalid_pod_ip")?
                .to_string();
            let restarts = row["restart_count"]
                .as_u64()
                .ok_or("restart_count_missing")?;
            if !uids.insert(uid.to_string())
                || pods
                    .insert(
                        name.to_string(),
                        Pod {
                            name: name.into(),
                            uid: uid.into(),
                            ip,
                            restarts,
                        },
                    )
                    .is_some()
            {
                return Err("duplicate_pod_identity");
            }
        }
        Ok(pods)
    }

    pub fn log_pod<'a>(&'a self, metric: &Value) -> Result<&'a Pod, &'static str> {
        let pod = metric["pod"]
            .as_str()
            .and_then(|name| self.pods.get(name))
            .ok_or("unrelated_log_pod")?;
        let prefix = format!(
            "/var/log/pods/{}_{}_{}/{}/",
            self.namespace, pod.name, pod.uid, self.workload
        );
        let file = metric["filename"]
            .as_str()
            .and_then(|s| s.strip_prefix(&prefix))
            .and_then(|s| s.strip_suffix(".log"))
            .ok_or("unrelated_log_pod_uid")?;
        if file.is_empty() || !file.bytes().all(|c| c.is_ascii_digit()) {
            return Err("invalid_log_filename");
        }
        Ok(pod)
    }
}
