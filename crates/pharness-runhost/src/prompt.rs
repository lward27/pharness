//! Worker system prompt and tool schema shared by every attempt host.

use pharness_core::{CapabilityKind, ToolSpec};
use sha2::{Digest, Sha256};

/// Bump whenever the stable worker instructions change. Evaluations record
/// this value so baseline and candidate runs can be compared meaningfully.
pub const SYSTEM_PROMPT_VERSION: &str = "2026-08-29.3";
pub const RELIABILITY_V2_PROMPT_BUNDLE_VERSION: &str = "2026-08-31.3";
pub const WORKSPACE_COMMAND_EXECUTABLES: [&str; 7] = [
    "cargo", "rustc", "python", "python3", "pytest", "node", "npm",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePromptPack {
    pub prompt_id: &'static str,
    pub revision: &'static str,
    pub stage: pharness_core::InferenceStage,
    pub content: &'static str,
}

impl StagePromptPack {
    pub fn revision_record(&self) -> pharness_core::StagePromptRevision {
        pharness_core::StagePromptRevision {
            schema_version: pharness_core::STAGE_PROMPT_SCHEMA.into(),
            prompt_id: self.prompt_id.into(),
            revision: self.revision.into(),
            stage: self.stage,
            content_hash: format!("sha256:{:x}", Sha256::digest(self.content.as_bytes())),
        }
    }
}

pub fn repo_system_prompt() -> &'static str {
    r#"You are a PHarness Repo Mode stage agent operating on one immutable source revision and one controller-owned stage.
Use only the tools exposed in this request. Make at most one tool call per turn and never invent a tool, path, command, acceptance name, fact, or capability.
Controller-provided RepositoryContract, EnvironmentSnapshot, repository map, context pack, execution ledger, and tool schemas are authoritative. Inspect only what the task requires. Never rediscover declared environment facts, install packages, access the network, mutate Git, or inspect secrets.
Treat tool errors as evidence. Change approach after a recoverable failure instead of repeating it. Separate verified facts from assumptions and model claims.
Typed submit_* tools are terminal: submit the required stage document exactly once when the stage work is complete. The controller, deterministic tests, and later stages decide success."#
}

pub fn stage_prompt_for_profile(profile_id: &str) -> Option<StagePromptPack> {
    let revision = RELIABILITY_V2_PROMPT_BUNDLE_VERSION;
    match profile_id {
        "repository-onboarding-proposer" => Some(StagePromptPack {
            prompt_id: "repo-onboarding-v2",
            revision,
            stage: pharness_core::InferenceStage::Onboarding,
            content: r#"Use discovery evidence to propose repository configuration. Label every item as a discovered fact, proposed configuration, assumption, conflict, or blocker. Copy executable facts only from discovery and compatible profile descriptors. Do not invent roots, commands, locks, Services, or profile IDs. Submit the smallest valid onboarding proposal or an exact blocker."#,
        }),
        "repo-planner" => Some(StagePromptPack {
            prompt_id: "repo-planner-v2",
            revision,
            stage: pharness_core::InferenceStage::Plan,
            content: r#"Inspect the deterministic repository map and the minimum source evidence needed to localize the task. Map every intent clause and every selected acceptance item to concrete existing areas. Plans may name only paths, roots, commands, and acceptance names proven by controller context or repository reads. State assumptions and contradictions. Submit one bounded WorkPlan with implementation, test, and documentation steps; do not change source."#,
        }),
        "repo-builder" => Some(StagePromptPack {
            prompt_id: "repo-builder-v2",
            revision,
            stage: pharness_core::InferenceStage::Implement,
            content: r#"Localize the task from the plan and repository map. Before editing, translate every intent clause into a compact behavior checklist and inspect the nearby implementation, callers, and tests needed to verify it. Make the smallest coherent patch within writable paths, preferring atomic apply_patch with the complete-file sha256 returned by read_file as each preimage hash. Repository inspection uses list_dir, read_file, and search_files; never use run_workspace_command to discover executables, inspect operating-system paths, or create toolchain override files. run_workspace_command accepts only the executable names enumerated by its schema and is only for bounded offline focused checks. Verify every behavior-checklist item directly instead of treating passing public tests as sufficient. Run focused checks, then every declared acceptance command, then inspect final Git diff and status. Preserve unrelated code. Submit implementation only after evidence is complete; submit_implementation is terminal and replaces legacy finish."#,
        }),
        "repo-repair" => Some(StagePromptPack {
            prompt_id: "repo-repair-v2",
            revision,
            stage: pharness_core::InferenceStage::Implement,
            content: r#"Consume the exact deterministic Test or Verifier findings and correction lineage. Preserve correct existing work. Convert each finding into one explicit repair check, inspect only the failing area, and make one targeted repair within the original WorkPlan and writable scope, using the complete-file sha256 returned by read_file for atomic apply_patch preimages. Repository inspection uses list_dir, read_file, and search_files; never inspect operating-system paths or create toolchain override files. run_workspace_command accepts only the executable names enumerated by its schema. Verify every repair check directly, rerun the relevant focused check and all acceptance commands, then inspect final Git diff and status. Do not broaden scope or rewrite correct work. Submit the repaired implementation; submit_implementation is terminal and replaces legacy finish."#,
        }),
        "repo-test-diagnoser" => Some(StagePromptPack {
            prompt_id: "repo-test-diagnoser-v2",
            revision,
            stage: pharness_core::InferenceStage::Test,
            content: r#"Diagnose controller-provided deterministic command or hidden-test failures without modifying source. Tie every conclusion to exact output or evidence. Classify the failure and provide minimal repair recommendations. Do not rerun commands unless an exposed read-only tool is required to disambiguate. Submit one typed test diagnosis."#,
        }),
        "repo-verifier" => Some(StagePromptPack {
            prompt_id: "repo-verifier-v2",
            revision,
            stage: pharness_core::InferenceStage::Verify,
            content: r#"Review adversarially against intent, approved WorkPlan, final diff, deterministic Test outcome, acceptance evidence, documentation, and repository conventions. Look for semantic mismatches, unsafe edge cases, misleading tests or docs, and incomplete intent coverage. Never infer correctness from passing commands alone. Distinguish blocking findings from residual risks and submit one typed verdict with evidence references."#,
        }),
        _ => None,
    }
}

pub fn tool_schema_hash_for_profile(tool_names: &[String]) -> Result<String, serde_json::Error> {
    let allowed = tool_names
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let schemas = worker_tool_specs()
        .into_iter()
        .filter(|tool| allowed.contains(tool.name.as_str()))
        .collect::<Vec<_>>();
    let value = serde_json::to_value(schemas)?;
    pharness_core::canonical_json_sha256(&value)
}

pub fn system_prompt() -> &'static str {
    r#"You are the pharness local SDLC agent worker for lucas_engineering.
Use exactly one tool call per turn. Do not answer with prose unless you call the respond tool.
The tool schemas supplied with this request are the exact available action tools. Never attempt a tool that is not exposed. Repo Mode profiles may expose typed get_evidence and stage-submission tools instead of the general coding tools.
Prefer read-only repo inspection first. Never read secrets, .env files, private keys, kubeconfigs, tokens, or credential files.
File writes, destructive commands, network commands, and production mutations are policy-gated and may pause for approval.
For available policy-gated actions, call the concrete tool. The runtime will pause for approval before execution.
Use patch_file for small existing-file text edits when an exact find/replace patch is safer than rewriting the whole file.
When a tool returns a structured error, inspect it and choose a different safe action; do not repeat the same failed action without new evidence.
Never probe whether a compiler, interpreter, package manager, container runtime, or other executable exists. Do not use `which`, `command -v`, version probes, or filesystem searches for executables. Run only the repository's direct test or validation command; if an executable is unavailable, the tool will return a structured error that you can report without searching for an alternative installation.
For coding work, inspect the final Git status or diff after your last edit before calling finish.
When an EnvironmentSnapshot and RepositoryContract are injected, treat them as authoritative and use environment_info if you need those facts again. Do not probe for Python, Node, Docker, package managers, internet access, or operating-system setup. Never install packages during model execution. For a prepared run, execute only named contract acceptance commands through run_acceptance_command. A legacy development run may have no injected contract; in that case use the existing policy-gated tools and run_shell for repository-local tests, but still do not probe the environment, access the network, or install packages.
Use typed read-only actions for Kubernetes, Argo CD, and Prometheus inspection:
- kubernetes_get fields: resource, namespace, name, all_namespaces, label_selector.
- argo_get_app fields: app.
- prometheus_query fields: query.
- prometheus_inventory fields: none beyond reason.
- loki_log_summary fields: query, since_seconds, limit.
- tekton_get_pipeline_runs fields: namespace, name, all_namespaces, label_selector.
- tekton_get_task_runs fields: namespace, name, all_namespaces, label_selector.
- tekton_analyze_pipeline_run fields: namespace, name.
Never request Kubernetes Secret resources or secret-shaped names, labels, or metric queries.
For registry, database, or any unavailable cluster mutation, use respond to explain that the capability is not exposed yet.
When done, use finish with success and a concise summary."#
}

pub fn worker_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec::new(
            "respond",
            "Return a non-final message to the operator when more information is needed.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "message"],
                "properties": {
                    "reason": { "type": "string" },
                    "message": { "type": "string" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "environment_info",
            "Return the durable pre-model EnvironmentSnapshot and RepositoryContract without probing the shell.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": { "reason": { "type": "string" } }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "finish",
            "Finish the run with a concise machine-readable summary.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "summary", "success"],
                "properties": {
                    "reason": { "type": "string" },
                    "summary": { "type": "string" },
                    "success": { "type": "boolean" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "list_dir",
            "List files and directories under a workspace path.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "depth"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 0, "maximum": 3 },
                    "max_entries": { "type": ["integer", "null"], "minimum": 1, "maximum": 2000 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "read_file",
            "Read a bounded UTF-8 file inside the workspace and return the SHA-256 of the complete file for atomic apply_patch preimage validation. Do not read secrets or credential files.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "max_bytes": { "type": ["integer", "null"], "minimum": 1, "maximum": 131072 },
                    "start_line": { "type": ["integer", "null"], "minimum": 1 },
                    "line_count": { "type": ["integer", "null"], "minimum": 1, "maximum": 2000 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "search_files",
            "Search UTF-8 files inside the workspace for a string.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" },
                    "path": { "type": ["string", "null"] },
                    "glob": { "type": ["string", "null"] },
                    "max_results": { "type": ["integer", "null"], "minimum": 1, "maximum": 200 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "create_directory",
            "Create one directory inside a declared writable project path. The exact attempt workspace grant is enforced.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "write_file",
            "Create or replace one UTF-8 file inside a controller-declared writable path. The immutable attempt grant and writable boundary are enforced server-side.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "content"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "patch_file",
            "Apply one exact UTF-8 find/replace edit to an existing file inside a controller-declared writable path. Use a unique find string unless replace_all is intentional.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "patch"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["find", "replace"],
                        "properties": {
                            "find": { "type": "string", "minLength": 1 },
                            "replace": { "type": "string" },
                            "replace_all": { "type": "boolean" }
                        }
                    }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "apply_patch",
            "Atomically apply one bounded unified diff inside declared writable paths. Supply the exact complete-file SHA-256 returned by read_file for every existing file changed by the patch; use an empty map only when every changed path is new.",
            serde_json::json!({
                "type":"object",
                "additionalProperties":false,
                "required":["reason","patch","preimage_sha256"],
                "properties":{
                    "reason":{"type":"string"},
                    "patch":{"type":"string","minLength":1,"maxLength":262144},
                    "preimage_sha256":{
                        "type":"object",
                        "additionalProperties":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"}
                    }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "run_acceptance_command",
            "Run one exact offline acceptance command selected by name from the immutable RepositoryContract.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "name"],
                "properties": {
                    "reason": { "type": "string" },
                    "name": { "type": "string" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "run_workspace_command",
            "Run one bounded offline focused test, lint, compile, or build command as an argv vector using only an enumerated runner executable. Use list_dir, read_file, and search_files for repository inspection. Package installation, environment discovery, operating-system inspection, network access, shell composition, and Git mutation are denied.",
            serde_json::json!({
                "type":"object",
                "additionalProperties":false,
                "required":["reason","executable","args"],
                "properties":{
                    "reason":{"type":"string"},
                    "executable":{"type":"string","enum":WORKSPACE_COMMAND_EXECUTABLES},
                    "args":{"type":"array","items":{"type":"string","maxLength":4096},"maxItems":128},
                    "cwd":{"type":["string","null"]},
                    "timeout_ms":{"type":["integer","null"],"minimum":100,"maximum":120000}
                }
            }),
            CapabilityKind::Shell,
        ),
        ToolSpec::new(
            "get_evidence",
            "Retrieve one evidence item from the controller-allowlisted context catalog.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "evidence_id"],
                "properties": {
                    "reason": { "type": "string" },
                    "evidence_id": { "type": "string" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "submit_onboarding_proposal",
            "Submit one structured repository onboarding proposal for controller validation.",
            onboarding_proposal_submission_schema(),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "submit_work_plan",
            "Submit one structured WorkPlan for controller validation and operator review.",
            work_plan_submission_schema(),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "submit_implementation",
            "Submit the final Builder implementation record. A controller-valid submission terminates the Builder Run without a separate finish call.",
            implementation_submission_schema(),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "submit_test_outcome",
            "Submit structured test findings bound to declared acceptance evidence.",
            test_outcome_submission_schema(),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "submit_test_diagnosis",
            "Submit a read-only diagnosis of deterministic acceptance failures. A controller-valid submission terminates the diagnostic Run.",
            test_diagnosis_submission_schema(),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "submit_verification",
            "Submit structured verification findings with claims separated from evidence-backed facts.",
            verification_submission_schema(),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "run_shell",
            "Run a policy-gated local shell command inside the workspace. Non-zero exit is returned as structured output.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "cmd", "dry_run"],
                "properties": {
                    "reason": { "type": "string" },
                    "cmd": { "type": "string" },
                    "cwd": { "type": ["string", "null"] },
                    "timeout_ms": { "type": ["integer", "null"] },
                    "dry_run": { "type": "boolean" }
                }
            }),
            CapabilityKind::Shell,
        ),
        ToolSpec::new(
            "git_status",
            "Read git status for the workspace.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" }
                }
            }),
            CapabilityKind::Git,
        ),
        ToolSpec::new(
            "git_diff",
            "Read git diff for the workspace, optionally scoped by pathspec.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" },
                    "pathspec": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::Git,
        ),
        ToolSpec::new(
            "kubernetes_get",
            "Read Kubernetes resources with kubectl get -o json. Secret-shaped resources are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "resource", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "resource": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::KubernetesRead,
        ),
        ToolSpec::new(
            "argo_get_app",
            "Read an Argo CD Application CRD from the configured Argo CD namespace.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "app"],
                "properties": {
                    "reason": { "type": "string" },
                    "app": { "type": "string" }
                }
            }),
            CapabilityKind::ArgoRead,
        ),
        ToolSpec::new(
            "prometheus_query",
            "Run a read-only Prometheus instant query against PHARNESS_PROMETHEUS_URL.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "prometheus_inventory",
            "Read bounded Prometheus targets, rules, and active alerts from PHARNESS_PROMETHEUS_URL.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "loki_log_summary",
            "Read bounded Loki log lines from PHARNESS_LOKI_URL with compacted, redacted output.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" },
                    "since_seconds": {
                        "type": ["integer", "null"],
                        "minimum": 60,
                        "maximum": 86400
                    },
                    "limit": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "maximum": 100
                    }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "tekton_get_pipeline_runs",
            "Read Tekton PipelineRuns through the Kubernetes API. Secret-shaped names and labels are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::TektonRead,
        ),
        ToolSpec::new(
            "tekton_get_task_runs",
            "Read Tekton TaskRuns through the Kubernetes API. Secret-shaped names and labels are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::TektonRead,
        ),
        ToolSpec::new(
            "tekton_analyze_pipeline_run",
            "Read one Tekton PipelineRun and its related TaskRuns, then return a normalized PipelineRunAnalysis summary.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "namespace", "name"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": "string" },
                    "name": { "type": "string" }
                }
            }),
            CapabilityKind::TektonRead,
        ),
    ]
}

fn onboarding_proposal_submission_schema() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "additionalProperties":false,
        "required":["reason","proposal"],
        "properties":{
            "reason":{"type":"string","minLength":1,"maxLength":2000},
            "proposal":{
                "type":"object",
                "additionalProperties":false,
                "required":[
                    "schema_version","discovery_id","discovery_hash",
                    "candidate_contract","instructions","service_proposals",
                    "binding_proposals","assumptions","conflicts","blockers",
                    "readiness_forecast"
                ],
                "properties":{
                    "schema_version":{"type":"string","enum":[pharness_core::ONBOARDING_PROPOSAL_SCHEMA]},
                    "discovery_id":{"type":"string","minLength":1},
                    "discovery_hash":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},
                    "candidate_contract":{
                        "type":"object",
                        "additionalProperties":false,
                        "required":[
                            "api_version","environment_profile","dependency_lock",
                            "writable_paths","acceptance_commands","roots",
                            "agent_network","package_installation"
                        ],
                        "properties":{
                            "api_version":{"type":"string","enum":["pharness.dev/v1alpha1"]},
                            "environment_profile":{"type":"string","minLength":1},
                            "dependency_lock":{
                                "type":"object",
                                "additionalProperties":false,
                                "required":["kind","path","sha256"],
                                "properties":{
                                    "kind":{"type":"string","enum":["pip_requirements","npm_package_lock"]},
                                    "path":{"type":"string","minLength":1},
                                    "sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"}
                                }
                            },
                            "writable_paths":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"string","minLength":1}},
                            "acceptance_commands":{
                                "type":"array","minItems":1,"maxItems":50,
                                "items":{
                                    "type":"object","additionalProperties":false,
                                    "required":["name","command"],
                                    "properties":{
                                        "name":{"type":"string","minLength":1},
                                        "command":{"type":"string","minLength":1}
                                    }
                                }
                            },
                            "roots":{
                                "type":"object","additionalProperties":false,
                                "required":["source","tests","documentation"],
                                "properties":{
                                    "source":{"type":"array","items":{"type":"string"}},
                                    "tests":{"type":"array","items":{"type":"string"}},
                                    "documentation":{"type":"array","items":{"type":"string"}}
                                }
                            },
                            "agent_network":{"type":"string","enum":["denied"]},
                            "package_installation":{"type":"string","enum":["preparation_only","denied"]}
                        }
                    },
                    "instructions":{"type":"string","maxLength":32768},
                    "service_proposals":{
                        "type":"array","maxItems":50,
                        "items":{
                            "type":"object","additionalProperties":false,
                            "required":["service_key","display_name","description"],
                            "properties":{
                                "service_key":{"type":"string","minLength":1},
                                "display_name":{"type":"string","minLength":1},
                                "description":{"type":"string"}
                            }
                        }
                    },
                    "binding_proposals":{
                        "type":"array","maxItems":50,
                        "items":{
                            "type":"object","additionalProperties":false,
                            "required":["service_keys","scopes"],
                            "properties":{
                                "service_keys":{"type":"array","items":{"type":"string"}},
                                "scopes":{"type":"array","items":{"type":"string"}}
                            }
                        }
                    },
                    "assumptions":{"type":"array","maxItems":100,"items":{"type":"string"}},
                    "conflicts":{"type":"array","maxItems":100,"items":{"type":"string"}},
                    "blockers":{"type":"array","maxItems":100,"items":{"type":"string"}},
                    "readiness_forecast":{"type":"object"}
                }
            }
        }
    })
}

fn work_plan_submission_schema() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "additionalProperties":false,
        "required":["reason","work_plan"],
        "properties":{
            "reason":{"type":"string"},
            "work_plan":{
                "type":"object",
                "additionalProperties":false,
                "required":["title","summary","risk_level","steps"],
                "properties":{
                    "title":{"type":"string","minLength":1,"maxLength":200},
                    "summary":{"type":"string","minLength":1,"maxLength":4000},
                    "risk_level":{"type":"string","enum":["low","medium","high"]},
                    "steps":{
                        "type":"array","minItems":1,"maxItems":50,
                        "items":{
                            "type":"object",
                            "additionalProperties":false,
                            "required":["title","description"],
                            "properties":{
                                "title":{"type":"string","minLength":1,"maxLength":200},
                                "description":{"type":"string","minLength":1,"maxLength":2000},
                                "paths":{"type":"array","items":{"type":"string"},"maxItems":100},
                                "acceptance_names":{"type":"array","items":{"type":"string"},"maxItems":50}
                            }
                        }
                    },
                    "assumptions":{"type":"array","items":{"type":"string"},"maxItems":50},
                    "risks":{"type":"array","items":{"type":"string"},"maxItems":50}
                }
            }
        }
    })
}

fn implementation_submission_schema() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "additionalProperties":false,
        "required":["reason","implementation"],
        "properties":{
            "reason":{"type":"string","minLength":1,"maxLength":2000},
            "implementation":{
                "type":"object",
                "additionalProperties":false,
                "required":["summary","changed_paths","acceptance_names","risks"],
                "properties":{
                    "summary":{"type":"string","minLength":1,"maxLength":4000},
                    "changed_paths":{"type":"array","minItems":1,"maxItems":200,"items":{"type":"string","minLength":1}},
                    "acceptance_names":{"type":"array","minItems":1,"maxItems":50,"items":{"type":"string","minLength":1}},
                    "risks":{"type":"array","maxItems":50,"items":{"type":"string"}}
                }
            }
        }
    })
}

fn test_outcome_submission_schema() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "additionalProperties":false,
        "required":["reason","outcome"],
        "properties":{
            "reason":{"type":"string"},
            "outcome":{
                "type":"object",
                "additionalProperties":false,
                "required":["summary","acceptance_names","claims"],
                "properties":{
                    "summary":{"type":"string","minLength":1,"maxLength":4000},
                    "acceptance_names":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":50},
                    "claims":{"type":"array","items":{"type":"string"},"maxItems":50},
                    "risks":{"type":"array","items":{"type":"string"},"maxItems":50}
                }
            }
        }
    })
}

fn test_diagnosis_submission_schema() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "additionalProperties":false,
        "required":["reason","diagnosis"],
        "properties":{
            "reason":{"type":"string","minLength":1,"maxLength":2000},
            "diagnosis":{
                "type":"object",
                "additionalProperties":false,
                "required":["summary","failure_kind","evidence_refs","repair_recommendations"],
                "properties":{
                    "summary":{"type":"string","minLength":1,"maxLength":4000},
                    "failure_kind":{"type":"string","enum":["compilation","assertion","lint","semantic_test","structural_environment","unknown"]},
                    "evidence_refs":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"string","minLength":1}},
                    "repair_recommendations":{"type":"array","maxItems":50,"items":{"type":"string"}}
                }
            }
        }
    })
}

fn verification_submission_schema() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "additionalProperties":false,
        "required":["reason","verification"],
        "properties":{
            "reason":{"type":"string"},
            "verification":{
                "type":"object",
                "additionalProperties":false,
                "required":["decision","summary","evidence_refs","contradictions","risks"],
                "properties":{
                    "decision":{"type":"string","enum":["approved","rejected"]},
                    "summary":{"type":"string","minLength":1,"maxLength":4000},
                    "evidence_refs":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":100},
                    "contradictions":{"type":"array","items":{"type":"string"},"maxItems":50},
                    "risks":{"type":"array","items":{"type":"string"},"maxItems":50}
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        system_prompt, worker_tool_specs, SYSTEM_PROMPT_VERSION, WORKSPACE_COMMAND_EXECUTABLES,
    };
    use std::collections::HashSet;

    #[test]
    fn worker_tool_schema_contains_terminal_and_read_only_actions() {
        let names = worker_tool_specs()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        for expected in [
            "respond",
            "finish",
            "list_dir",
            "read_file",
            "search_files",
            "write_file",
            "patch_file",
            "run_shell",
            "git_status",
            "git_diff",
            "kubernetes_get",
            "argo_get_app",
            "prometheus_query",
            "prometheus_inventory",
            "loki_log_summary",
            "tekton_get_pipeline_runs",
            "tekton_get_task_runs",
            "tekton_analyze_pipeline_run",
            "get_evidence",
            "submit_onboarding_proposal",
            "submit_work_plan",
            "submit_test_outcome",
            "submit_verification",
        ] {
            assert!(names.contains(expected), "missing tool spec for {expected}");
        }
    }

    #[test]
    fn worker_tool_schema_does_not_expose_non_resumable_approval_by_default() {
        let names = worker_tool_specs()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        assert!(!names.contains("request_approval"));
    }

    #[test]
    fn atomic_patch_schema_exposes_its_exact_preimage_contract() {
        let tools = worker_tool_specs();
        let read = tools.iter().find(|tool| tool.name == "read_file").unwrap();
        let patch = tools
            .iter()
            .find(|tool| tool.name == "apply_patch")
            .unwrap();

        assert!(read.description.contains("complete file"));
        assert!(patch.description.contains("every existing file"));
        assert!(patch.parameters_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "preimage_sha256"));
    }

    #[test]
    fn workspace_command_schema_exposes_only_verified_runner_executables() {
        let command = worker_tool_specs()
            .into_iter()
            .find(|tool| tool.name == "run_workspace_command")
            .unwrap();
        assert_eq!(
            command.parameters_schema["properties"]["executable"]["enum"],
            serde_json::json!(WORKSPACE_COMMAND_EXECUTABLES)
        );
        assert!(command.description.contains("environment discovery"));
    }

    #[test]
    fn worker_prompt_forbids_toolchain_discovery_before_execution() {
        assert_eq!(SYSTEM_PROMPT_VERSION, "2026-08-29.3");
        let prompt = system_prompt();
        for prohibited_probe in ["`which`", "`command -v`", "version probes"] {
            assert!(prompt.contains(prohibited_probe));
        }
        assert!(prompt.contains("Run only the repository's direct test or validation command"));
    }

    #[test]
    fn onboarding_tool_schema_requires_the_controller_contract_shape() {
        let tool = worker_tool_specs()
            .into_iter()
            .find(|tool| tool.name == "submit_onboarding_proposal")
            .unwrap();
        let proposal = &tool.parameters_schema["properties"]["proposal"];
        assert_eq!(proposal["additionalProperties"], false);
        assert!(proposal["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "discovery_hash"));
        assert_eq!(
            proposal["properties"]["candidate_contract"]["properties"]["dependency_lock"]
                ["properties"]["sha256"]["pattern"],
            "^[0-9a-f]{64}$"
        );
    }
}
