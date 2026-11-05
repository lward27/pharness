//! Resource selectors: the part of the session a call touches.

use super::registry::CampusSelectorKind;
use crate::AgentAction;
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_PATH_BYTES: usize = 1024;
const MAX_EXECUTABLE_BYTES: usize = 64;
const MAX_PATCH_PATHS: usize = 256;

/// What a call touches inside its own session. Paths are normalized and
/// relative to the session workspace root; `.` is the root itself.
///
/// The broker always derives the selector from the typed action. A caller
/// that declares one must declare exactly the derived selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CampusResourceSelector {
    AgentControl,
    Workspace,
    WorkspacePath { path: String },
    WorkspacePaths { paths: Vec<String> },
    WorkspaceCommand { cwd: String, executable: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusSelectorError {
    #[error("{0} is not a Campus capability")]
    NotACampusTool(String),
    #[error("path is outside the session workspace")]
    OutsideWorkspace,
    #[error("path is not a valid workspace path")]
    InvalidPath,
    #[error("patch must name between one and {MAX_PATCH_PATHS} workspace paths")]
    InvalidPatch,
    #[error("executable must be a bare program name")]
    InvalidExecutable,
}

impl CampusResourceSelector {
    pub fn kind(&self) -> CampusSelectorKind {
        match self {
            Self::AgentControl => CampusSelectorKind::AgentControl,
            Self::Workspace => CampusSelectorKind::Workspace,
            Self::WorkspacePath { .. } => CampusSelectorKind::WorkspacePath,
            Self::WorkspacePaths { .. } => CampusSelectorKind::WorkspacePaths,
            Self::WorkspaceCommand { .. } => CampusSelectorKind::WorkspaceCommand,
        }
    }

    /// Derive the selector for `action`, refusing any path that is absolute,
    /// climbs out of the workspace or is otherwise not a plain relative path.
    /// Symlinks are the executor's to resolve; this check is lexical.
    pub fn for_action(action: &AgentAction) -> Result<Self, CampusSelectorError> {
        let path = |path: &Utf8PathBuf| {
            workspace_path(path.as_str()).map(|path| Self::WorkspacePath { path })
        };
        match action {
            AgentAction::Finish { .. } => Ok(Self::AgentControl),
            AgentAction::EnvironmentInfo { .. } | AgentAction::GitStatus { .. } => {
                Ok(Self::Workspace)
            }
            AgentAction::GitDiff { pathspec, .. } => {
                if let Some(pathspec) = pathspec {
                    if pathspec.starts_with('-') {
                        return Err(CampusSelectorError::InvalidPath);
                    }
                    workspace_path(pathspec)?;
                }
                Ok(Self::Workspace)
            }
            AgentAction::ListDir { path: target, .. }
            | AgentAction::ReadFile { path: target, .. }
            | AgentAction::WriteFile { path: target, .. }
            | AgentAction::PatchFile { path: target, .. }
            | AgentAction::CreateDirectory { path: target, .. } => path(target),
            AgentAction::SearchFiles {
                path: target, glob, ..
            } => {
                if let Some(glob) = glob {
                    workspace_path(glob)?;
                }
                match target {
                    Some(target) => path(target),
                    None => Ok(Self::WorkspacePath { path: ".".into() }),
                }
            }
            AgentAction::ApplyPatch {
                patch,
                preimage_sha256,
                ..
            } => {
                let paths = patch_paths(patch)?;
                for key in preimage_sha256.keys() {
                    if !paths.contains(&workspace_path(key)?) {
                        return Err(CampusSelectorError::InvalidPatch);
                    }
                }
                Ok(Self::WorkspacePaths {
                    paths: paths.into_iter().collect(),
                })
            }
            AgentAction::RunWorkspaceCommand {
                executable, cwd, ..
            } => Ok(Self::WorkspaceCommand {
                cwd: workspace_path(cwd.as_ref().map_or(".", |cwd| cwd.as_str()))?,
                executable: bare_executable(executable)?,
            }),
            other => Err(CampusSelectorError::NotACampusTool(
                other.kind_name().to_string(),
            )),
        }
    }
}

/// Normalize a workspace-relative path. Empty and `.` segments are dropped;
/// absolute paths, home-relative paths, `..`, backslashes and control
/// characters are refused rather than repaired.
pub(super) fn workspace_path(raw: &str) -> Result<String, CampusSelectorError> {
    if raw.len() > MAX_PATH_BYTES
        || raw.contains('\\')
        || raw.chars().any(|character| character.is_control())
    {
        return Err(CampusSelectorError::InvalidPath);
    }
    if raw.starts_with('/') || raw.starts_with('~') {
        return Err(CampusSelectorError::OutsideWorkspace);
    }
    let mut segments = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => return Err(CampusSelectorError::OutsideWorkspace),
            segment => segments.push(segment),
        }
    }
    Ok(if segments.is_empty() {
        ".".into()
    } else {
        segments.join("/")
    })
}

fn bare_executable(executable: &str) -> Result<String, CampusSelectorError> {
    let valid = (1..=MAX_EXECUTABLE_BYTES).contains(&executable.len())
        && !executable.starts_with(['-', '.'])
        && executable
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'));
    if valid {
        Ok(executable.into())
    } else {
        Err(CampusSelectorError::InvalidExecutable)
    }
}

/// Every path a unified diff names in a header line. Hunk lines that happen
/// to begin with `--- ` or `+++ ` are also read as headers; that can only
/// add paths to check, so it fails closed.
fn patch_paths(patch: &str) -> Result<BTreeSet<String>, CampusSelectorError> {
    let mut paths = BTreeSet::new();
    for line in patch.lines() {
        let named: Vec<&str> = if let Some(rest) = line.strip_prefix("diff --git ") {
            rest.split_whitespace().collect()
        } else if let Some(rest) = line
            .strip_prefix("--- ")
            .or_else(|| line.strip_prefix("+++ "))
        {
            vec![rest.split('\t').next().unwrap_or_default().trim_end()]
        } else if let Some(rest) = ["rename from ", "rename to ", "copy from ", "copy to "]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix))
        {
            vec![rest]
        } else {
            continue;
        };
        for name in named {
            if name == "/dev/null" {
                continue;
            }
            let name = name
                .strip_prefix("a/")
                .or_else(|| name.strip_prefix("b/"))
                .unwrap_or(name);
            let path = workspace_path(name)?;
            if path == "." {
                return Err(CampusSelectorError::InvalidPatch);
            }
            paths.insert(path);
        }
    }
    if paths.is_empty() || paths.len() > MAX_PATCH_PATHS {
        return Err(CampusSelectorError::InvalidPatch);
    }
    Ok(paths)
}
