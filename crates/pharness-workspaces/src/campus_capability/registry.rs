//! The typed capability each Campus tool binds to.

use crate::campus_profile::CampusCapabilityScope;
use pharness_core::{CapabilityKind, ToolCapability};
use serde::{Deserialize, Serialize};

/// The shape of resource selector a tool's calls are checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusSelectorKind {
    AgentControl,
    Workspace,
    WorkspacePath,
    WorkspacePaths,
    WorkspaceCommand,
}

/// One registry entry: the Campus profile scope a tool runs in, the
/// [`ToolCapability`] it exercises and the selector its calls must declare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusToolBinding {
    pub tool: &'static str,
    pub scope: CampusCapabilityScope,
    pub capability: ToolCapability,
    pub selector: CampusSelectorKind,
}

const fn binding(
    tool: &'static str,
    scope: CampusCapabilityScope,
    capability: ToolCapability,
    selector: CampusSelectorKind,
) -> CampusToolBinding {
    CampusToolBinding {
        tool,
        scope,
        capability,
        selector,
    }
}

use CampusCapabilityScope::{AgentControl, SessionShell, SessionWorkspace};

/// Every tool a Campus profile may bind. It matches the profile tool catalog
/// exactly; a tool missing here cannot be brokered even if a profile binds it.
/// Cluster, registry, delivery and SDLC submission capabilities are absent.
pub const CAMPUS_CAPABILITY_REGISTRY: &[CampusToolBinding] = &[
    binding(
        "finish",
        AgentControl,
        ToolCapability::read_only(CapabilityKind::AgentControl),
        CampusSelectorKind::AgentControl,
    ),
    binding(
        "environment_info",
        SessionWorkspace,
        ToolCapability::read_only(CapabilityKind::Filesystem),
        CampusSelectorKind::Workspace,
    ),
    binding(
        "list_dir",
        SessionWorkspace,
        ToolCapability::read_only(CapabilityKind::Filesystem),
        CampusSelectorKind::WorkspacePath,
    ),
    binding(
        "read_file",
        SessionWorkspace,
        ToolCapability::read_only(CapabilityKind::Filesystem),
        CampusSelectorKind::WorkspacePath,
    ),
    binding(
        "search_files",
        SessionWorkspace,
        ToolCapability::read_only(CapabilityKind::Filesystem),
        CampusSelectorKind::WorkspacePath,
    ),
    binding(
        "create_directory",
        SessionWorkspace,
        ToolCapability::mutating(CapabilityKind::Filesystem, false),
        CampusSelectorKind::WorkspacePath,
    ),
    binding(
        "write_file",
        SessionWorkspace,
        ToolCapability::mutating(CapabilityKind::Filesystem, false),
        CampusSelectorKind::WorkspacePath,
    ),
    binding(
        "patch_file",
        SessionWorkspace,
        ToolCapability::mutating(CapabilityKind::Filesystem, false),
        CampusSelectorKind::WorkspacePath,
    ),
    binding(
        "apply_patch",
        SessionWorkspace,
        ToolCapability::mutating(CapabilityKind::Filesystem, false),
        CampusSelectorKind::WorkspacePaths,
    ),
    binding(
        "git_diff",
        SessionWorkspace,
        ToolCapability::read_only(CapabilityKind::Git),
        CampusSelectorKind::Workspace,
    ),
    binding(
        "git_status",
        SessionWorkspace,
        ToolCapability::read_only(CapabilityKind::Git),
        CampusSelectorKind::Workspace,
    ),
    binding(
        "run_workspace_command",
        SessionShell,
        ToolCapability::mutating(CapabilityKind::Shell, false),
        CampusSelectorKind::WorkspaceCommand,
    ),
];

pub fn campus_tool_binding(tool: &str) -> Option<&'static CampusToolBinding> {
    CAMPUS_CAPABILITY_REGISTRY
        .iter()
        .find(|binding| binding.tool == tool)
}
