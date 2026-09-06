//! Synthetic persisted evidence for controller recovery/admission tests.
//! The receipt shape comes from the retained 23ea38f live reader exercise;
//! identities, times and correlation hashes are rebound for isolated tests.
//! These fixtures never represent live or autonomous acceptance evidence.
use super::{
    now, staging, HostedStagingAuthority, KubectlFixture, RepoDeliveryFixture, StagingGitOpsPlan,
};
use pharness_core::tools::{
    FinanceDeploymentExpectation, FinanceRuntimeEvidence, ReadOnlyClusterTools,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) fn tools(fake: &KubectlFixture) -> ReadOnlyClusterTools {
    let path = fake.dir.join("native-reader");
    let dir = serde_json::to_string(fake.dir.to_str().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            r#"#!/usr/bin/env python3
import json,sys
from pathlib import Path
root=Path({dir})
assert sys.argv[1]=='get'
with (root/'native-reads').open('a') as log:log.write(sys.argv[2]+'\n')
print(json.dumps(json.loads((root/'native-resources.json').read_text())[sys.argv[2]]))
"#
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    ReadOnlyClusterTools::default().with_kubectl_bin(path.to_string_lossy().to_string())
}

fn rebound(text: &str, expected: &FinanceDeploymentExpectation) -> String {
    text.replace(
        "04a98931af43b6ea1d189369442f6a1b76dda589",
        &expected.gitops_commit_sha,
    )
    .replace(
        "sha256:33f1a08b74c82fb5dc01ef0ebef8a1fa5e2fc0ac78be17dadd1f74bbf1e319ca",
        &expected.image_digest,
    )
}

fn shift(value: &mut Value, seconds: i64) {
    let shifted = |n: i64| {
        for scale in [1_i64, 1_000, 1_000_000_000] {
            if (1_788_653_000 * scale..1_788_654_000 * scale).contains(&n) {
                return n + seconds * scale;
            }
        }
        n
    };
    match value {
        Value::Number(n) => {
            if let Some(n) = n.as_i64() {
                *value = json!(shifted(n));
            }
        }
        Value::String(s) => {
            if let Ok(n) = s.parse::<i64>() {
                *s = shifted(n).to_string();
            }
        }
        Value::Array(rows) => {
            for row in rows {
                shift(row, seconds);
            }
        }
        Value::Object(map) => {
            for row in map.values_mut() {
                shift(row, seconds);
            }
        }
        _ => {}
    }
}

pub(super) async fn evidence(
    f: &RepoDeliveryFixture,
    fake: &KubectlFixture,
    a: &HostedStagingAuthority,
    p: &StagingGitOpsPlan,
    age_ms: i64,
) -> FinanceRuntimeEvidence {
    let expected = FinanceDeploymentExpectation {
        application: pharness_core::tools::FinanceApplication::Yfinance,
        environment: pharness_core::tools::FinanceEnvironment::Staging,
        gitops_commit_sha: p.base_commit_sha.clone(),
        image_digest: p.previous_image_digest(a).unwrap(),
    };
    std::fs::write(
        fake.dir.join("native-resources.json"),
        rebound(include_str!("fixtures/native-resources.json"), &expected),
    )
    .unwrap();
    let identity = f
        .state
        .cluster_tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    assert_eq!(identity["identity_state"], "verified", "{identity}");
    let end = ((now() - age_ms - 15_000) / 30_000) * 30;
    let mut fixture: Value = serde_json::from_str(&rebound(
        include_str!("fixtures/native-baseline.json"),
        &expected,
    ))
    .unwrap();
    shift(&mut fixture, end - 1_788_653_340);
    let mut e: FinanceRuntimeEvidence = serde_json::from_value(fixture).unwrap();
    e.identity_before = identity.clone();
    e.identity_after = identity;
    e.identity_before["started_at_unix_ms"] = json!((end - 300) * 1000 - 2000);
    e.identity_before["completed_at_unix_ms"] = json!((end - 300) * 1000 - 1000);
    e.identity_after["started_at_unix_ms"] = json!(end * 1000);
    e.identity_after["completed_at_unix_ms"] = json!(end * 1000 + 100);
    let correlation = format!(
        "{:x}",
        Sha256::digest(
            json!([
                expected,
                e.functional_probes["started_at_unix_ms"],
                "/healthz"
            ])
            .to_string()
            .as_bytes()
        )
    );
    let health = e.functional_probes["probes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["path"] == "/healthz")
        .unwrap();
    health["backend_trace_id"] = json!(&correlation[..32]);
    health["backend_parent_span_id"] = json!(&correlation[32..48]);
    for pointer in ["/trace_id", "/trace/trace_id"] {
        *e.health_trace.pointer_mut(pointer).unwrap() = json!(&correlation[..32]);
    }
    for pointer in ["/parent_span_id", "/trace/parent_span_id"] {
        *e.health_trace.pointer_mut(pointer).unwrap() = json!(&correlation[32..48]);
    }
    e.health_trace["receipt"]["path"] = json!(format!("/api/v2/traces/{}", &correlation[..32]));
    let hash = |v: &Value| pharness_core::canonical_json_sha256(v).unwrap();
    for value in [&mut e.signals, &mut e.health_trace] {
        value["identity_before_sha256"] = json!(hash(&e.identity_before));
        value["identity_after_sha256"] = json!(hash(&e.identity_after));
    }
    e.health_trace["functional_probes_sha256"] = json!(hash(&e.functional_probes));
    e
}

pub(super) async fn seed(
    f: &RepoDeliveryFixture,
    fake: &KubectlFixture,
    a: &HostedStagingAuthority,
    p: &StagingGitOpsPlan,
) {
    let e = evidence(f, fake, a, p, 0).await;
    staging::seed_baseline(&f.state, &a.deployment_intent_id, &a.execution_id, e, None).await;
}

pub(super) fn reads(fake: &KubectlFixture) -> usize {
    std::fs::read_to_string(fake.dir.join("native-reads"))
        .unwrap_or_default()
        .lines()
        .count()
}
