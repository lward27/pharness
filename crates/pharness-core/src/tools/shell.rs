use super::{ToolError, ToolExecutor, ToolResult};
use crate::{classify_command, AgentAction, CommandClass};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct LocalShellTools {
    workspace_root: PathBuf,
    canonical_root: PathBuf,
    shell_bin: String,
    timeout_ms: u64,
    max_output_bytes: usize,
}

impl LocalShellTools {
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
            shell_bin: std::env::var("PHARNESS_SHELL_BIN").unwrap_or_else(|_| "sh".to_string()),
            timeout_ms: std::env::var("PHARNESS_SHELL_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            max_output_bytes: std::env::var("PHARNESS_SHELL_MAX_OUTPUT_BYTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES),
        })
    }

    async fn run_shell(
        &self,
        cmd: &str,
        cwd: Option<&Utf8PathBuf>,
        timeout_ms: Option<u64>,
        dry_run: bool,
    ) -> Result<ToolResult, ToolError> {
        self.reject_never_execute_command(cmd)?;
        let cwd = self.resolve_cwd(cwd)?;

        if dry_run {
            return Ok(ToolResult::ok(
                "dry-run shell command",
                serde_json::json!({
                    "cmd": cmd,
                    "cwd": self.display_path(&cwd),
                    "dry_run": true,
                }),
            ));
        }

        let args = vec!["-lc".to_string(), cmd.to_string()];
        let output = self
            .run_program(&self.shell_bin, &args, &cwd, timeout_ms)
            .await?;
        let summary = if output.success {
            "shell command completed"
        } else {
            "shell command exited non-zero"
        };

        Ok(output.into_tool_result(
            summary,
            serde_json::json!({
                "cmd": cmd,
                "cwd": self.display_path(&cwd),
                "dry_run": false,
            }),
        ))
    }

    async fn git_status(&self) -> Result<ToolResult, ToolError> {
        let args = vec![
            "status".to_string(),
            "--short".to_string(),
            "--branch".to_string(),
        ];
        let output = self
            .run_program("git", &args, &self.workspace_root, Some(30_000))
            .await?;
        let summary = if output.success {
            "git status completed"
        } else {
            "git status failed"
        };

        Ok(output.into_tool_result(
            summary,
            serde_json::json!({
                "cwd": self.display_path(&self.workspace_root),
            }),
        ))
    }

    async fn git_diff(&self, pathspec: Option<&str>) -> Result<ToolResult, ToolError> {
        if let Some(pathspec) = pathspec {
            reject_secretish("pathspec", pathspec)?;
            if pathspec.starts_with('-') {
                return Err(ToolError::InvalidArguments {
                    message: "pathspec cannot start with '-'".to_string(),
                });
            }
        }

        let mut args = vec!["diff".to_string(), "--no-ext-diff".to_string()];
        if let Some(pathspec) = pathspec {
            args.push("--".to_string());
            args.push(pathspec.to_string());
        }

        let output = self
            .run_program("git", &args, &self.workspace_root, Some(30_000))
            .await?;
        let summary = if output.success {
            "git diff completed"
        } else {
            "git diff failed"
        };

        Ok(output.into_tool_result(
            summary,
            serde_json::json!({
                "cwd": self.display_path(&self.workspace_root),
                "pathspec": pathspec,
            }),
        ))
    }

    async fn run_program(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        timeout_ms: Option<u64>,
    ) -> Result<ProcessOutput, ToolError> {
        let timeout_ms = timeout_ms.unwrap_or(self.timeout_ms);
        let command = command_summary(program, args);
        let start = Instant::now();
        let mut process = Command::new(program);
        process
            .args(args)
            .current_dir(cwd)
            .kill_on_drop(true)
            .env_clear();

        if let Some(path) = std::env::var_os("PATH") {
            process.env("PATH", path);
        }
        process.env("PHARNESS", "1");

        let output = timeout(Duration::from_millis(timeout_ms), process.output())
            .await
            .map_err(|_| ToolError::TimedOut {
                command: command.clone(),
                timeout_ms,
            })?
            .map_err(|error| ToolError::Io {
                message: format!("failed to run {command}: {error}"),
            })?;

        let (stdout, stdout_truncated) = truncate(
            &String::from_utf8_lossy(&output.stdout),
            self.max_output_bytes,
        );
        let (stderr, stderr_truncated) = truncate(
            &String::from_utf8_lossy(&output.stderr),
            self.max_output_bytes,
        );

        Ok(ProcessOutput {
            command,
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: redact_text(&stdout),
            stderr: redact_text(&stderr),
            stdout_truncated,
            stderr_truncated,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn reject_never_execute_command(&self, command: &str) -> Result<(), ToolError> {
        match classify_command(command) {
            CommandClass::Privileged => Err(ToolError::InvalidArguments {
                message: "privileged shell commands are denied by the executor".to_string(),
            }),
            CommandClass::SecretAccessing => Err(ToolError::InvalidArguments {
                message: "secret-accessing shell commands are denied by the executor".to_string(),
            }),
            _ => Ok(()),
        }
    }

    fn resolve_cwd(&self, cwd: Option<&Utf8PathBuf>) -> Result<PathBuf, ToolError> {
        let candidate = match cwd {
            Some(cwd) if cwd.is_absolute() => PathBuf::from(cwd.as_str()),
            Some(cwd) => self.workspace_root.join(cwd.as_str()),
            None => self.workspace_root.clone(),
        };

        let canonical = candidate.canonicalize().map_err(|error| ToolError::Io {
            message: format!(
                "failed to canonicalize cwd {}: {error}",
                candidate.display()
            ),
        })?;

        if !canonical.starts_with(&self.canonical_root) {
            return Err(ToolError::OutsideWorkspace {
                path: candidate.display().to_string(),
            });
        }

        Ok(canonical)
    }

    fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.canonical_root)
            .unwrap_or(path)
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string()
    }
}

#[async_trait]
impl ToolExecutor for LocalShellTools {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        match action {
            AgentAction::RunShell {
                cmd,
                cwd,
                timeout_ms,
                dry_run,
                ..
            } => {
                self.run_shell(cmd, cwd.as_ref(), *timeout_ms, *dry_run)
                    .await
            }
            AgentAction::GitStatus { .. } => self.git_status().await,
            AgentAction::GitDiff { pathspec, .. } => self.git_diff(pathspec.as_deref()).await,
            other => Err(ToolError::UnsupportedAction {
                action: other.kind_name().to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessOutput {
    command: String,
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
    duration_ms: u64,
}

impl ProcessOutput {
    fn into_tool_result(self, summary: &str, extra: serde_json::Value) -> ToolResult {
        let content = serde_json::json!({
            "command": self.command,
            "exit_code": self.exit_code,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "stdout_truncated": self.stdout_truncated,
            "stderr_truncated": self.stderr_truncated,
            "duration_ms": self.duration_ms,
            "extra": extra,
        });

        if self.success {
            ToolResult::ok(summary, content)
        } else {
            ToolResult::error(summary, content)
        }
    }
}

fn command_summary(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn reject_secretish(label: &str, value: &str) -> Result<(), ToolError> {
    if looks_secretish(value) {
        return Err(ToolError::InvalidArguments {
            message: format!("{label} appears to request secret data"),
        });
    }
    Ok(())
}

fn looks_secretish(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        ".env",
        "secret",
        "token",
        "password",
        "credential",
        "kubeconfig",
        "private_key",
        "authorization",
        "id_rsa",
        "id_ed25519",
    ]
    .into_iter()
    .any(|needle| value.contains(needle))
}

fn redact_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if looks_secretish(line) {
                "[redacted]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    (format!("{}...[truncated]", &value[..end]), true)
}

#[cfg(test)]
mod tests {
    use super::LocalShellTools;
    use crate::{AgentAction, ToolError, ToolExecutor, ToolResultStatus};
    use camino::Utf8PathBuf;
    use std::fs;

    #[tokio::test]
    async fn dry_run_shell_does_not_execute_command() {
        let temp = unique_temp_dir("shell-dry-run");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalShellTools::new(&temp).unwrap();

        let result = tools
            .execute(&AgentAction::RunShell {
                id: "act_shell".into(),
                reason: "dry run".to_string(),
                cmd: "touch created.txt".to_string(),
                cwd: None,
                timeout_ms: None,
                dry_run: true,
            })
            .await
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert!(!temp.join("created.txt").exists());
    }

    #[tokio::test]
    async fn runs_shell_inside_workspace() {
        let temp = unique_temp_dir("shell-pwd");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalShellTools::new(&temp).unwrap();

        let result = tools
            .execute(&AgentAction::RunShell {
                id: "act_pwd".into(),
                reason: "pwd".to_string(),
                cmd: "pwd".to_string(),
                cwd: None,
                timeout_ms: Some(10_000),
                dry_run: false,
            })
            .await
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert!(result.content["stdout"]
            .as_str()
            .unwrap()
            .contains(temp.canonicalize().unwrap().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn nonzero_shell_exit_is_tool_result_error() {
        let temp = unique_temp_dir("shell-nonzero");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalShellTools::new(&temp).unwrap();

        let result = tools
            .execute(&AgentAction::RunShell {
                id: "act_exit".into(),
                reason: "exit".to_string(),
                cmd: "exit 7".to_string(),
                cwd: None,
                timeout_ms: Some(10_000),
                dry_run: false,
            })
            .await
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Error);
        assert_eq!(result.content["exit_code"], 7);
    }

    #[tokio::test]
    async fn rejects_cwd_outside_workspace() {
        let temp = unique_temp_dir("shell-outside");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalShellTools::new(&temp).unwrap();

        let error = tools
            .execute(&AgentAction::RunShell {
                id: "act_outside".into(),
                reason: "outside".to_string(),
                cmd: "pwd".to_string(),
                cwd: Some(Utf8PathBuf::from("/")),
                timeout_ms: Some(10_000),
                dry_run: false,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::OutsideWorkspace { .. }));
    }

    #[tokio::test]
    async fn denies_secret_shell_command_before_execution() {
        let temp = unique_temp_dir("shell-secret");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalShellTools::new(&temp).unwrap();

        let error = tools
            .execute(&AgentAction::RunShell {
                id: "act_secret".into(),
                reason: "secret".to_string(),
                cmd: "cat .env".to_string(),
                cwd: None,
                timeout_ms: Some(10_000),
                dry_run: false,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn git_status_non_repo_is_structured_tool_error() {
        let temp = unique_temp_dir("git-status");
        fs::create_dir_all(&temp).unwrap();
        let tools = LocalShellTools::new(&temp).unwrap();

        let result = tools
            .execute(&AgentAction::GitStatus {
                id: "act_git".into(),
                reason: "status".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Error);
        assert!(result.content["stderr"].as_str().unwrap().contains("git"));
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
