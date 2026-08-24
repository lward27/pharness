use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use thiserror::Error;

pub const REPOSITORY_DISCOVERY_SCHEMA: &str = "pharness.dev/repository-discovery/v1alpha1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDiscoveryLimits {
    pub max_entries: usize,
    pub max_inspected_text_bytes: usize,
    pub max_text_file_bytes: usize,
}

impl Default for RepositoryDiscoveryLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_inspected_text_bytes: 32 * 1024 * 1024,
            max_text_file_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDiscoveryIdentity {
    pub provider: String,
    pub canonical_url: String,
    pub default_branch: String,
    pub registered_commit: String,
    pub resolved_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDiscovery {
    pub schema_version: String,
    pub repository: RepositoryDiscoveryIdentity,
    pub files: Vec<DiscoveredRepositoryEntry>,
    pub symlinks: Vec<DiscoveredSymlink>,
    pub submodules: Vec<DiscoveredSubmodule>,
    pub contract: DiscoveredContractState,
    pub language_indicators: BTreeMap<String, usize>,
    pub dependency_candidates: Vec<DiscoveredCandidate>,
    pub command_candidates: Vec<DiscoveredCommandCandidate>,
    pub root_candidates: Vec<String>,
    pub automation_references: Vec<String>,
    pub conflicts: Vec<DiscoveryFinding>,
    pub blockers: Vec<DiscoveryFinding>,
    pub inspected_text_bytes: usize,
    pub limits: RepositoryDiscoveryLimits,
    pub content_hash: String,
}

impl RepositoryDiscovery {
    pub fn verify_content_hash(&self) -> Result<(), RepositoryDiscoveryError> {
        let expected = self.content_hash.clone();
        let mut material = self.clone();
        material.content_hash.clear();
        let encoded = serde_json::to_vec(&material)
            .map_err(|error| RepositoryDiscoveryError::Serialization(error.to_string()))?;
        if sha256_prefixed(&encoded) != expected {
            return Err(RepositoryDiscoveryError::Serialization(
                "repository discovery content hash does not match its canonical payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredRepositoryEntry {
    pub path: String,
    pub kind: String,
    pub size_bytes: u64,
    pub inspected: bool,
    pub content_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredSymlink {
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredSubmodule {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredContractState {
    pub canonical_present: bool,
    pub canonical_sha256: Option<String>,
    pub alias_present: bool,
    pub alias_sha256: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredCandidate {
    pub kind: String,
    pub path: String,
    pub content_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredCommandCandidate {
    pub command: String,
    pub source_path: String,
    pub source_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryFinding {
    pub code: String,
    pub summary: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Error)]
pub enum RepositoryDiscoveryError {
    #[error("repository discovery root is unavailable: {0}")]
    Root(String),
    #[error("repository discovery path escapes the checkout: {0}")]
    PathEscape(String),
    #[error("repository discovery exceeded the {0} entry limit")]
    EntryLimit(usize),
    #[error("repository discovery exceeded the {0} byte inspected-text limit")]
    TextLimit(usize),
    #[error("repository discovery I/O failed for {path}: {error}")]
    Io { path: String, error: String },
    #[error("repository discovery serialization failed: {0}")]
    Serialization(String),
}

pub fn discover_repository(
    workspace: &Path,
    identity: RepositoryDiscoveryIdentity,
    limits: RepositoryDiscoveryLimits,
) -> Result<RepositoryDiscovery, RepositoryDiscoveryError> {
    let root = workspace
        .canonicalize()
        .map_err(|error| RepositoryDiscoveryError::Root(error.to_string()))?;
    let mut files = Vec::new();
    let mut symlinks = Vec::new();
    let mut language_indicators = BTreeMap::new();
    let mut dependency_candidates = Vec::new();
    let mut command_candidates = Vec::new();
    let mut roots = BTreeSet::new();
    let mut automation = BTreeSet::new();
    let mut blockers = Vec::new();
    let mut inspected_text_bytes = 0usize;
    let mut inspected_text = BTreeMap::<String, String>::new();

    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .build();
    for entry in walker {
        let entry = entry.map_err(|error| RepositoryDiscoveryError::Io {
            path: ".".into(),
            error: error.to_string(),
        })?;
        let path = entry.path();
        if path == root || path.starts_with(root.join(".git")) {
            continue;
        }
        if files.len() >= limits.max_entries {
            return Err(RepositoryDiscoveryError::EntryLimit(limits.max_entries));
        }
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| RepositoryDiscoveryError::PathEscape(path_text(path)))?;
        let relative = path_text(relative);
        let metadata =
            std::fs::symlink_metadata(path).map_err(|error| RepositoryDiscoveryError::Io {
                path: relative.clone(),
                error: error.to_string(),
            })?;
        if metadata.file_type().is_symlink() {
            let target =
                std::fs::read_link(path).map_err(|error| RepositoryDiscoveryError::Io {
                    path: relative.clone(),
                    error: error.to_string(),
                })?;
            symlinks.push(DiscoveredSymlink {
                path: relative.clone(),
                target: path_text(&target),
            });
            files.push(entry_record(
                relative,
                "symlink",
                metadata.len(),
                false,
                None,
            ));
            continue;
        }
        if metadata.is_dir() {
            if matches!(
                relative.as_str(),
                "src" | "tests" | "test" | "docs" | "documentation"
            ) {
                roots.insert(relative.clone());
            }
            files.push(entry_record(relative, "directory", 0, false, None));
            continue;
        }
        if !metadata.is_file() {
            files.push(entry_record(relative, "other", metadata.len(), false, None));
            continue;
        }

        record_language_indicator(&relative, &mut language_indicators);
        if relative.starts_with(".github/workflows/")
            || matches!(
                relative.as_str(),
                "Makefile" | "Taskfile.yml" | "Jenkinsfile"
            )
        {
            automation.insert(relative.clone());
        }
        let secret_shaped = is_secret_shaped(&relative);
        if secret_shaped {
            blockers.push(DiscoveryFinding {
                code: "secret_shaped_path".into(),
                summary: "secret-shaped repository path was not inspected".into(),
                paths: vec![relative.clone()],
            });
        }
        let can_inspect = !secret_shaped && metadata.len() as usize <= limits.max_text_file_bytes;
        let mut inspected = false;
        let mut content_hash = None;
        if can_inspect {
            let bytes = std::fs::read(path).map_err(|error| RepositoryDiscoveryError::Io {
                path: relative.clone(),
                error: error.to_string(),
            })?;
            if !looks_binary(&bytes) {
                inspected_text_bytes = inspected_text_bytes.saturating_add(bytes.len());
                if inspected_text_bytes > limits.max_inspected_text_bytes {
                    return Err(RepositoryDiscoveryError::TextLimit(
                        limits.max_inspected_text_bytes,
                    ));
                }
                content_hash = Some(sha256_prefixed(&bytes));
                inspected = true;
                if let Ok(text) = String::from_utf8(bytes) {
                    collect_command_candidates(&relative, &text, &mut command_candidates);
                    inspected_text.insert(relative.clone(), text);
                }
            }
        }
        if let Some(kind) = dependency_kind(&relative) {
            dependency_candidates.push(DiscoveredCandidate {
                kind: kind.into(),
                path: relative.clone(),
                content_sha256: content_hash.clone(),
            });
        }
        files.push(entry_record(
            relative,
            if inspected { "text" } else { "file" },
            metadata.len(),
            inspected,
            content_hash,
        ));
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    symlinks.sort_by(|a, b| a.path.cmp(&b.path));
    dependency_candidates.sort_by(|a, b| a.path.cmp(&b.path));
    command_candidates.sort_by(|a, b| {
        (&a.source_path, a.source_line, &a.command).cmp(&(
            &b.source_path,
            b.source_line,
            &b.command,
        ))
    });
    blockers.sort_by(|a, b| (&a.code, &a.paths).cmp(&(&b.code, &b.paths)));
    let contract = contract_state(&files);
    let conflicts = if contract.status == "conflicting" {
        vec![DiscoveryFinding {
            code: "repository_contract_conflict".into(),
            summary: "canonical and deprecated repository contracts differ".into(),
            paths: vec![
                ".pharness/repository.yaml".into(),
                ".pharness/project.yaml".into(),
            ],
        }]
    } else {
        Vec::new()
    };
    let mut discovery = RepositoryDiscovery {
        schema_version: REPOSITORY_DISCOVERY_SCHEMA.into(),
        repository: identity,
        files,
        symlinks,
        submodules: parse_submodules(inspected_text.get(".gitmodules")),
        contract,
        language_indicators,
        dependency_candidates,
        command_candidates,
        root_candidates: roots.into_iter().collect(),
        automation_references: automation.into_iter().collect(),
        conflicts,
        blockers,
        inspected_text_bytes,
        limits,
        content_hash: String::new(),
    };
    let encoded = serde_json::to_vec(&discovery)
        .map_err(|error| RepositoryDiscoveryError::Serialization(error.to_string()))?;
    discovery.content_hash = sha256_prefixed(&encoded);
    Ok(discovery)
}

fn entry_record(
    path: String,
    kind: &str,
    size_bytes: u64,
    inspected: bool,
    content_sha256: Option<String>,
) -> DiscoveredRepositoryEntry {
    DiscoveredRepositoryEntry {
        path,
        kind: kind.into(),
        size_bytes,
        inspected,
        content_sha256,
    }
}

fn contract_state(files: &[DiscoveredRepositoryEntry]) -> DiscoveredContractState {
    let canonical = files
        .iter()
        .find(|entry| entry.path == ".pharness/repository.yaml");
    let alias = files
        .iter()
        .find(|entry| entry.path == ".pharness/project.yaml");
    let status = match (canonical, alias) {
        (Some(canonical), Some(alias)) if canonical.content_sha256 == alias.content_sha256 => {
            "canonical_with_matching_alias"
        }
        (Some(_), Some(_)) => "conflicting",
        (Some(_), None) => "canonical",
        (None, Some(_)) => "legacy_alias",
        (None, None) => "missing",
    };
    DiscoveredContractState {
        canonical_present: canonical.is_some(),
        canonical_sha256: canonical.and_then(|entry| entry.content_sha256.clone()),
        alias_present: alias.is_some(),
        alias_sha256: alias.and_then(|entry| entry.content_sha256.clone()),
        status: status.into(),
    }
}

fn record_language_indicator(path: &str, indicators: &mut BTreeMap<String, usize>) {
    let indicator = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
            "rs" => Some("rust"),
            "py" => Some("python"),
            "js" | "jsx" | "ts" | "tsx" => Some("javascript_typescript"),
            "go" => Some("go"),
            "java" | "kt" => Some("jvm"),
            "rb" => Some("ruby"),
            _ => None,
        });
    if let Some(indicator) = indicator {
        *indicators.entry(indicator.into()).or_default() += 1;
    }
}

fn dependency_kind(path: &str) -> Option<&'static str> {
    match path {
        "requirements.lock" | "requirements.txt" => Some("pip_requirements"),
        "pyproject.toml" | "poetry.lock" | "uv.lock" => Some("python_project"),
        "Cargo.toml" | "Cargo.lock" => Some("cargo"),
        "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => Some("node"),
        "go.mod" | "go.sum" => Some("go"),
        _ => None,
    }
}

fn collect_command_candidates(
    path: &str,
    text: &str,
    candidates: &mut Vec<DiscoveredCommandCandidate>,
) {
    const MARKERS: &[&str] = &[
        "cargo test",
        "python -m unittest",
        "python3 -m unittest",
        "pytest",
        "npm test",
        "npm run test",
        "pnpm test",
        "go test",
        "make test",
        "compileall",
    ];
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.len() <= 256 && MARKERS.iter().any(|marker| trimmed.contains(marker)) {
            candidates.push(DiscoveredCommandCandidate {
                command: trimmed.into(),
                source_path: path.into(),
                source_line: index + 1,
            });
        }
    }
}

fn parse_submodules(contents: Option<&String>) -> Vec<DiscoveredSubmodule> {
    let mut paths = contents
        .into_iter()
        .flat_map(|contents| contents.lines())
        .filter_map(|line| line.trim().strip_prefix("path ="))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| DiscoveredSubmodule { path: path.into() })
        .collect::<Vec<_>>();
    paths.sort_by(|a, b| a.path.cmp(&b.path));
    paths
}

fn is_secret_shaped(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == ".env"
        || lower.ends_with("/.env")
        || lower.contains("credentials")
        || lower.contains("private_key")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.split('/').any(|part| part == "secrets")
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8_192).any(|byte| *byte == 0)
}

fn path_text(path: &Path) -> String {
    let body = path
        .components()
        .filter(|component| !matches!(component, std::path::Component::RootDir))
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if path.is_absolute() {
        format!("/{body}")
    } else {
        body
    }
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "pharness-repository-discovery-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".pharness")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("src/lib.py"), "print('ok')\n").unwrap();
        std::fs::write(
            root.join("readme.md"),
            "Run python -m unittest discover -s tests -v\n",
        )
        .unwrap();
        std::fs::write(
            root.join("requirements.lock"),
            "fastapi==1 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".pharness/project.yaml"),
            "api_version: pharness.dev/v1alpha1\n",
        )
        .unwrap();
        root
    }

    fn identity() -> RepositoryDiscoveryIdentity {
        RepositoryDiscoveryIdentity {
            provider: "github".into(),
            canonical_url: "https://github.com/example/repo.git".into(),
            default_branch: "main".into(),
            registered_commit: "a".repeat(40),
            resolved_commit: "a".repeat(40),
        }
    }

    #[test]
    fn discovery_is_sorted_hashed_and_reports_legacy_contract() {
        let root = fixture();
        let first = discover_repository(&root, identity(), Default::default()).unwrap();
        let second = discover_repository(&root, identity(), Default::default()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.contract.status, "legacy_alias");
        assert_eq!(first.language_indicators.get("python"), Some(&1));
        assert!(first
            .command_candidates
            .iter()
            .any(|candidate| candidate.command.contains("unittest")));
        assert!(first.content_hash.starts_with("sha256:"));
        first.verify_content_hash().unwrap();
        let paths = first
            .files
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.windows(2).all(|pair| pair[0] <= pair[1]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn discovery_does_not_follow_symlinks_or_inspect_secret_paths() {
        let root = fixture();
        std::fs::write(root.join(".env"), "TOKEN=secret\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", root.join("src/outside")).unwrap();
        let discovery = discover_repository(&root, identity(), Default::default()).unwrap();
        assert!(discovery
            .blockers
            .iter()
            .any(|finding| finding.code == "secret_shaped_path"));
        #[cfg(unix)]
        assert_eq!(discovery.symlinks[0].target, "/etc/passwd");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn discovery_enforces_entry_limit() {
        let root = fixture();
        let result = discover_repository(
            &root,
            identity(),
            RepositoryDiscoveryLimits {
                max_entries: 1,
                ..Default::default()
            },
        );
        assert!(matches!(
            result,
            Err(RepositoryDiscoveryError::EntryLimit(1))
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
