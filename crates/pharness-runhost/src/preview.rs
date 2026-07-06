//! Approval preview generation for file-write actions.
//!
//! Previews read the run workspace, so they must be computed by the process
//! that owns the workspace filesystem: the worker attempt, not the API.

use pharness_core::{simple_text_diff, AgentAction, TextPatch};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_APPROVAL_PREVIEW_DIFF_BYTES: usize = 64 * 1024;

pub fn approval_preview_for_action(
    cwd: &str,
    action: Option<&AgentAction>,
) -> Option<serde_json::Value> {
    match action? {
        AgentAction::WriteFile { path, content, .. } => {
            Some(write_file_approval_preview(cwd, path.as_str(), content))
        }
        AgentAction::PatchFile { path, patch, .. } => {
            Some(patch_file_approval_preview(cwd, path.as_str(), patch))
        }
        _ => None,
    }
}

fn write_file_approval_preview(cwd: &str, path: &str, content: &str) -> serde_json::Value {
    if is_secret_shaped_path(path) {
        return approval_preview_error(
            "write_file",
            path,
            "preview skipped for secret-shaped path",
        );
    }

    let target = match resolve_preview_write_path(cwd, path) {
        Ok(target) => target,
        Err(error) => return approval_preview_error("write_file", path, error),
    };
    let existed = target.path.exists();
    let before = if existed {
        match fs::read_to_string(&target.path) {
            Ok(content) => Some(content),
            Err(error) => {
                return approval_preview_error(
                    "write_file",
                    path,
                    format!("failed to read existing file for preview: {error}"),
                );
            }
        }
    } else {
        None
    };
    let (diff, diff_truncated) = bounded_preview_diff(before.as_deref(), content);

    serde_json::json!({
        "kind": "file_write",
        "action": "write_file",
        "path": target.display_path(),
        "status": "ok",
        "existed": existed,
        "before_bytes": before.as_ref().map(|value| value.len()),
        "after_bytes": content.len(),
        "diff": diff,
        "diff_truncated": diff_truncated
    })
}

fn patch_file_approval_preview(cwd: &str, path: &str, patch: &TextPatch) -> serde_json::Value {
    if is_secret_shaped_path(path) {
        return approval_preview_error(
            "patch_file",
            path,
            "preview skipped for secret-shaped path",
        );
    }
    if patch.find.is_empty() {
        return approval_preview_error("patch_file", path, "patch.find must not be empty");
    }

    let target = match resolve_preview_existing_path(cwd, path) {
        Ok(target) => target,
        Err(error) => return approval_preview_error("patch_file", path, error),
    };
    if !target.path.is_file() {
        return approval_preview_error("patch_file", path, "patch target is not a file");
    }

    let before = match fs::read_to_string(&target.path) {
        Ok(content) => content,
        Err(error) => {
            return approval_preview_error(
                "patch_file",
                path,
                format!("failed to read target file for preview: {error}"),
            );
        }
    };
    let matches = before.matches(&patch.find).count();
    if matches == 0 {
        return patch_preview_match_error(path, "patch.find did not match target file", matches);
    }
    if !patch.replace_all && matches != 1 {
        return patch_preview_match_error(
            path,
            format!(
                "patch.find matched {matches} times; set replace_all=true to preview execution"
            ),
            matches,
        );
    }

    let after = if patch.replace_all {
        before.replace(&patch.find, &patch.replace)
    } else {
        before.replacen(&patch.find, &patch.replace, 1)
    };
    let replacements = if patch.replace_all { matches } else { 1 };
    let (diff, diff_truncated) = bounded_preview_diff(Some(&before), &after);

    serde_json::json!({
        "kind": "file_write",
        "action": "patch_file",
        "path": target.display_path(),
        "status": "ok",
        "replacements": replacements,
        "replace_all": patch.replace_all,
        "before_bytes": before.len(),
        "after_bytes": after.len(),
        "diff": diff,
        "diff_truncated": diff_truncated
    })
}

fn patch_preview_match_error(
    path: &str,
    error: impl Into<String>,
    matches: usize,
) -> serde_json::Value {
    let mut preview = approval_preview_error("patch_file", path, error);
    if let Some(object) = preview.as_object_mut() {
        object.insert("matches".to_string(), serde_json::json!(matches));
    }
    preview
}

fn approval_preview_error(action: &str, path: &str, error: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "kind": "file_write",
        "action": action,
        "path": path,
        "status": "error",
        "error": error.into()
    })
}

fn bounded_preview_diff(before: Option<&str>, after: &str) -> (String, bool) {
    let diff = simple_text_diff(before, after);
    if diff.len() <= MAX_APPROVAL_PREVIEW_DIFF_BYTES {
        return (diff, false);
    }

    (truncate_utf8(&diff, MAX_APPROVAL_PREVIEW_DIFF_BYTES), true)
}

fn truncate_utf8(input: &str, max_bytes: usize) -> String {
    let end = input
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_bytes)
        .last()
        .unwrap_or(0);
    format!("{}\n[diff truncated]", &input[..end])
}

#[derive(Debug)]
struct PreviewPath {
    canonical_root: PathBuf,
    path: PathBuf,
}

impl PreviewPath {
    fn display_path(&self) -> String {
        self.path
            .strip_prefix(&self.canonical_root)
            .unwrap_or(&self.path)
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string()
    }
}

fn resolve_preview_write_path(cwd: &str, path: &str) -> Result<PreviewPath, String> {
    let canonical_root = canonical_workspace_root(cwd)?;
    let candidate = candidate_path(&canonical_root, path);
    let parent = candidate
        .parent()
        .ok_or_else(|| format!("write path has no parent: {path}"))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize parent for {path}: {error}"))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("path resolves outside the workspace".to_string());
    }

    Ok(PreviewPath {
        canonical_root,
        path: candidate,
    })
}

fn resolve_preview_existing_path(cwd: &str, path: &str) -> Result<PreviewPath, String> {
    let canonical_root = canonical_workspace_root(cwd)?;
    let candidate = candidate_path(&canonical_root, path);
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {path}: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("path resolves outside the workspace".to_string());
    }

    Ok(PreviewPath {
        canonical_root,
        path: canonical,
    })
}

fn canonical_workspace_root(cwd: &str) -> Result<PathBuf, String> {
    Path::new(cwd)
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize workspace root: {error}"))
}

fn candidate_path(canonical_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        canonical_root.join(path)
    }
}

fn is_secret_shaped_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file_name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();

    file_name == ".env"
        || file_name.starts_with(".env.")
        || lower.contains("kubeconfig")
        || lower.contains("id_rsa")
        || lower.contains("id_ed25519")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
        || lower.ends_with(".key")
}

#[cfg(test)]
mod tests {
    use super::approval_preview_for_action;
    use pharness_core::{AgentAction, TextPatch};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn previews_write_file_approval_with_diff() {
        let temp = temp_dir("write-preview");
        fs::write(temp.join("README.md"), "old\n").unwrap();
        let action = AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "test".to_string(),
            path: "README.md".into(),
            content: "new\n".to_string(),
        };

        let preview = approval_preview_for_action(temp.to_str().unwrap(), Some(&action)).unwrap();

        assert_eq!(preview["status"], "ok");
        assert_eq!(preview["action"], "write_file");
        assert_eq!(preview["path"], "README.md");
        assert_eq!(preview["existed"], true);
        assert!(preview["diff"].as_str().unwrap().contains("-old"));
        assert!(preview["diff"].as_str().unwrap().contains("+new"));
    }

    #[test]
    fn previews_patch_file_approval_with_diff() {
        let temp = temp_dir("patch-preview");
        fs::write(temp.join("README.md"), "alpha\nbeta\n").unwrap();
        let action = AgentAction::PatchFile {
            id: "act_patch".into(),
            reason: "test".to_string(),
            path: "README.md".into(),
            patch: TextPatch {
                find: "beta".to_string(),
                replace: "gamma".to_string(),
                replace_all: false,
            },
        };

        let preview = approval_preview_for_action(temp.to_str().unwrap(), Some(&action)).unwrap();

        assert_eq!(preview["status"], "ok");
        assert_eq!(preview["action"], "patch_file");
        assert_eq!(preview["replacements"], 1);
        assert!(preview["diff"].as_str().unwrap().contains("-beta"));
        assert!(preview["diff"].as_str().unwrap().contains("+gamma"));
    }

    #[test]
    fn skips_secret_shaped_approval_preview() {
        let temp = temp_dir("secret-preview");
        let action = AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "test".to_string(),
            path: ".env".into(),
            content: "TOKEN=value\n".to_string(),
        };

        let preview = approval_preview_for_action(temp.to_str().unwrap(), Some(&action)).unwrap();

        assert_eq!(preview["status"], "error");
        assert_eq!(preview["action"], "write_file");
        assert!(preview["diff"].is_null());
        assert!(preview["error"]
            .as_str()
            .unwrap()
            .contains("secret-shaped path"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pharness-runhost-{name}-{suffix}"));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
