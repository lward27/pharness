use super::{ToolError, ToolExecutor, ToolResult};
use crate::{AgentAction, TextPatch};
use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
use ignore::WalkBuilder;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalReadOnlyFsTools {
    workspace_root: PathBuf,
    canonical_root: PathBuf,
}

impl LocalReadOnlyFsTools {
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let canonical_root = workspace_root
            .canonicalize()
            .map_err(|error| ToolError::Io {
                message: format!("failed to canonicalize workspace root: {error}"),
            })?;

        Ok(Self {
            workspace_root,
            canonical_root,
        })
    }

    fn resolve_existing(&self, path: &Utf8Path) -> Result<PathBuf, ToolError> {
        let candidate = if path.as_str().is_empty() || path.as_str() == "." {
            self.workspace_root.clone()
        } else if path.is_absolute() {
            PathBuf::from(path.as_str())
        } else {
            self.workspace_root.join(path.as_str())
        };

        let canonical = candidate.canonicalize().map_err(|error| ToolError::Io {
            message: format!("failed to canonicalize {}: {error}", path.as_str()),
        })?;

        if !canonical.starts_with(&self.canonical_root) {
            return Err(ToolError::OutsideWorkspace {
                path: path.to_string(),
            });
        }

        Ok(canonical)
    }

    fn resolve_for_write(&self, path: &Utf8Path) -> Result<PathBuf, ToolError> {
        if path.is_absolute() {
            let parent = PathBuf::from(path.as_str())
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| ToolError::Io {
                    message: format!("write path has no parent: {}", path.as_str()),
                })?;
            let canonical_parent = parent.canonicalize().map_err(|error| ToolError::Io {
                message: format!(
                    "failed to canonicalize parent for {}: {error}",
                    path.as_str()
                ),
            })?;
            if !canonical_parent.starts_with(&self.canonical_root) {
                return Err(ToolError::OutsideWorkspace {
                    path: path.to_string(),
                });
            }
            return Ok(PathBuf::from(path.as_str()));
        }

        let candidate = self.workspace_root.join(path.as_str());
        let parent = candidate.parent().ok_or_else(|| ToolError::Io {
            message: format!("write path has no parent: {}", path.as_str()),
        })?;

        let canonical_parent = parent.canonicalize().map_err(|error| ToolError::Io {
            message: format!(
                "failed to canonicalize parent for {}: {error}",
                path.as_str()
            ),
        })?;

        if !canonical_parent.starts_with(&self.canonical_root) {
            return Err(ToolError::OutsideWorkspace {
                path: path.to_string(),
            });
        }

        Ok(candidate)
    }

    fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.canonical_root)
            .unwrap_or(path)
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string()
    }

    fn list_dir(
        &self,
        path: &Utf8Path,
        depth: u8,
        max_entries: Option<u32>,
    ) -> Result<ToolResult, ToolError> {
        let root = self.resolve_existing(path)?;
        if !root.is_dir() {
            return Err(ToolError::NotDirectory {
                path: path.to_string(),
            });
        }

        let max_entries = max_entries.unwrap_or(500).clamp(1, 2_000) as usize;
        let mut entries = Vec::new();
        let mut truncated = false;
        let mut builder = WalkBuilder::new(&root);
        builder
            .standard_filters(true)
            .hidden(false)
            .git_ignore(true)
            .max_depth(Some(depth as usize + 1));
        builder.add_custom_ignore_filename(".gitignore");
        for entry in builder.build() {
            let entry = entry.map_err(|error| ToolError::Io {
                message: error.to_string(),
            })?;
            let path = entry.path();
            if path == root || should_ignore_path(path) {
                continue;
            }
            if entries.len() >= max_entries {
                truncated = true;
                break;
            }
            let file_type = entry.file_type().ok_or_else(|| ToolError::Io {
                message: format!("failed to read file type for {}", path.display()),
            })?;
            entries.push(DirectoryEntry {
                path: self.display_path(path),
                kind: if file_type.is_dir() { "dir" } else { "file" }.to_string(),
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(ToolResult::ok(
            format!("listed {} entries", entries.len()),
            serde_json::json!({ "entries": entries, "truncated": truncated }),
        ))
    }

    fn read_file(
        &self,
        path: &Utf8Path,
        max_bytes: Option<u64>,
        start_line: Option<u64>,
        line_count: Option<u64>,
    ) -> Result<ToolResult, ToolError> {
        let resolved = self.resolve_existing(path)?;
        let content = fs::read_to_string(&resolved).map_err(|error| ToolError::Io {
            message: format!("failed to read {}: {error}", path.as_str()),
        })?;
        let start_line = start_line.unwrap_or(1).max(1) as usize;
        let line_count = line_count.unwrap_or(400).clamp(1, 2_000) as usize;
        let max_bytes = max_bytes.unwrap_or(64 * 1024).clamp(1, 128 * 1024) as usize;
        let total_lines = content.lines().count();
        let selected = content
            .lines()
            .skip(start_line.saturating_sub(1))
            .take(line_count)
            .collect::<Vec<_>>()
            .join("\n");
        let truncated_before = start_line > 1;
        let truncated_after_lines = total_lines > start_line.saturating_sub(1) + line_count;
        let (selected, truncated_after_bytes) = truncate_utf8(&selected, max_bytes);

        Ok(ToolResult::ok(
            format!("read {} bytes from {}", selected.len(), path.as_str()),
            serde_json::json!({
                "path": path.as_str(),
                "content": selected,
                "total_lines": total_lines,
                "start_line": start_line,
                "line_count": line_count,
                "truncated_before": truncated_before,
                "truncated_after": truncated_after_lines || truncated_after_bytes,
            }),
        ))
    }

    fn search_files(
        &self,
        query: &str,
        path: Option<&Utf8PathBuf>,
        glob: Option<&str>,
        max_results: Option<u32>,
    ) -> Result<ToolResult, ToolError> {
        let root =
            self.resolve_existing(path.map(Utf8PathBuf::as_path).unwrap_or(Utf8Path::new(".")))?;
        // A directly selected file is already scope-checked above. Permit it
        // even when an ancestor is normally ignored; broad searches continue
        // to skip generated and metadata trees.
        let explicitly_selected_file = path.is_some() && root.is_file();
        let max_results = max_results.unwrap_or(50).clamp(1, 200) as usize;
        let mut matches = Vec::new();
        let mut truncated = false;
        let mut builder = WalkBuilder::new(&root);
        builder
            .standard_filters(true)
            .hidden(false)
            .git_ignore(true)
            .max_depth(None);
        builder.add_custom_ignore_filename(".gitignore");
        for entry in builder.build() {
            let entry = entry.map_err(|error| ToolError::Io {
                message: error.to_string(),
            })?;
            let candidate = entry.path();
            if (candidate == root && root.is_dir())
                || candidate.is_dir()
                || (should_ignore_path(candidate) && !explicitly_selected_file)
            {
                continue;
            }
            if let Some(glob) = glob {
                let display_path = self.display_path(candidate);
                if !display_path.contains(glob.trim_matches('*')) {
                    continue;
                }
            }
            let Ok(content) = fs::read_to_string(candidate) else {
                continue;
            };
            for (line_index, line) in content.lines().enumerate() {
                if line.contains(query) {
                    if matches.len() >= max_results {
                        truncated = true;
                        break;
                    }
                    matches.push(SearchMatch {
                        path: self.display_path(candidate),
                        line: line_index + 1,
                        snippet: line.trim().to_string(),
                    });
                }
            }
            if truncated {
                break;
            }
        }

        Ok(ToolResult::ok(
            format!("found {} matches", matches.len()),
            serde_json::json!({ "matches": matches, "truncated": truncated }),
        ))
    }

    fn write_file(&self, path: &Utf8Path, content: &str) -> Result<ToolResult, ToolError> {
        let destination = self.resolve_for_write(path)?;
        let before = fs::read_to_string(&destination).ok();
        self.replace_file(path, &destination, content)?;

        Ok(ToolResult::ok(
            format!("wrote {} bytes to {}", content.len(), path.as_str()),
            serde_json::json!({
                "path": path.as_str(),
                "bytes": content.len(),
                "existed": before.is_some(),
                "diff": simple_text_diff(before.as_deref(), content),
            }),
        ))
    }

    fn patch_file(&self, path: &Utf8Path, patch: &TextPatch) -> Result<ToolResult, ToolError> {
        if patch.find.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "patch.find must not be empty".to_string(),
            });
        }

        let destination = self.resolve_existing(path)?;
        if !destination.is_file() {
            return Err(ToolError::InvalidArguments {
                message: format!("patch target is not a file: {}", path.as_str()),
            });
        }

        let before = fs::read_to_string(&destination).map_err(|error| ToolError::Io {
            message: format!("failed to read {}: {error}", path.as_str()),
        })?;
        let replacements = before.matches(&patch.find).count();
        if replacements == 0 {
            return Err(ToolError::InvalidArguments {
                message: "patch.find did not match target file".to_string(),
            });
        }
        if !patch.replace_all && replacements != 1 {
            return Err(ToolError::InvalidArguments {
                message: format!(
                    "patch.find matched {replacements} times; set replace_all=true to replace every match"
                ),
            });
        }

        let after = if patch.replace_all {
            before.replace(&patch.find, &patch.replace)
        } else {
            before.replacen(&patch.find, &patch.replace, 1)
        };
        self.replace_file(path, &destination, &after)?;

        Ok(ToolResult::ok(
            format!(
                "patched {} replacement{} in {}",
                if patch.replace_all { replacements } else { 1 },
                if patch.replace_all && replacements != 1 {
                    "s"
                } else {
                    ""
                },
                path.as_str()
            ),
            serde_json::json!({
                "path": path.as_str(),
                "bytes": after.len(),
                "replacements": if patch.replace_all { replacements } else { 1 },
                "diff": simple_text_diff(Some(&before), &after),
            }),
        ))
    }

    fn replace_file(
        &self,
        path: &Utf8Path,
        destination: &Path,
        content: &str,
    ) -> Result<(), ToolError> {
        let temp_path = destination.with_extension(format!(
            "{}.pharness-tmp",
            destination
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("tmp")
        ));

        {
            let mut file = fs::File::create(&temp_path).map_err(|error| ToolError::Io {
                message: format!("failed to create temp file for {}: {error}", path.as_str()),
            })?;
            file.write_all(content.as_bytes())
                .map_err(|error| ToolError::Io {
                    message: format!("failed to write temp file for {}: {error}", path.as_str()),
                })?;
            file.sync_all().map_err(|error| ToolError::Io {
                message: format!("failed to sync temp file for {}: {error}", path.as_str()),
            })?;
        }

        fs::rename(&temp_path, destination).map_err(|error| ToolError::Io {
            message: format!("failed to replace {}: {error}", path.as_str()),
        })
    }
}

#[async_trait]
impl ToolExecutor for LocalReadOnlyFsTools {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        match action {
            AgentAction::ListDir {
                path,
                depth,
                max_entries,
                ..
            } => self.list_dir(path, *depth, *max_entries),
            AgentAction::ReadFile {
                path,
                max_bytes,
                start_line,
                line_count,
                ..
            } => self.read_file(path, *max_bytes, *start_line, *line_count),
            AgentAction::WriteFile { path, content, .. } => self.write_file(path, content),
            AgentAction::PatchFile { path, patch, .. } => self.patch_file(path, patch),
            AgentAction::SearchFiles {
                query,
                path,
                glob,
                max_results,
                ..
            } => self.search_files(query, path.as_ref(), glob.as_deref(), *max_results),
            other => Err(ToolError::UnsupportedAction {
                action: other.kind_name().to_string(),
            }),
        }
    }
}

fn should_ignore_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | "target" | "node_modules" | ".pharness")
        )
    })
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

pub fn simple_text_diff(before: Option<&str>, after: &str) -> String {
    match before {
        Some(before) if before == after => "unchanged".to_string(),
        Some(before) => format!(
            "--- before\n+++ after\n-{}\n+{}",
            before.replace('\n', "\n-"),
            after.replace('\n', "\n+")
        ),
        None => format!("--- before\n+++ after\n+{}", after.replace('\n', "\n+")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DirectoryEntry {
    path: String,
    kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SearchMatch {
    path: String,
    line: usize,
    snippet: String,
}

#[cfg(test)]
mod tests {
    use super::LocalReadOnlyFsTools;
    use crate::{AgentAction, ToolExecutor};
    use camino::Utf8PathBuf;
    use std::fs;

    #[tokio::test]
    async fn reads_files_inside_workspace() {
        let temp = unique_temp_dir("read");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("hello.txt"), "hello world").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let result = tools
            .execute(&AgentAction::ReadFile {
                id: "act_read".into(),
                reason: "read".to_string(),
                path: Utf8PathBuf::from("hello.txt"),
                max_bytes: None,
                start_line: None,
                line_count: None,
            })
            .await
            .unwrap();

        assert_eq!(result.content["content"], "hello world");
    }

    #[tokio::test]
    async fn reads_requested_utf8_line_range_with_boundaries() {
        let temp = unique_temp_dir("ranged-read");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("lines.txt"), "one\ntwø\nthree\nfour\n").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let result = tools
            .execute(&AgentAction::ReadFile {
                id: "act_read".into(),
                reason: "read range".to_string(),
                path: Utf8PathBuf::from("lines.txt"),
                max_bytes: Some(64),
                start_line: Some(2),
                line_count: Some(2),
            })
            .await
            .unwrap();

        assert_eq!(result.content["content"], "twø\nthree");
        assert_eq!(result.content["total_lines"], 4);
        assert_eq!(result.content["start_line"], 2);
        assert_eq!(result.content["truncated_before"], true);
        assert_eq!(result.content["truncated_after"], true);
    }

    #[tokio::test]
    async fn rejects_paths_outside_workspace() {
        let temp = unique_temp_dir("outside");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();

        let error = tools
            .execute(&AgentAction::ReadFile {
                id: "act_read".into(),
                reason: "read".to_string(),
                path: Utf8PathBuf::from("../outside.txt"),
                max_bytes: None,
                start_line: None,
                line_count: None,
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("I/O error"));
    }

    #[tokio::test]
    async fn searches_text_files() {
        let temp = unique_temp_dir("search");
        fs::create_dir_all(temp.join("src")).unwrap();
        fs::write(temp.join("src/lib.rs"), "fn target() {}\n").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let result = tools
            .execute(&AgentAction::SearchFiles {
                id: "act_search".into(),
                reason: "search".to_string(),
                query: "target".to_string(),
                path: Some(Utf8PathBuf::from(".")),
                glob: None,
                max_results: None,
            })
            .await
            .unwrap();

        assert_eq!(result.content["matches"][0]["path"], "src/lib.rs");
        assert_eq!(result.content["matches"][0]["line"], 1);
    }

    #[tokio::test]
    async fn search_and_directory_listing_apply_caps_and_ignore_rules() {
        let temp = unique_temp_dir("ignore-and-caps");
        fs::create_dir_all(temp.join("src")).unwrap();
        fs::create_dir_all(temp.join("ignored")).unwrap();
        fs::create_dir_all(temp.join("target")).unwrap();
        fs::write(temp.join(".gitignore"), "ignored/\n").unwrap();
        fs::write(temp.join("src/one.rs"), "needle\n").unwrap();
        fs::write(temp.join("src/two.rs"), "needle\n").unwrap();
        fs::write(temp.join("ignored/hidden.rs"), "needle\n").unwrap();
        fs::write(temp.join("target/build.rs"), "needle\n").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let search = tools
            .execute(&AgentAction::SearchFiles {
                id: "act_search".into(),
                reason: "search".to_string(),
                query: "needle".to_string(),
                path: Some(Utf8PathBuf::from(".")),
                glob: None,
                max_results: Some(1),
            })
            .await
            .unwrap();
        assert_eq!(search.content["matches"].as_array().unwrap().len(), 1);
        assert_eq!(search.content["truncated"], true);

        let listing = tools
            .execute(&AgentAction::ListDir {
                id: "act_list".into(),
                reason: "list".to_string(),
                path: Utf8PathBuf::from("."),
                depth: 2,
                max_entries: Some(2),
            })
            .await
            .unwrap();
        assert_eq!(listing.content["truncated"], true);
        let paths = listing.content["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["path"].as_str())
            .collect::<Vec<_>>();
        assert!(!paths.iter().any(|path| path.starts_with("target")));
        assert!(!paths.iter().any(|path| path.starts_with("ignored")));
    }

    #[tokio::test]
    async fn search_can_use_an_explicit_safe_file_inside_an_ignored_directory() {
        let temp = unique_temp_dir("explicit-ignored-file");
        fs::create_dir_all(temp.join("node_modules")).unwrap();
        fs::write(temp.join("node_modules/package.txt"), "needle\n").unwrap();
        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();

        let result = tools
            .execute(&AgentAction::SearchFiles {
                id: "act_search".into(),
                reason: "inspect an explicitly selected file".to_string(),
                query: "needle".to_string(),
                path: Some(Utf8PathBuf::from("node_modules/package.txt")),
                glob: None,
                max_results: None,
            })
            .await
            .unwrap();

        assert_eq!(
            result.content["matches"][0]["path"],
            "node_modules/package.txt"
        );
        let _ = fs::remove_dir_all(temp);
    }

    #[tokio::test]
    async fn writes_files_inside_workspace() {
        let temp = unique_temp_dir("write");
        fs::create_dir_all(&temp).unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let result = tools
            .execute(&AgentAction::WriteFile {
                id: "act_write".into(),
                reason: "write".to_string(),
                path: Utf8PathBuf::from("hello.txt"),
                content: "hello world".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(temp.join("hello.txt")).unwrap(),
            "hello world"
        );
        assert_eq!(result.content["existed"], false);
    }

    #[tokio::test]
    async fn patches_files_inside_workspace() {
        let temp = unique_temp_dir("patch");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("hello.txt"), "hello old world\n").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let result = tools
            .execute(&AgentAction::PatchFile {
                id: "act_patch".into(),
                reason: "patch".to_string(),
                path: Utf8PathBuf::from("hello.txt"),
                patch: crate::TextPatch {
                    find: "old".to_string(),
                    replace: "new".to_string(),
                    replace_all: false,
                },
            })
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(temp.join("hello.txt")).unwrap(),
            "hello new world\n"
        );
        assert_eq!(result.content["replacements"], 1);
        assert!(result.content["diff"]
            .as_str()
            .unwrap()
            .contains("+hello new"));
    }

    #[tokio::test]
    async fn patch_rejects_ambiguous_single_replacement() {
        let temp = unique_temp_dir("patch-ambiguous");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("hello.txt"), "same same\n").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let error = tools
            .execute(&AgentAction::PatchFile {
                id: "act_patch".into(),
                reason: "patch".to_string(),
                path: Utf8PathBuf::from("hello.txt"),
                patch: crate::TextPatch {
                    find: "same".to_string(),
                    replace: "changed".to_string(),
                    replace_all: false,
                },
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("matched 2 times"));
        assert_eq!(
            fs::read_to_string(temp.join("hello.txt")).unwrap(),
            "same same\n"
        );
    }

    #[tokio::test]
    async fn patch_can_replace_all_matches() {
        let temp = unique_temp_dir("patch-all");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("hello.txt"), "same same\n").unwrap();

        let tools = LocalReadOnlyFsTools::new(&temp).unwrap();
        let result = tools
            .execute(&AgentAction::PatchFile {
                id: "act_patch".into(),
                reason: "patch".to_string(),
                path: Utf8PathBuf::from("hello.txt"),
                patch: crate::TextPatch {
                    find: "same".to_string(),
                    replace: "changed".to_string(),
                    replace_all: true,
                },
            })
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(temp.join("hello.txt")).unwrap(),
            "changed changed\n"
        );
        assert_eq!(result.content["replacements"], 2);
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "pharness-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
