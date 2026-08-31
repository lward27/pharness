use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::watch;

const MAX_PROTOCOL_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct AppServerConfig {
    pub codex_path: PathBuf,
    pub codex_home: PathBuf,
    pub cwd: PathBuf,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt: String,
    pub output_schema: Value,
    pub workspace_write: bool,
    pub writable_roots: Vec<PathBuf>,
    pub environment: BTreeMap<String, String>,
    pub upstream_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerEvent {
    pub method: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct AppServerOutcome {
    pub thread_id: String,
    pub turn_id: String,
    pub status: String,
    pub structured_output: Option<Value>,
    pub error: Option<String>,
    pub events: Vec<AppServerEvent>,
    pub usage: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct SandboxedCommandOutcome {
    pub exit_code: Option<i64>,
    pub stdout: String,
    pub stderr: String,
}

pub struct AppServerSession {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    events: Vec<AppServerEvent>,
    usage: Option<Value>,
}

impl AppServerSession {
    pub async fn start(config: &AppServerConfig) -> anyhow::Result<Self> {
        write_command_environment_policy(config)?;
        let mut command = Command::new(&config.codex_path);
        command
            .args(["app-server", "--listen", "stdio://"])
            .env("CODEX_HOME", &config.codex_home)
            .envs(&config.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(api_key) = &config.upstream_api_key {
            command.env("OPENAI_API_KEY", api_key);
        } else {
            command.env_remove("OPENAI_API_KEY");
        }
        let mut child = command
            .spawn()
            .context("failed to start Codex App Server")?;
        let stdin = child.stdin.take().context("App Server has no stdin")?;
        let stdout = child.stdout.take().context("App Server has no stdout")?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(message = %bounded(&line, 2_000), "Codex App Server stderr");
                }
            });
        }
        let mut session = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 1,
            events: Vec::new(),
            usage: None,
        };
        let initialized = session
            .request(
                "initialize",
                json!({
                    "clientInfo":{"name":"pharness-codex-host","version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"experimentalApi":false,"requestAttestation":false}
                }),
            )
            .await?;
        if initialized.get("userAgent").is_none() && !initialized.is_object() {
            anyhow::bail!("Codex App Server returned an invalid initialize response");
        }
        session.notify("initialized", json!({})).await?;
        session.seal_authentication_boundary(config).await?;
        Ok(session)
    }

    pub async fn start_or_resume_thread(
        &mut self,
        config: &AppServerConfig,
        existing_thread_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let sandbox = if config.workspace_write {
            "workspace-write"
        } else {
            "read-only"
        };
        let result = if let Some(thread_id) = existing_thread_id {
            self.request(
                "thread/resume",
                json!({
                    "threadId":thread_id,
                    "model":config.model,
                    "cwd":config.cwd,
                    "approvalPolicy":"never",
                    "sandbox":sandbox,
                    "baseInstructions":config.prompt,
                    "excludeTurns":false,
                }),
            )
            .await?
        } else {
            self.request(
                "thread/start",
                json!({
                    "model":config.model,
                    "cwd":config.cwd,
                    "approvalPolicy":"never",
                    "sandbox":sandbox,
                    "baseInstructions":config.prompt,
                    "ephemeral":false,
                    "sessionStartSource":"startup",
                }),
            )
            .await?
        };
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("Codex App Server thread response has no thread ID")
    }

    pub async fn run_turn(
        &mut self,
        config: &AppServerConfig,
        thread_id: &str,
        mut cancel: watch::Receiver<bool>,
        active_time_limit: Duration,
    ) -> anyhow::Result<AppServerOutcome> {
        let sandbox_policy = if config.workspace_write {
            json!({
                "type":"workspaceWrite",
                "networkAccess":false,
                "writableRoots":config.writable_roots,
                "excludeTmpdirEnvVar":true,
                "excludeSlashTmp":true,
            })
        } else {
            json!({"type":"readOnly","networkAccess":false})
        };
        let request_id = self.next_request_id();
        self.write(&json!({
            "jsonrpc":"2.0",
            "id":request_id,
            "method":"turn/start",
            "params":{
                "threadId":thread_id,
                "input":[{"type":"text","text":config.prompt,"text_elements":[]}],
                "model":config.model,
                "effort":config.reasoning_effort,
                "cwd":config.cwd,
                "approvalPolicy":"never",
                "sandboxPolicy":sandbox_policy,
                "outputSchema":config.output_schema,
            }
        }))
        .await?;
        let mut turn_id = String::new();
        let mut interrupted = false;
        let mut stop_reason: Option<&'static str> = None;
        let mut interruption_deadline: Option<tokio::time::Instant> = None;
        let active_deadline = tokio::time::sleep(active_time_limit);
        tokio::pin!(active_deadline);
        loop {
            tokio::select! {
                changed = cancel.changed(), if !interrupted => {
                    if changed.is_ok() && *cancel.borrow() {
                        stop_reason = Some("cancelled");
                        interruption_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
                        if !turn_id.is_empty() {
                            let _ = self.request("turn/interrupt", json!({"threadId":thread_id,"turnId":turn_id})).await;
                        }
                        interrupted = true;
                    }
                }
                _ = &mut active_deadline, if !interrupted => {
                    stop_reason = Some("active_time_exhausted");
                    interruption_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
                    if !turn_id.is_empty() {
                        let _ = self.request("turn/interrupt", json!({"threadId":thread_id,"turnId":turn_id})).await;
                    }
                    interrupted = true;
                }
                _ = tokio::time::sleep_until(interruption_deadline.unwrap_or_else(tokio::time::Instant::now)), if interruption_deadline.is_some() => {
                    anyhow::bail!("Codex App Server did not stop within 15 seconds after {}", stop_reason.unwrap_or("interruption"));
                }
                message = self.next_message() => {
                    let message = message?;
                    if message.get("id").and_then(Value::as_u64) == Some(request_id) {
                        if let Some(error) = message.get("error") {
                            return Err(anyhow::anyhow!("turn/start rejected: {}", bounded(&error.to_string(), 4_000)));
                        }
                        turn_id = message.pointer("/result/turn/id").and_then(Value::as_str).unwrap_or_default().to_string();
                        if interrupted && !turn_id.is_empty() {
                            let _ = self.request("turn/interrupt", json!({"threadId":thread_id,"turnId":turn_id})).await;
                        }
                        continue;
                    }
                    if message.get("method").is_some() && message.get("id").is_some() {
                        self.reject_server_request(&message).await?;
                        continue;
                    }
                    let Some(method) = message.get("method").and_then(Value::as_str) else {
                        continue;
                    };
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    if method == "thread/tokenUsage/updated" {
                        self.usage = Some(params.clone());
                    }
                    if method == "turn/completed" {
                        let notification_thread = params.get("threadId").and_then(Value::as_str);
                        if notification_thread != Some(thread_id) {
                            continue;
                        }
                        let turn = params.get("turn").cloned().unwrap_or(Value::Null);
                        if turn_id.is_empty() {
                            turn_id = turn.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                        }
                        let status = match stop_reason {
                            Some("active_time_exhausted") => "active_time_exhausted".to_string(),
                            Some("cancelled") => "interrupted".to_string(),
                            _ => turn.get("status").and_then(Value::as_str).unwrap_or("failed").to_string(),
                        };
                        let error = stop_reason
                            .map(str::to_string)
                            .or_else(|| turn.pointer("/error/message").and_then(Value::as_str).map(str::to_string));
                        let structured_output = extract_structured_output(&turn);
                        self.events.push(AppServerEvent { method: method.into(), payload: params });
                        return Ok(AppServerOutcome {
                            thread_id: thread_id.into(),
                            turn_id,
                            status,
                            structured_output,
                            error,
                            events: std::mem::take(&mut self.events),
                            usage: self.usage.take(),
                        });
                    }
                    if retain_notification(method) {
                        self.events.push(AppServerEvent { method: method.into(), payload: params });
                    }
                }
            }
        }
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        let _ = self.stdin.shutdown().await;
        match tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await {
            Ok(result) => {
                let _ = result?;
            }
            Err(_) => {
                self.child.kill().await?;
            }
        }
        Ok(())
    }

    pub async fn exec_sandboxed_command(
        &mut self,
        cwd: &std::path::Path,
        command: &str,
        environment: &BTreeMap<String, String>,
        writable_roots: &[PathBuf],
        timeout: Duration,
    ) -> anyhow::Result<SandboxedCommandOutcome> {
        let mut env = environment
            .iter()
            .map(|(name, value)| (name.clone(), Value::String(value.clone())))
            .collect::<serde_json::Map<_, _>>();
        env.insert("HOME".into(), Value::String("/tmp".into()));
        env.insert("LANG".into(), Value::String("C.UTF-8".into()));
        env.insert("CODEX_HOME".into(), Value::Null);
        let result = self
            .request(
                "command/exec",
                json!({
                    "command":["/bin/sh","-c",command],
                    "cwd":cwd,
                    "env":env,
                    "timeoutMs":u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    "outputBytesCap":131_072,
                    "sandboxPolicy":{
                        "type":"workspaceWrite",
                        "networkAccess":false,
                        "writableRoots":writable_roots,
                        "excludeTmpdirEnvVar":true,
                        "excludeSlashTmp":false,
                    },
                }),
            )
            .await?;
        Ok(SandboxedCommandOutcome {
            exit_code: result.get("exitCode").and_then(Value::as_i64),
            stdout: command_output(&result, "stdout"),
            stderr: command_output(&result, "stderr"),
        })
    }

    async fn seal_authentication_boundary(
        &mut self,
        config: &AppServerConfig,
    ) -> anyhow::Result<()> {
        let auth_path = config.codex_home.join("auth.json");
        if auth_path.exists() {
            std::fs::remove_file(&auth_path)
                .context("failed to remove the transient Codex authentication file")?;
        }
        let result = self
            .request(
                "command/exec",
                json!({
                    "command":[
                        "/bin/sh",
                        "-c",
                        "test ! -r \"$1\"",
                        "pharness-auth-boundary",
                        auth_path,
                    ],
                    "cwd":config.cwd,
                    "env":{"CODEX_HOME":null,"HOME":"/tmp"},
                    "timeoutMs":5_000,
                    "outputBytesCap":4_096,
                    "sandboxPolicy":{"type":"readOnly","networkAccess":false},
                }),
            )
            .await?;
        if result.get("exitCode").and_then(Value::as_i64) != Some(0) {
            anyhow::bail!("Codex command sandbox can read its authentication file");
        }
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_request_id();
        self.write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let message = self.next_message().await?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    anyhow::bail!(
                        "Codex App Server {method} failed: {}",
                        bounded(&error.to_string(), 4_000)
                    );
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            if message.get("method").is_some() && message.get("id").is_some() {
                self.reject_server_request(&message).await?;
                continue;
            }
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                let payload = message.get("params").cloned().unwrap_or(Value::Null);
                if retain_notification(method) {
                    self.events.push(AppServerEvent {
                        method: method.into(),
                        payload,
                    });
                }
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write(&json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await
    }

    async fn reject_server_request(&mut self, message: &Value) -> anyhow::Result<()> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "item/commandExecution/requestApproval" | "execCommandApproval" => {
                json!({"decision":"decline"})
            }
            "item/fileChange/requestApproval" | "applyPatchApproval" => {
                json!({"decision":"decline"})
            }
            _ => {
                self.write(&json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "error":{"code":-32601,"message":"PHarness does not permit interactive or external App Server requests"}
                }))
                .await?;
                return Ok(());
            }
        };
        self.write(&json!({"jsonrpc":"2.0","id":id,"result":result}))
            .await
    }

    async fn next_message(&mut self) -> anyhow::Result<Value> {
        let line = self
            .lines
            .next_line()
            .await
            .context("failed to read Codex App Server output")?
            .context("Codex App Server closed its protocol stream")?;
        if line.len() > MAX_PROTOCOL_LINE_BYTES {
            anyhow::bail!("Codex App Server protocol line exceeded the 4 MiB limit");
        }
        serde_json::from_str(&line).context("Codex App Server emitted malformed JSON-RPC")
    }

    async fn write(&mut self, value: &Value) -> anyhow::Result<()> {
        let mut encoded = serde_json::to_vec(value)?;
        encoded.push(b'\n');
        self.stdin.write_all(&encoded).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn write_command_environment_policy(config: &AppServerConfig) -> anyhow::Result<()> {
    std::fs::create_dir_all(&config.codex_home)?;
    let mut values = BTreeMap::new();
    for name in ["PATH", "PYTHONPATH", "PHARNESS_AGENT_STAGE"] {
        if let Some(value) = config.environment.get(name) {
            values.insert(name.to_string(), value.clone());
        }
    }
    values.insert("HOME".into(), "/tmp".into());
    values.insert("LANG".into(), "C.UTF-8".into());
    let document = toml::to_string(&json!({
        "shell_environment_policy":{
            "inherit":"none",
            "ignore_default_excludes":false,
            "set":values,
            "include_only":[],
            "experimental_use_profile":false,
        }
    }))?;
    std::fs::write(config.codex_home.join("config.toml"), document)?;
    Ok(())
}

fn retain_notification(method: &str) -> bool {
    matches!(
        method,
        "turn/started"
            | "turn/completed"
            | "item/started"
            | "item/completed"
            | "item/commandExecution/outputDelta"
            | "item/fileChange/patchUpdated"
            | "turn/diff/updated"
            | "thread/tokenUsage/updated"
            | "error"
    )
}

fn extract_structured_output(turn: &Value) -> Option<Value> {
    let items = turn.get("items")?.as_array()?;
    for item in items.iter().rev() {
        if item.get("type").and_then(Value::as_str) != Some("agentMessage") {
            continue;
        }
        for candidate in [
            item.get("text"),
            item.get("content").and_then(|value| value.get("text")),
            item.get("message").and_then(|value| value.get("text")),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(text) = candidate.as_str() {
                if let Ok(value) = serde_json::from_str(text) {
                    return Some(value);
                }
            } else if candidate.is_object() {
                return Some(candidate.clone());
            }
        }
    }
    None
}

fn command_output(result: &Value, field: &str) -> String {
    result
        .get(field)
        .and_then(Value::as_str)
        .or_else(|| {
            (field == "stdout")
                .then(|| result.get("output").and_then(Value::as_str))
                .flatten()
        })
        .unwrap_or_default()
        .to_string()
}

fn bounded(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
fn sandbox_policy(workspace_write: bool, writable_roots: &[PathBuf]) -> Value {
    if workspace_write {
        json!({
            "type":"workspaceWrite",
            "networkAccess":false,
            "writableRoots":writable_roots,
            "excludeTmpdirEnvVar":true,
            "excludeSlashTmp":true,
        })
    } else {
        json!({"type":"readOnly","networkAccess":false})
    }
}

#[cfg(test)]
fn validate_workspace_root(
    root: &std::path::Path,
    candidate: &std::path::Path,
) -> anyhow::Result<()> {
    if !candidate.is_absolute() || !candidate.starts_with(root) {
        anyhow::bail!("App Server writable root escapes the lease workspace");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_terminal_agent_message() {
        let turn = json!({
            "items":[
                {"type":"reasoning","summary":[]},
                {"type":"agentMessage","text":"{\"decision\":\"approved\"}"}
            ]
        });
        assert_eq!(
            extract_structured_output(&turn).unwrap()["decision"],
            "approved"
        );
    }

    #[test]
    fn sandbox_never_grants_command_network() {
        let policy = sandbox_policy(true, &[PathBuf::from("/workspace/src")]);
        assert_eq!(policy["networkAccess"], false);
        assert_eq!(policy["type"], "workspaceWrite");
    }

    #[test]
    fn writable_root_must_stay_inside_workspace() {
        assert!(validate_workspace_root(
            std::path::Path::new("/workspace"),
            std::path::Path::new("/workspace/src")
        )
        .is_ok());
        assert!(validate_workspace_root(
            std::path::Path::new("/workspace"),
            std::path::Path::new("/etc")
        )
        .is_err());
    }
}
