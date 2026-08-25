use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const REPOSITORY_CONTRACT_PATH: &str = ".pharness/repository.yaml";
pub const LEGACY_PROJECT_CONTRACT_PATH: &str = ".pharness/project.yaml";
#[deprecated(note = "use REPOSITORY_CONTRACT_PATH")]
pub const PROJECT_CONTRACT_PATH: &str = LEGACY_PROJECT_CONTRACT_PATH;
pub const MAX_REPOSITORY_CONTRACT_BYTES: usize = 32 * 1024;
#[deprecated(note = "use MAX_REPOSITORY_CONTRACT_BYTES")]
pub const MAX_PROJECT_CONTRACT_BYTES: usize = MAX_REPOSITORY_CONTRACT_BYTES;

#[derive(Debug, Error)]
pub enum RepositoryContractError {
    #[error(
        "repository contract is missing at {REPOSITORY_CONTRACT_PATH} or {LEGACY_PROJECT_CONTRACT_PATH}"
    )]
    Missing,
    #[error("repository contract exceeds {MAX_REPOSITORY_CONTRACT_BYTES} bytes")]
    TooLarge,
    #[error("repository contract is not valid YAML: {0}")]
    InvalidYaml(String),
    #[error("repository contract is invalid: {0}")]
    Invalid(String),
    #[error("repository contract path escapes the repository: {0}")]
    PathEscape(String),
    #[error("repository contract I/O failed: {0}")]
    Io(String),
    #[error("canonical repository contract is required at {REPOSITORY_CONTRACT_PATH}")]
    CanonicalRequired,
    #[error(
        "canonical and deprecated repository contracts conflict: {REPOSITORY_CONTRACT_PATH} and {LEGACY_PROJECT_CONTRACT_PATH}"
    )]
    ConflictingAliases,
}

#[deprecated(note = "use RepositoryContractError")]
pub type ProjectContractError = RepositoryContractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryContractSource {
    Canonical,
    CanonicalWithMatchingAlias,
    LegacyAlias,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedRepositoryContract {
    pub contract: RepositoryContract,
    pub content_sha256: String,
    pub active_path: String,
    pub source: RepositoryContractSource,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryContract {
    pub api_version: String,
    pub environment_profile: String,
    pub dependency_lock: DependencyLock,
    pub writable_paths: Vec<String>,
    pub acceptance_commands: Vec<AcceptanceCommand>,
    pub roots: ProjectRoots,
    pub agent_network: AgentNetworkPolicy,
    pub package_installation: PackageInstallationPolicy,
}

#[deprecated(note = "use RepositoryContract")]
pub type ProjectContract = RepositoryContract;

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
    pub fn validate(&self) -> Result<(), RepositoryContractError> {
        if !(1..=100).contains(&self.initial_turns)
            || self.hard_turns > 100
            || self.initial_turns > self.hard_turns
        {
            return Err(RepositoryContractError::Invalid(
                "turn budget must be ordered and capped at 100".into(),
            ));
        }
        if self.initial_tokens == 0
            || self.hard_tokens > 1_000_000
            || self.initial_tokens > self.hard_tokens
        {
            return Err(RepositoryContractError::Invalid(
                "token budget must be ordered and capped at 1000000".into(),
            ));
        }
        if self.active_execution_seconds == 0
            || self.recoverable_tool_errors > 8
            || !(1..=3).contains(&self.identical_failures)
            || self.verification_reserve_turns >= self.initial_turns
        {
            return Err(RepositoryContractError::Invalid(
                "active time or recovery thresholds are invalid".into(),
            ));
        }
        Ok(())
    }
}

impl EnvironmentProfile {
    pub fn validate(&self) -> Result<(), RepositoryContractError> {
        validate_identifier(&self.id, "environment profile id")?;
        validate_digest_ref(&self.image)?;
        if !is_full_sha(&self.revision) {
            return Err(RepositoryContractError::Invalid(
                "environment profile revision must be a full Git SHA".into(),
            ));
        }
        if self.platform != "linux/amd64" {
            return Err(RepositoryContractError::Invalid(
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
            return Err(RepositoryContractError::Invalid(
                "required executables must be non-empty command names".into(),
            ));
        }
        if self.service_account.trim().is_empty() {
            return Err(RepositoryContractError::Invalid(
                "environment profile service account is required".into(),
            ));
        }
        Ok(())
    }
}

impl RepositoryContract {
    /// Validate the executable shape of an onboarding proposal before a
    /// checkout exists. Exact lock bytes, roots, and symlink containment are
    /// still revalidated from the merged immutable revision before readiness.
    pub fn validate_candidate(&self) -> Result<(), RepositoryContractError> {
        if self.api_version != "pharness.dev/v1alpha1" {
            return Err(RepositoryContractError::Invalid(
                "api_version must be pharness.dev/v1alpha1".into(),
            ));
        }
        validate_identifier(&self.environment_profile, "environment_profile")?;
        if self.dependency_lock.kind != "pip_requirements" {
            return Err(RepositoryContractError::Invalid(
                "dependency_lock.kind must be pip_requirements".into(),
            ));
        }
        validate_relative_path(&self.dependency_lock.path, false)?;
        if !is_sha256(&self.dependency_lock.sha256) {
            return Err(RepositoryContractError::Invalid(
                "dependency lock must declare a full SHA-256".into(),
            ));
        }
        validate_unique_paths(&self.writable_paths, "writable_paths", true)?;
        if self.writable_paths.is_empty() {
            return Err(RepositoryContractError::Invalid(
                "writable_paths must not be empty".into(),
            ));
        }
        let mut command_names = BTreeSet::new();
        if self.acceptance_commands.is_empty() {
            return Err(RepositoryContractError::Invalid(
                "acceptance_commands must not be empty".into(),
            ));
        }
        for command in &self.acceptance_commands {
            validate_identifier(&command.name, "acceptance command name")?;
            if !command_names.insert(command.name.as_str()) {
                return Err(RepositoryContractError::Invalid(format!(
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
            return Err(RepositoryContractError::Invalid(
                "source and test roots are required".into(),
            ));
        }
        Ok(())
    }

    /// Compatibility loader used by legacy WorkItems. Repo Mode callers must
    /// use `load_for_repo_mode`, which rejects alias-only repositories.
    pub fn load(workspace: &Path) -> Result<(Self, String), RepositoryContractError> {
        let loaded = Self::load_with_metadata(workspace)?;
        Ok((loaded.contract, loaded.content_sha256))
    }

    pub fn load_for_repo_mode(
        workspace: &Path,
    ) -> Result<LoadedRepositoryContract, RepositoryContractError> {
        let loaded = Self::load_with_metadata(workspace)?;
        if loaded.source == RepositoryContractSource::LegacyAlias {
            return Err(RepositoryContractError::CanonicalRequired);
        }
        Ok(loaded)
    }

    pub fn load_with_metadata(
        workspace: &Path,
    ) -> Result<LoadedRepositoryContract, RepositoryContractError> {
        let root = workspace
            .canonicalize()
            .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
        let canonical_path = root.join(REPOSITORY_CONTRACT_PATH);
        let alias_path = root.join(LEGACY_PROJECT_CONTRACT_PATH);
        let canonical_exists = canonical_path.exists();
        let alias_exists = alias_path.exists();
        if !canonical_exists && !alias_exists {
            return Err(RepositoryContractError::Missing);
        }

        let (path, active_path, source, warnings) = match (canonical_exists, alias_exists) {
            (true, true) => {
                let canonical_bytes =
                    read_contract_bytes(&root, &canonical_path, REPOSITORY_CONTRACT_PATH)?;
                let alias_bytes =
                    read_contract_bytes(&root, &alias_path, LEGACY_PROJECT_CONTRACT_PATH)?;
                if canonical_bytes != alias_bytes {
                    return Err(RepositoryContractError::ConflictingAliases);
                }
                (
                    canonical_path,
                    REPOSITORY_CONTRACT_PATH,
                    RepositoryContractSource::CanonicalWithMatchingAlias,
                    vec![format!(
                        "{LEGACY_PROJECT_CONTRACT_PATH} is deprecated and should be removed"
                    )],
                )
            }
            (true, false) => (
                canonical_path,
                REPOSITORY_CONTRACT_PATH,
                RepositoryContractSource::Canonical,
                Vec::new(),
            ),
            (false, true) => (
                alias_path,
                LEGACY_PROJECT_CONTRACT_PATH,
                RepositoryContractSource::LegacyAlias,
                vec![format!(
                    "{LEGACY_PROJECT_CONTRACT_PATH} is a deprecated compatibility alias"
                )],
            ),
            (false, false) => unreachable!("missing contracts returned above"),
        };

        let bytes = read_contract_bytes(&root, &path, active_path)?;
        let contract: Self = serde_yaml::from_slice(&bytes)
            .map_err(|error| RepositoryContractError::InvalidYaml(error.to_string()))?;
        contract.validate(&root)?;
        Ok(LoadedRepositoryContract {
            contract,
            content_sha256: sha256_hex(&bytes),
            active_path: active_path.to_string(),
            source,
            warnings,
        })
    }

    pub fn validate(&self, workspace: &Path) -> Result<(), RepositoryContractError> {
        self.validate_candidate()?;
        let lock = resolve_declared_path(workspace, &self.dependency_lock.path)?;
        let lock_bytes =
            std::fs::read(&lock).map_err(|error| RepositoryContractError::Io(error.to_string()))?;
        if !is_sha256(&self.dependency_lock.sha256)
            || sha256_hex(&lock_bytes) != self.dependency_lock.sha256.to_ascii_lowercase()
        {
            return Err(RepositoryContractError::Invalid(
                "dependency lock SHA-256 does not match the pinned file".into(),
            ));
        }
        validate_immutable_pip_lock(&lock_bytes)?;
        validate_declared_paths(workspace, &self.writable_paths)?;
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

fn read_contract_bytes(
    root: &Path,
    path: &Path,
    display_path: &str,
) -> Result<Vec<u8>, RepositoryContractError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(RepositoryContractError::PathEscape(display_path.into()));
    }
    let bytes = std::fs::read(&canonical)
        .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
    if bytes.len() > MAX_REPOSITORY_CONTRACT_BYTES {
        return Err(RepositoryContractError::TooLarge);
    }
    Ok(bytes)
}

fn resolve_declared_path(root: &Path, value: &str) -> Result<PathBuf, RepositoryContractError> {
    validate_relative_path(value, false)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
    let path = canonical_root.join(value);
    let canonical = path
        .canonicalize()
        .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(RepositoryContractError::PathEscape(value.into()));
    }
    Ok(canonical)
}

fn validate_unique_paths(
    values: &[String],
    label: &str,
    allow_glob: bool,
) -> Result<(), RepositoryContractError> {
    let mut unique = BTreeSet::new();
    for value in values {
        validate_relative_path(value, allow_glob)?;
        if !unique.insert(value.as_str()) {
            return Err(RepositoryContractError::Invalid(format!(
                "{label} contains duplicate path {value}"
            )));
        }
    }
    Ok(())
}

fn validate_declared_paths(
    workspace: &Path,
    values: &[String],
) -> Result<(), RepositoryContractError> {
    let canonical_root = workspace
        .canonicalize()
        .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
    for value in values {
        let value = value.strip_suffix("/**").unwrap_or(value);
        validate_relative_path(value, false)?;
        let path = canonical_root.join(value);
        let mut existing = path.as_path();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| RepositoryContractError::PathEscape(value.to_string()))?;
        }
        let canonical_existing = existing
            .canonicalize()
            .map_err(|error| RepositoryContractError::Io(error.to_string()))?;
        if !canonical_existing.starts_with(&canonical_root) {
            return Err(RepositoryContractError::PathEscape(value.to_string()));
        }
    }
    Ok(())
}

fn validate_immutable_pip_lock(bytes: &[u8]) -> Result<(), RepositoryContractError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        RepositoryContractError::Invalid("pip requirements lock must be UTF-8".into())
    })?;
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
        return Err(RepositoryContractError::Invalid(
            "pip requirements lock must contain at least one exact requirement".into(),
        ));
    }
    Ok(())
}

fn validate_locked_requirement(requirement: &str) -> Result<(), RepositoryContractError> {
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
        return Err(RepositoryContractError::Invalid(format!(
            "pip requirements lock contains a mutable dependency input: {requirement:?}"
        )));
    }
    let hashes = requirement
        .split_whitespace()
        .filter_map(|part| part.strip_prefix("--hash=sha256:"))
        .collect::<Vec<_>>();
    if hashes.is_empty() || hashes.iter().any(|hash| !is_sha256(hash)) {
        return Err(RepositoryContractError::Invalid(format!(
            "exact requirement is missing a valid SHA-256 hash: {requirement:?}"
        )));
    }
    Ok(())
}

fn validate_relative_path(value: &str, allow_glob: bool) -> Result<(), RepositoryContractError> {
    if value.is_empty() || value.len() > 256 || value.contains('\\') {
        return Err(RepositoryContractError::Invalid(format!(
            "invalid repository-relative path {value:?}"
        )));
    }
    let plain = value.strip_suffix("/**").unwrap_or(value);
    if value.contains('*') && (!allow_glob || !value.ends_with("/**") || plain.contains('*')) {
        return Err(RepositoryContractError::Invalid(format!(
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
        return Err(RepositoryContractError::Invalid(format!(
            "unsafe repository-relative path {value:?}"
        )));
    }
    Ok(())
}

fn validate_command(command: &str) -> Result<(), RepositoryContractError> {
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
        return Err(RepositoryContractError::Invalid(format!(
            "acceptance command is not an exact offline command: {command:?}"
        )));
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), RepositoryContractError> {
    if value.is_empty()
        || value.len() > 64
        || !value.bytes().all(is_safe_identifier_byte)
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(RepositoryContractError::Invalid(format!(
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

fn validate_digest_ref(value: &str) -> Result<(), RepositoryContractError> {
    let Some((repository, digest)) = value.rsplit_once("@sha256:") else {
        return Err(RepositoryContractError::Invalid(
            "environment profile image must use repository@sha256:digest".into(),
        ));
    };
    if repository.is_empty() || !is_sha256(digest) {
        return Err(RepositoryContractError::Invalid(
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
        std::fs::write(root.join(LEGACY_PROJECT_CONTRACT_PATH), yaml).unwrap();
        root
    }

    #[test]
    fn loads_strict_hashed_contract() {
        let root = fixture();
        let loaded = RepositoryContract::load_with_metadata(&root).unwrap();
        assert_eq!(loaded.contract.environment_profile, "python-3.11");
        assert_eq!(loaded.content_sha256.len(), 64);
        assert_eq!(loaded.source, RepositoryContractSource::LegacyAlias);
        assert!(matches!(
            RepositoryContract::load_for_repo_mode(&root),
            Err(RepositoryContractError::CanonicalRequired)
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn repo_mode_requires_canonical_contract_and_reports_matching_alias() {
        let root = fixture();
        let alias = root.join(LEGACY_PROJECT_CONTRACT_PATH);
        let canonical = root.join(REPOSITORY_CONTRACT_PATH);
        std::fs::copy(&alias, &canonical).unwrap();

        let loaded = RepositoryContract::load_for_repo_mode(&root).unwrap();
        assert_eq!(
            loaded.source,
            RepositoryContractSource::CanonicalWithMatchingAlias
        );
        assert_eq!(loaded.active_path, REPOSITORY_CONTRACT_PATH);
        assert_eq!(loaded.warnings.len(), 1);

        std::fs::remove_file(alias).unwrap();
        let loaded = RepositoryContract::load_for_repo_mode(&root).unwrap();
        assert_eq!(loaded.source, RepositoryContractSource::Canonical);
        assert!(loaded.warnings.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conflicting_canonical_and_alias_contracts_block_loading() {
        let root = fixture();
        let canonical = root.join(REPOSITORY_CONTRACT_PATH);
        let mut yaml = std::fs::read_to_string(root.join(LEGACY_PROJECT_CONTRACT_PATH)).unwrap();
        yaml.push_str("# canonical formatting differs\n");
        std::fs::write(canonical, yaml).unwrap();
        assert!(matches!(
            RepositoryContract::load_with_metadata(&root),
            Err(RepositoryContractError::ConflictingAliases)
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_fields_and_lock_drift() {
        let root = fixture();
        let path = root.join(LEGACY_PROJECT_CONTRACT_PATH);
        let mut yaml = std::fs::read_to_string(&path).unwrap();
        yaml.push_str("surprise: true\n");
        std::fs::write(&path, yaml).unwrap();
        assert!(matches!(
            RepositoryContract::load(&root),
            Err(RepositoryContractError::InvalidYaml(_))
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
