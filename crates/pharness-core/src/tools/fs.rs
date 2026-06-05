use super::{ToolError, ToolExecutor, ToolResult};
use crate::{AgentAction, TextPatch};
use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
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

    fn list_dir(&self, path: &Utf8Path, depth: u8) -> Result<ToolResult, ToolError> {
        let root = self.resolve_existing(path)?;
        if !root.is_dir() {
            return Err(ToolError::NotDirectory {
                path: path.to_string(),
            });
        }

        let mut entries = Vec::new();
        self.collect_dir_entries(&root, depth, &mut entries)?;
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(ToolResult::ok(
            format!("listed {} entries", entries.len()),
            serde_json::to_value(entries).expect("directory entries should serialize"),
        ))
    }

    fn collect_dir_entries(
        &self,
        dir: &Path,
        depth: u8,
        entries: &mut Vec<DirectoryEntry>,
    ) -> Result<(), ToolError> {
        for entry in fs::read_dir(dir).map_err(|error| ToolError::Io {
            message: format!("failed to read {}: {error}", dir.display()),
        })? {
            let entry = entry.map_err(|error| ToolError::Io {
                message: format!("failed to read directory entry: {error}"),
            })?;
            let file_type = entry.file_type().map_err(|error| ToolError::Io {
                message: format!("failed to read file type: {error}"),
            })?;
            let path = entry.path();

            entries.push(DirectoryEntry {
                path: self.display_path(&path),
                kind: if file_type.is_dir() { "dir" } else { "file" }.to_string(),
            });

            if depth > 0 && file_type.is_dir() {
                self.collect_dir_entries(&path, depth - 1, entries)?;
            }
        }

        Ok(())
    }

    fn read_file(&self, path: &Utf8Path, max_bytes: Option<u64>) -> Result<ToolResult, ToolError> {
        let resolved = self.resolve_existing(path)?;
        let bytes = fs::read(&resolved).map_err(|error| ToolError::Io {
            message: format!("failed to read {}: {error}", path.as_str()),
        })?;
        let max_bytes = max_bytes.unwrap_or(256 * 1024) as usize;
        let truncated = bytes.len() > max_bytes;
        let bytes = if truncated {
            &bytes[..max_bytes]
        } else {
            bytes.as_slice()
        };
        let content = std::str::from_utf8(bytes).map_err(|_| ToolError::NonUtf8 {
            path: path.to_string(),
        })?;

        Ok(ToolResult::ok(
            format!("read {} bytes from {}", bytes.len(), path.as_str()),
            serde_json::json!({
                "path": path.as_str(),
                "content": content,
                "truncated": truncated,
            }),
        ))
    }

    fn search_files(
        &self,
        query: &str,
        path: Option<&Utf8PathBuf>,
        glob: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        let root =
            self.resolve_existing(path.map(Utf8PathBuf::as_path).unwrap_or(Utf8Path::new(".")))?;
        let mut matches = Vec::new();
        self.search_path(query, glob, &root, &mut matches)?;

        Ok(ToolResult::ok(
            format!("found {} matches", matches.len()),
            serde_json::to_value(matches).expect("search matches should serialize"),
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

    fn search_path(
        &self,
        query: &str,
        glob: Option<&str>,
        path: &Path,
        matches: &mut Vec<SearchMatch>,
    ) -> Result<(), ToolError> {
        if matches.len() >= 100 {
            return Ok(());
        }

        if path.is_dir() {
            for entry in fs::read_dir(path).map_err(|error| ToolError::Io {
                message: format!("failed to read {}: {error}", path.display()),
            })? {
                let entry = entry.map_err(|error| ToolError::Io {
                    message: format!("failed to read directory entry: {error}"),
                })?;
                self.search_path(query, glob, &entry.path(), matches)?;
                if matches.len() >= 100 {
                    break;
                }
            }
            return Ok(());
        }

        if let Some(glob) = glob {
            let display_path = self.display_path(path);
            if !display_path.contains(glob.trim_matches('*')) {
                return Ok(());
            }
        }

        let Ok(content) = fs::read_to_string(path) else {
            return Ok(());
        };

        for (line_index, line) in content.lines().enumerate() {
            if line.contains(query) {
                matches.push(SearchMatch {
                    path: self.display_path(path),
                    line: line_index + 1,
                    snippet: line.trim().to_string(),
                });

                if matches.len() >= 100 {
                    break;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ToolExecutor for LocalReadOnlyFsTools {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        match action {
            AgentAction::ListDir { path, depth, .. } => self.list_dir(path, *depth),
            AgentAction::ReadFile {
                path, max_bytes, ..
            } => self.read_file(path, *max_bytes),
            AgentAction::WriteFile { path, content, .. } => self.write_file(path, content),
            AgentAction::PatchFile { path, patch, .. } => self.patch_file(path, patch),
            AgentAction::SearchFiles {
                query, path, glob, ..
            } => self.search_files(query, path.as_ref(), glob.as_deref()),
            other => Err(ToolError::UnsupportedAction {
                action: other.kind_name().to_string(),
            }),
        }
    }
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
            })
            .await
            .unwrap();

        assert_eq!(result.content["content"], "hello world");
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
            })
            .await
            .unwrap();

        assert_eq!(result.content[0]["path"], "src/lib.rs");
        assert_eq!(result.content[0]["line"], 1);
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
