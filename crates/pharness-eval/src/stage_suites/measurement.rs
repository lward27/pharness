//! Finite, evaluator-owned workspaces. Expected answers never enter agent inputs.
use super::{git, git_lines, StageFixture, SuiteKind};
use anyhow::{bail, Context, Result};
use pharness_core::canonical_json_sha256;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(test)]
mod tests;

pub(super) fn fixtures(suite: SuiteKind) -> Result<Vec<StageFixture>> {
    let data = match suite {
        SuiteKind::PlannerV2 => include_str!("fixtures/planner-v2.json"),
        SuiteKind::VerifierV2 => include_str!("fixtures/verifier-v2.json"),
        _ => bail!("stage does not use this measurement manifest"),
    };
    let rows: Vec<Value> = serde_json::from_str(data)?;
    rows.into_iter()
        .map(|row| {
            let spec = &row["spec"];
            let acceptance = spec.get("acceptance").cloned().unwrap_or_else(|| {
                json!(spec["commands"].as_array().unwrap().iter().map(|c| c["name"].clone()).collect::<Vec<_>>())
            });
            let writable = spec.get("writable_paths").cloned()
                .unwrap_or_else(|| json!(["src/**", "tests/**", "README.md"]));
            Ok(StageFixture {
                id: row["id"].as_str().context("case ID")?.into(),
                task: if suite == SuiteKind::PlannerV2 {
                    format!("Plan the following bounded change using the pinned repository and its actual baseline receipts. Use concrete paths and declared acceptance_names in structured steps. Record unresolved choices and baseline risks. Do not execute changes. {}", spec["requirements"].as_str().unwrap())
                } else {
                    format!("Independently verify this proposed change against its requirements, source diff and recorded test receipts. The working tree contains the proposed diff; do not modify it. Test success alone is not proof of correctness. Submit an evidence-backed verdict and explain any blocking contradiction. {}", spec["requirements"].as_str().unwrap())
                },
                context: json!({"acceptance":acceptance,"writable_paths":writable}),
                evidence: json!({}),
                expected: json!({"measurement":spec,"acceptance":acceptance,"decision":spec["decision"],"required_paths":spec["required_paths"]}),
            })
        })
        .collect()
}

pub(super) fn visible_id(suite: SuiteKind, fixture: &StageFixture) -> String {
    if matches!(
        suite,
        SuiteKind::PlannerV2 | SuiteKind::VerifierV2 | SuiteKind::TestDiagnosisV2
    ) {
        if let Some(id) = fixture
            .expected
            .pointer("/measurement/public_id")
            .and_then(Value::as_str)
        {
            return id.into();
        }
        let hash = canonical_json_sha256(&json!(["measurement-2026-09-06", fixture.id]))
            .expect("case hash");
        return format!("case-{}", &hash[7..23]);
    }
    fixture.id.clone()
}

pub(super) fn prepare(
    root: &Path,
    fixture: &StageFixture,
    suite: SuiteKind,
) -> Result<StageFixture> {
    let mut prepared = fixture.clone();
    let spec = &fixture.expected["measurement"];
    write_files(root, &spec["baseline_files"])?;
    fs::write(
        root.join(".gitignore"),
        "__pycache__/\n*.pyc\n.pharness-runtime/\n",
    )?;
    git(root, &["init", "-q"])?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "-c",
            "user.email=eval@example.invalid",
            "-c",
            "user.name=PHarness Eval",
            "commit",
            "-qm",
            "source baseline",
        ],
    )?;
    let baseline_sha = git_lines(root, &["rev-parse", "HEAD"])?.remove(0);
    let contract_value = contract(spec, &fixture.context["writable_paths"])?;
    let native_contract: pharness_core::RepositoryContract =
        serde_json::from_value(contract_value.clone())?;
    native_contract.validate(root)?;
    let baseline_hash = source_hash(root)?;
    let baseline_receipts = receipts(root, &spec["commands"], &baseline_hash)?;
    write_files(root, &spec["candidate_files"])?;
    git(root, &["add", "-N", "."])?;
    native_contract.validate(root)?;
    let candidate_hash = source_hash(root)?;
    let mode = spec["mode"].as_str().unwrap_or("normal");
    let mut test_receipts = if mode == "stale_test_receipt" {
        baseline_receipts.clone()
    } else {
        receipts(root, &spec["commands"], &candidate_hash)?
    };
    if mode == "missing_unit_receipt" {
        test_receipts.retain(|r| r["name"] != "unit");
    }
    let semantic = if mode == "semantic_receipt" {
        Some(oracle_receipt(root, spec, &candidate_hash)?)
    } else {
        None
    };
    let diff = git_lines(root, &["diff", "--no-ext-diff", "--no-color", "--", "."])?.join("\n");
    let paths = git_lines(root, &["diff", "--name-only"])?;
    let authorized_hash = if mode == "unapproved_source" {
        &baseline_hash
    } else {
        &candidate_hash
    };
    let contract = contract_value;
    prepared.context["repository_contract"] = contract.clone();
    prepared.context["source"] = json!({"base_commit":baseline_sha,"candidate_content_hash":candidate_hash,"authorized_content_hash":authorized_hash});
    prepared.context["selected_acceptance_names"] = fixture.expected["acceptance"].clone();
    prepared.context["effective_upstream_outcomes"] = json!([{"stage":if suite == SuiteKind::PlannerV2 {"discover"} else {"test"},"status":"recorded","evidence_ref":"fixture_evidence"}]);
    prepared.evidence = json!({
        "schema_version":"pharness.dev/stage-measurement/v1",
        "requirements":spec["requirements"],"repository_contract":contract,
        "source":prepared.context["source"],"diff":diff,"changed_paths":paths,
        "baseline_receipts":baseline_receipts,"test_receipts":test_receipts,
        "semantic_receipt":semantic,
        "meaning":"Recorded command outcomes and source facts; no overall correctness verdict is supplied."
    });
    if spec.pointer("/options/stale") == Some(&json!(true)) {
        prepared.evidence["retained_reference"] = json!({"content":"Use /quote for the request route","content_hash":canonical_json_sha256(&spec["baseline_files"])?});
    }
    prepared.expected["prepared_source_hash"] = json!(candidate_hash);
    prepared.expected["prepared_base_sha"] = json!(baseline_sha);
    Ok(prepared)
}

pub(super) fn contract(spec: &Value, writable: &Value) -> Result<Value> {
    use sha2::{Digest, Sha256};
    let node = spec["environment_profile"] == "node-24";
    let lock_path = if node {
        "package-lock.json"
    } else {
        "requirements.lock"
    };
    let lock = spec["candidate_files"][lock_path]
        .as_str()
        .context("real lock content")?;
    Ok(json!({
        "api_version":"pharness.dev/v1alpha1","environment_profile":spec["environment_profile"],
        "dependency_lock":{"kind":if node {"npm_package_lock"} else {"pip_requirements"},"path":lock_path,"sha256":format!("{:x}",Sha256::digest(lock.as_bytes()))},
        "writable_paths":writable,"acceptance_commands":spec["commands"].as_array().unwrap().iter().map(|c| json!({"name":c["name"],"command":c["argv"].as_array().unwrap().iter().map(|v|v.as_str().unwrap()).collect::<Vec<_>>().join(" ")})).collect::<Vec<_>>(),
        "roots":{"source":["src"],"tests":["tests"],"documentation":["README.md"]},
        "agent_network":"denied","package_installation":"denied"
    }))
}

pub(super) fn write_files(root: &Path, files: &Value) -> Result<()> {
    for (path, content) in files.as_object().context("workspace file map")? {
        if path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|p| matches!(p, "" | "." | ".." | ".git"))
        {
            bail!("invalid compiled workspace path");
        }
        let target = root.join(path);
        fs::create_dir_all(target.parent().context("file parent")?)?;
        fs::write(target, content.as_str().context("compiled file text")?)?;
    }
    Ok(())
}

/// Source hashes include contents, including dirty and untracked paths. Git status
/// alone cannot detect a second edit to an already modified candidate file.
pub(super) fn source_hash(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()?;
    if !output.status.success() {
        bail!("cannot enumerate evaluation source");
    }
    let mut contents = BTreeMap::new();
    for path in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = std::str::from_utf8(path)?;
        contents.insert(
            path,
            match fs::read(root.join(path)) {
                Ok(bytes) => json!(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
                Err(e) => return Err(e.into()),
            },
        );
    }
    Ok(canonical_json_sha256(&json!(contents))?)
}

pub(super) fn fingerprint(root: &Path) -> Result<String> {
    Ok(canonical_json_sha256(
        &json!({"contents":source_hash(root)?,"head":git_lines(root,&["rev-parse","HEAD"])?,"status":git_lines(root,&["status","--short"])?,"index":git_lines(root,&["diff","--cached","--binary"])?}),
    )?)
}

fn receipts(root: &Path, commands: &Value, source: &str) -> Result<Vec<Value>> {
    commands
        .as_array()
        .context("declared commands")?
        .iter()
        .map(|command| {
            let mut receipt = run_receipt(
                root,
                &command["argv"],
                command["timeout_ms"].as_u64().unwrap_or(5_000),
            )?;
            if receipt["status"] != "completed" {
                bail!(
                    "measurement prerequisite failed for {}: {}",
                    command["name"],
                    receipt
                );
            }
            receipt["name"] = command["name"].clone();
            receipt["source_content_hash"] = json!(source);
            Ok(receipt)
        })
        .collect()
}

/// Runs only compiled fixture commands. The Python standard library provides a
/// portable subprocess deadline and captures real output without a shell.
pub(super) fn run_receipt(root: &Path, argv: &Value, timeout_ms: u64) -> Result<Value> {
    const RUN: &str = r#"import json,os,subprocess,sys,time,tempfile
args=json.loads(sys.argv[1]);start=time.monotonic()
def text(v):
 if isinstance(v,bytes):v=v.decode('utf-8','replace')
 return (v or '').replace(os.getcwd(),'<workspace>').encode('utf-8')[:8192].decode('utf-8','ignore')
try:
 with tempfile.TemporaryDirectory(prefix='pharness-receipt-cache-') as cache:
  p=subprocess.run(args,capture_output=True,timeout=int(sys.argv[2])/1000,env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1','PYTHONPYCACHEPREFIX':cache})
 r={'status':'completed','exit_code':p.returncode,'stdout':text(p.stdout),'stderr':text(p.stderr)}
except subprocess.TimeoutExpired as e:r={'status':'timed_out','exit_code':None,'stdout':text(e.stdout),'stderr':text(e.stderr)}
except FileNotFoundError as e:r={'status':'spawn_failed','exit_code':None,'stdout':'','stderr':text(str(e))}
r.update({'argv':args,'timeout_ms':int(sys.argv[2]),'elapsed_ms':round((time.monotonic()-start)*1000),'executed':r['status']!='spawn_failed','output_limit_bytes_per_stream':8192,'python_cache_isolated':True})
print(json.dumps(r))
"#;
    if timeout_ms == 0 || timeout_ms > 10_000 {
        bail!("invalid fixture command deadline");
    }
    let output = Command::new("python3")
        .current_dir(root)
        .args(["-c", RUN, &argv.to_string(), &timeout_ms.to_string()])
        .output()?;
    if !output.status.success() {
        bail!(
            "fixture receipt collector failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(super) fn oracle_receipt(root: &Path, spec: &Value, source: &str) -> Result<Value> {
    let oracle = spec["oracle"]
        .as_str()
        .context("compiled behavioral oracle")?;
    let args = if spec["environment_profile"] == "node-24" {
        json!(["node", "--input-type=module", "-e", oracle])
    } else {
        json!(["python3", "-c", oracle])
    };
    let mut receipt = run_receipt(root, &args, 5_000)?;
    // Hidden oracle source is evaluator-owned; only its observed failure is evidence.
    receipt.as_object_mut().unwrap().remove("argv");
    for field in ["stdout", "stderr"] {
        if let Some(text) = receipt[field].as_str() {
            receipt[field] = json!(text.replace(oracle, "[oracle source withheld]"));
        }
    }
    receipt["oracle_content_hash"] = canonical_json_sha256(&json!(oracle))?.into();
    receipt["name"] = json!("controller_semantic_check");
    receipt["source_content_hash"] = json!(source);
    Ok(receipt)
}
