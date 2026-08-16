use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const PROJECT_CONTRACT_PATH: &str = ".pharness/project.yaml";
pub const MAX_PROJECT_CONTRACT_BYTES: usize = 32 * 1024;

#[derive(Debug, Error)]
pub enum ProjectContractError {
    #[error("project contract is missing at {PROJECT_CONTRACT_PATH}")]
    Missing,
    #[error("project contract exceeds {MAX_PROJECT_CONTRACT_BYTES} bytes")]
    TooLarge,
    #[error("project contract is not valid YAML: {0}")]
    InvalidYaml(String),
    #[error("project contract is invalid: {0}")]
    Invalid(String),
    #[error("project contract path escapes the repository: {0}")]
    PathEscape(String),
    #[error("project contract I/O failed: {0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectContract {
    pub api_version: String,
    pub environment_profile: String,
    pub dependency_lock: DependencyLock,
    pub writable_paths: Vec<String>,
    pub acceptance_commands: Vec<AcceptanceCommand>,
    pub roots: ProjectRoots,
    pub agent_network: AgentNetworkPolicy,
    pub package_installation: PackageInstallationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyLock {
    pub kind: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCommand {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRoots {
    pub source: Vec<String>,
    pub tests: Vec<String>,
    pub documentation: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentNetworkPolicy {
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageInstallationPolicy {
    PreparationOnly,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentProfile {
    pub id: String,
    pub active: bool,
    pub image: String,
    pub revision: String,
    pub platform: String,
    pub required_executables: Vec<String>,
    pub preparation_strategy: PreparationStrategy,
    pub service_account: String,
    #[serde(default)]
    pub repository_allowlist: Vec<String>,
    pub limits: EnvironmentProfileLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationStrategy {
    PythonHashedRequirements,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentProfileLimits {
    pub cpu: String,
    pub memory: String,
    pub ephemeral_storage: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub source_sha: String,
    pub manifest_sha256: String,
    pub dependency_lock_sha256: String,
    pub runner_image_digest: String,
    pub runner_revision: String,
    pub os: String,
    pub architecture: String,
    pub effective_user: String,
    pub python_version: String,
    pub python_path: String,
    pub writable_paths: Vec<String>,
    pub unavailable_tools: Vec<String>,
    pub agent_network: AgentNetworkPolicy,
    pub package_installation: PackageInstallationPolicy,
    pub acceptance_commands: Vec<AcceptanceCommand>,
    pub preparation_evidence: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunBudget {
    pub initial_turns: u32,
    pub hard_turns: u32,
    pub initial_tokens: u64,
    pub hard_tokens: u64,
    pub active_execution_seconds: u64,
    pub recoverable_tool_errors: u32,
    pub identical_failures: u32,
    pub verification_reserve_turns: u32,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            initial_turns: 48,
            hard_turns: 100,
            initial_tokens: 400_000,
            hard_tokens: 1_000_000,
            active_execution_seconds: 3_600,
            recoverable_tool_errors: 4,
            identical_failures: 2,
            verification_reserve_turns: 8,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudgetConsumption {
    pub allowed_turns: u32,
    pub allowed_tokens: u64,
    pub turns_used: u32,
    pub tokens_used: u64,
    pub active_execution_seconds_used: u64,
    pub extensions: u32,
}

impl RunBudget {
    pub fn validate(&self) -> Result<(), ProjectContractError> {
        if !(1..=100).contains(&self.initial_turns)
            || self.hard_turns > 100
            || self.initial_turns > self.hard_turns
        {
            return Err(ProjectContractError::Invalid(
                "turn budget must be ordered and capped at 100".into(),
            ));
        }
        if self.initial_tokens == 0
            || self.hard_tokens > 1_000_000
            || self.initial_tokens > self.hard_tokens
        {
            return Err(ProjectContractError::Invalid(
                "token budget must be ordered and capped at 1000000".into(),
            ));
        }
        if self.active_execution_seconds == 0
            || self.recoverable_tool_errors > 8
            || !(1..=3).contains(&self.identical_failures)
            || self.verification_reserve_turns >= self.initial_turns
        {
            return Err(ProjectContractError::Invalid(
                "active time or recovery thresholds are invalid".into(),
            ));
        }
        Ok(())
    }
}

impl EnvironmentProfile {
    pub fn validate(&self) -> Result<(), ProjectContractError> {
        validate_identifier(&self.id, "environment profile id")?;
        validate_digest_ref(&self.image)?;
        if !is_full_sha(&self.revision) {
            return Err(ProjectContractError::Invalid(
                "environment profile revision must be a full Git SHA".into(),
            ));
        }
        if self.platform != "linux/amd64" {
            return Err(ProjectContractError::Invalid(
                "production environment profiles must use linux/amd64".into(),
            ));
        }
        if self.required_executables.is_empty()
            || self.required_executables.iter().any(|value| {
                value.is_empty()
                    || value.contains('/')
                    || !value.bytes().all(is_safe_identifier_byte)
            })
        {
            return Err(ProjectContractError::Invalid(
                "required executables must be non-empty command names".into(),
            ));
        }
        if self.service_account.trim().is_empty() {
            return Err(ProjectContractError::Invalid(
                "environment profile service account is required".into(),
            ));
        }
        Ok(())
    }
}

impl ProjectContract {
    pub fn load(workspace: &Path) -> Result<(Self, String), ProjectContractError> {
        let root = workspace
            .canonicalize()
            .map_err(|error| ProjectContractError::Io(error.to_string()))?;
        let path = root.join(PROJECT_CONTRACT_PATH);
        if !path.exists() {
            return Err(ProjectContractError::Missing);
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| ProjectContractError::Io(error.to_string()))?;
        if !canonical.starts_with(&root) {
            return Err(ProjectContractError::PathEscape(
                PROJECT_CONTRACT_PATH.into(),
            ));
        }
        let bytes = std::fs::read(&canonical)
            .map_err(|error| ProjectContractError::Io(error.to_string()))?;
        if bytes.len() > MAX_PROJECT_CONTRACT_BYTES {
            return Err(ProjectContractError::TooLarge);
        }
        let contract: Self = serde_yaml::from_slice(&bytes)
            .map_err(|error| ProjectContractError::InvalidYaml(error.to_string()))?;
        contract.validate(&root)?;
        Ok((contract, sha256_hex(&bytes)))
    }

    pub fn validate(&self, workspace: &Path) -> Result<(), ProjectContractError> {
        if self.api_version != "pharness.dev/v1alpha1" {
            return Err(ProjectContractError::Invalid(
                "api_version must be pharness.dev/v1alpha1".into(),
            ));
        }
        validate_identifier(&self.environment_profile, "environment_profile")?;
        if self.dependency_lock.kind != "pip_requirements" {
            return Err(ProjectContractError::Invalid(
                "dependency_lock.kind must be pip_requirements".into(),
            ));
        }
        let lock = resolve_declared_path(workspace, &self.dependency_lock.path)?;
        let lock_bytes =
            std::fs::read(&lock).map_err(|error| ProjectContractError::Io(error.to_string()))?;
        if !is_sha256(&self.dependency_lock.sha256)
            || sha256_hex(&lock_bytes) != self.dependency_lock.sha256.to_ascii_lowercase()
        {
            return Err(ProjectContractError::Invalid(
                "dependency lock SHA-256 does not match the pinned file".into(),
            ));
        }
        validate_immutable_pip_lock(&lock_bytes)?;
        validate_unique_paths(&self.writable_paths, "writable_paths", true)?;
        if self.writable_paths.is_empty() {
            return Err(ProjectContractError::Invalid(
                "writable_paths must not be empty".into(),
            ));
        }
        validate_declared_paths(workspace, &self.writable_paths)?;
        let mut command_names = BTreeSet::new();
        if self.acceptance_commands.is_empty() {
            return Err(ProjectContractError::Invalid(
                "acceptance_commands must not be empty".into(),
            ));
        }
        for command in &self.acceptance_commands {
            validate_identifier(&command.name, "acceptance command name")?;
            if !command_names.insert(command.name.as_str()) {
                return Err(ProjectContractError::Invalid(format!(
                    "duplicate acceptance command {}",
                    command.name
                )));
            }
            validate_command(&command.command)?;
        }
        validate_unique_paths(&self.roots.source, "roots.source", false)?;
        validate_unique_paths(&self.roots.tests, "roots.tests", false)?;
        validate_unique_paths(&self.roots.documentation, "roots.documentation", false)?;
        if self.roots.source.is_empty() || self.roots.tests.is_empty() {
            return Err(ProjectContractError::Invalid(
                "source and test roots are required".into(),
            ));
        }
        validate_declared_paths(workspace, &self.roots.source)?;
        validate_declared_paths(workspace, &self.roots.tests)?;
        validate_declared_paths(workspace, &self.roots.documentation)?;
        Ok(())
    }

    pub fn command(&self, name: &str) -> Option<&AcceptanceCommand> {
        self.acceptance_commands
            .iter()
            .find(|command| command.name == name)
    }
}

fn resolve_declared_path(root: &Path, value: &str) -> Result<PathBuf, ProjectContractError> {
    validate_relative_path(value, false)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| ProjectContractError::Io(error.to_string()))?;
    let path = canonical_root.join(value);
    let canonical = path
        .canonicalize()
        .map_err(|error| ProjectContractError::Io(error.to_string()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(ProjectContractError::PathEscape(value.into()));
    }
    Ok(canonical)
}

fn validate_unique_paths(
    values: &[String],
    label: &str,
    allow_glob: bool,
) -> Result<(), ProjectContractError> {
    let mut unique = BTreeSet::new();
    for value in values {
        validate_relative_path(value, allow_glob)?;
        if !unique.insert(value.as_str()) {
            return Err(ProjectContractError::Invalid(format!(
                "{label} contains duplicate path {value}"
            )));
        }
    }
    Ok(())
}

fn validate_declared_paths(
    workspace: &Path,
    values: &[String],
) -> Result<(), ProjectContractError> {
    let canonical_root = workspace
        .canonicalize()
        .map_err(|error| ProjectContractError::Io(error.to_string()))?;
    for value in values {
        let value = value.strip_suffix("/**").unwrap_or(value);
        validate_relative_path(value, false)?;
        let path = canonical_root.join(value);
        let mut existing = path.as_path();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| ProjectContractError::PathEscape(value.to_string()))?;
        }
        let canonical_existing = existing
            .canonicalize()
            .map_err(|error| ProjectContractError::Io(error.to_string()))?;
        if !canonical_existing.starts_with(&canonical_root) {
            return Err(ProjectContractError::PathEscape(value.to_string()));
        }
    }
    Ok(())
}

fn validate_immutable_pip_lock(bytes: &[u8]) -> Result<(), ProjectContractError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ProjectContractError::Invalid("pip requirements lock must be UTF-8".into()))?;
    let mut logical = String::new();
    let mut requirements = 0_u32;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let continued = line.ends_with('\\');
        let segment = line.strip_suffix('\\').unwrap_or(line).trim();
        if !logical.is_empty() {
            logical.push(' ');
        }
        logical.push_str(segment);
        if continued {
            continue;
        }
        validate_locked_requirement(&logical)?;
        requirements = requirements.saturating_add(1);
        logical.clear();
    }
    if !logical.is_empty() {
        validate_locked_requirement(&logical)?;
        requirements = requirements.saturating_add(1);
    }
    if requirements == 0 {
        return Err(ProjectContractError::Invalid(
            "pip requirements lock must contain at least one exact requirement".into(),
        ));
    }
    Ok(())
}

fn validate_locked_requirement(requirement: &str) -> Result<(), ProjectContractError> {
    let lower = requirement.to_ascii_lowercase();
    if requirement.starts_with('-')
        || !requirement.contains("==")
        || lower.contains("git+")
        || lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("file:")
        || lower.contains(" @ ")
        || lower.contains("../")
        || lower.contains("./")
    {
        return Err(ProjectContractError::Invalid(format!(
            "pip requirements lock contains a mutable dependency input: {requirement:?}"
        )));
    }
    let hashes = requirement
        .split_whitespace()
        .filter_map(|part| part.strip_prefix("--hash=sha256:"))
        .collect::<Vec<_>>();
    if hashes.is_empty() || hashes.iter().any(|hash| !is_sha256(hash)) {
        return Err(ProjectContractError::Invalid(format!(
            "exact requirement is missing a valid SHA-256 hash: {requirement:?}"
        )));
    }
    Ok(())
}

fn validate_relative_path(value: &str, allow_glob: bool) -> Result<(), ProjectContractError> {
    if value.is_empty() || value.len() > 256 || value.contains('\\') {
        return Err(ProjectContractError::Invalid(format!(
            "invalid repository-relative path {value:?}"
        )));
    }
    let plain = value.strip_suffix("/**").unwrap_or(value);
    if value.contains('*') && (!allow_glob || !value.ends_with("/**") || plain.contains('*')) {
        return Err(ProjectContractError::Invalid(format!(
            "unsupported path glob {value:?}"
        )));
    }
    let path = Path::new(plain);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || secret_shaped_path(plain)
    {
        return Err(ProjectContractError::Invalid(format!(
            "unsafe repository-relative path {value:?}"
        )));
    }
    Ok(())
}

fn validate_command(command: &str) -> Result<(), ProjectContractError> {
    let lower = command.to_ascii_lowercase();
    if command.is_empty()
        || command.len() > 1024
        || command.contains('\n')
        || ["&&", "||", ";", "|", ">", "<", "`", "$("]
            .iter()
            .any(|needle| command.contains(needle))
        || [
            "curl ",
            "wget ",
            "http://",
            "https://",
            "pip install",
            "apt ",
            "apt-get ",
            "apk ",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return Err(ProjectContractError::Invalid(format!(
            "acceptance command is not an exact offline command: {command:?}"
        )));
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), ProjectContractError> {
    if value.is_empty()
        || value.len() > 64
        || !value.bytes().all(is_safe_identifier_byte)
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(ProjectContractError::Invalid(format!(
            "{label} is not a safe identifier"
        )));
    }
    Ok(())
}

fn is_safe_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
}

fn secret_shaped_path(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.split('/').any(|part| {
        part == ".env"
            || part.starts_with(".env.")
            || part.ends_with(".pem")
            || part.ends_with(".key")
            || part.contains("secret")
            || part.contains("credential")
            || part.contains("token")
            || part.contains("kubeconfig")
    })
}

fn validate_digest_ref(value: &str) -> Result<(), ProjectContractError> {
    let Some((repository, digest)) = value.rsplit_once("@sha256:") else {
        return Err(ProjectContractError::Invalid(
            "environment profile image must use repository@sha256:digest".into(),
        ));
    };
    if repository.is_empty() || !is_sha256(digest) {
        return Err(ProjectContractError::Invalid(
            "environment profile image digest is malformed".into(),
        ));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_full_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "pharness-project-contract-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".pharness")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("readme.md"), "# Fixture\n").unwrap();
        let lock = b"fastapi==1.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        std::fs::write(root.join("requirements.lock"), lock).unwrap();
        let hash = sha256_hex(lock);
        let yaml = format!(
            r#"api_version: pharness.dev/v1alpha1
environment_profile: python-3.11
dependency_lock:
  kind: pip_requirements
  path: requirements.lock
  sha256: {hash}
writable_paths: [src/**, tests/**, readme.md]
acceptance_commands:
  - name: unit
    command: python -m unittest discover -s tests -v
  - name: compile
    command: python -m compileall -q src tests
roots:
  source: [src]
  tests: [tests]
  documentation: [readme.md]
agent_network: denied
package_installation: preparation_only
"#
        );
        std::fs::write(root.join(PROJECT_CONTRACT_PATH), yaml).unwrap();
        root
    }

    #[test]
    fn loads_strict_hashed_contract() {
        let root = fixture();
        let (contract, hash) = ProjectContract::load(&root).unwrap();
        assert_eq!(contract.environment_profile, "python-3.11");
        assert_eq!(hash.len(), 64);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_fields_and_lock_drift() {
        let root = fixture();
        let path = root.join(PROJECT_CONTRACT_PATH);
        let mut yaml = std::fs::read_to_string(&path).unwrap();
        yaml.push_str("surprise: true\n");
        std::fs::write(&path, yaml).unwrap();
        assert!(matches!(
            ProjectContract::load(&root),
            Err(ProjectContractError::InvalidYaml(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_traversal_and_secret_paths() {
        assert!(validate_relative_path("../src/**", true).is_err());
        assert!(validate_relative_path(".env", false).is_err());
        assert!(validate_relative_path("src/*.py", true).is_err());
        let root = fixture();
        validate_declared_paths(&root, &["new-tests".to_string()]).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_mutable_or_partially_hashed_dependency_locks() {
        assert!(validate_immutable_pip_lock(
            b"safe==1.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nunsafe==2.0\n"
        )
        .is_err());
        assert!(validate_immutable_pip_lock(
            b"unsafe @ git+https://github.com/example/unsafe.git --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_declared_symlink_escapes() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escaped")).unwrap();
        assert!(validate_declared_paths(&root, &["escaped".to_string()]).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn validates_budget_caps() {
        RunBudget::default().validate().unwrap();
        let invalid = RunBudget {
            hard_turns: 101,
            ..RunBudget::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn profile_requires_an_immutable_linux_amd64_artifact() {
        let valid = EnvironmentProfile {
            id: "python-3.11".to_string(),
            active: true,
            image: format!("registry.example/pharness-python@sha256:{}", "a".repeat(64)),
            revision: "b".repeat(40),
            platform: "linux/amd64".to_string(),
            required_executables: vec!["python".to_string(), "git".to_string()],
            preparation_strategy: PreparationStrategy::PythonHashedRequirements,
            service_account: "pharness-python-runner".to_string(),
            repository_allowlist: vec!["lward27/yfinance_wrapper".to_string()],
            limits: EnvironmentProfileLimits {
                cpu: "2".to_string(),
                memory: "4Gi".to_string(),
                ephemeral_storage: "2Gi".to_string(),
            },
        };
        valid.validate().unwrap();
        assert!(EnvironmentProfile {
            image: "registry.example/pharness-python:latest".to_string(),
            ..valid.clone()
        }
        .validate()
        .is_err());
        assert!(EnvironmentProfile {
            platform: "linux/arm64".to_string(),
            ..valid
        }
        .validate()
        .is_err());
    }
}
