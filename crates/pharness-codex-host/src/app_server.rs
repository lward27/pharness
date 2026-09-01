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
const PHARNESS_PERMISSION_PROFILE: &str = "pharness-stage";
const SAFE_COMMAND_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

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
    pub denied_read_paths: Vec<PathBuf>,
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
                    "capabilities":{"experimentalApi":true,"requestAttestation":false}
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
        let result = if let Some(thread_id) = existing_thread_id {
            self.request(
                "thread/resume",
                json!({
                    "threadId":thread_id,
                    "model":config.model,
                    "cwd":config.cwd,
                    "approvalPolicy":"never",
                    "permissions":PHARNESS_PERMISSION_PROFILE,
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
                    "permissions":PHARNESS_PERMISSION_PROFILE,
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
                "permissions":PHARNESS_PERMISSION_PROFILE,
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
                    "permissionProfile":PHARNESS_PERMISSION_PROFILE,
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
        let denied_paths = denied_read_paths(config)?;
        let existing_denied_paths = denied_paths
            .iter()
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        if existing_denied_paths.is_empty() {
            anyhow::bail!("Codex authentication boundary has no credential path to verify");
        }
        let mut command = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "for path do test ! -r \"$path\" || exit 41; done".to_string(),
            "pharness-auth-boundary".to_string(),
        ];
        command.extend(
            existing_denied_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned()),
        );
        let result = self
            .request(
                "command/exec",
                json!({
                    "command":command,
                    "cwd":config.cwd,
                    "env":{"CODEX_HOME":null,"HOME":"/tmp"},
                    "timeoutMs":5_000,
                    "outputBytesCap":4_096,
                    "permissionProfile":PHARNESS_PERMISSION_PROFILE,
                }),
            )
            .await?;
        match result.get("exitCode").and_then(Value::as_i64) {
            Some(0) => {}
            Some(41) => {
                anyhow::bail!("Codex command sandbox can read authentication material");
            }
            exit_code => {
                anyhow::bail!(
                    "Codex authentication boundary probe failed to execute: exit_code={exit_code:?}, stderr={}",
                    bounded(&command_output(&result, "stderr"), 1_000)
                );
            }
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
    if !config.workspace_write && !config.writable_roots.is_empty() {
        anyhow::bail!("read-only Codex stage cannot declare writable roots");
    }
    let mut values = BTreeMap::new();
    for name in ["PATH", "PYTHONPATH", "PHARNESS_AGENT_STAGE"] {
        if let Some(value) = config.environment.get(name) {
            values.insert(name.to_string(), value.clone());
        }
    }
    // Codex resolves the Linux sandbox launcher from the command environment.
    // Keep a deterministic system PATH even for startup probes that do not yet
    // have an EnvironmentSnapshot, so Ubuntu's AppArmor-authorized bwrap is
    // preferred over the bundled fallback.
    values
        .entry("PATH".into())
        .or_insert_with(|| SAFE_COMMAND_PATH.into());
    values.insert("HOME".into(), "/tmp".into());
    values.insert("LANG".into(), "C.UTF-8".into());
    let mut filesystem = BTreeMap::from([(String::from("/"), String::from("read"))]);
    for path in &config.writable_roots {
        let path = absolute_policy_path(path)?;
        filesystem.insert(path, "write".into());
    }
    let git_metadata = config.cwd.join(".git");
    filesystem.insert(absolute_policy_path(&git_metadata)?, "read".into());
    for path in denied_read_paths(config)? {
        filesystem.insert(absolute_policy_path(&path)?, "deny".into());
    }
    let permissions = BTreeMap::from([(
        PHARNESS_PERMISSION_PROFILE.to_string(),
        json!({
            "description":"PHarness stage-scoped filesystem and network boundary",
            "filesystem":filesystem,
            "network":{"enabled":false},
        }),
    )]);
    let document = toml::to_string(&json!({
        "check_for_update_on_startup":false,
        "default_permissions":PHARNESS_PERMISSION_PROFILE,
        "web_search":"disabled",
        "agents":{"enabled":false},
        "apps":{"_default":{"enabled":false}},
        "features":{
            "apps":false,
            "browser_use":false,
            "browser_use_external":false,
            "computer_use":false,
            "hooks":false,
            "image_generation":false,
            "in_app_browser":false,
            "multi_agent":false,
            "plugins":false,
            "remote_plugin":false,
            "skill_mcp_dependency_install":false,
            "tool_suggest":false,
            "workspace_dependencies":false,
        },
        "permissions":permissions,
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

fn denied_read_paths(config: &AppServerConfig) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = config.denied_read_paths.clone();
    paths.push(config.codex_home.join("auth.json"));
    paths.sort();
    paths.dedup();
    for path in &paths {
        absolute_policy_path(path)?;
    }
    Ok(paths)
}

fn absolute_policy_path(path: &std::path::Path) -> anyhow::Result<String> {
    if !path.is_absolute() {
        anyhow::bail!("Codex permission path must be absolute");
    }
    Ok(path.to_string_lossy().into_owned())
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
    fn permission_profile_denies_auth_and_network() {
        let temporary = std::env::temp_dir().join(format!(
            "pharness-codex-permission-profile-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&temporary).unwrap();
        let config = AppServerConfig {
            codex_path: PathBuf::from("/usr/local/bin/codex"),
            codex_home: temporary.join("codex-home"),
            cwd: PathBuf::from("/workspace"),
            model: "gpt-5.6-sol".into(),
            reasoning_effort: "high".into(),
            prompt: "test".into(),
            output_schema: json!({"type":"object"}),
            workspace_write: true,
            writable_roots: vec![PathBuf::from("/workspace/src")],
            denied_read_paths: vec![PathBuf::from("/run/secrets/api-key")],
            environment: BTreeMap::new(),
            upstream_api_key: None,
        };
        write_command_environment_policy(&config).unwrap();
        let document = std::fs::read_to_string(config.codex_home.join("config.toml")).unwrap();
        let value = document.parse::<toml::Value>().unwrap();
        assert_eq!(
            value
                .get("default_permissions")
                .and_then(toml::Value::as_str),
            Some(PHARNESS_PERMISSION_PROFILE)
        );
        let filesystem = value
            .get("permissions")
            .and_then(|value| value.get(PHARNESS_PERMISSION_PROFILE))
            .and_then(|value| value.get("filesystem"))
            .and_then(toml::Value::as_table)
            .unwrap();
        assert_eq!(
            filesystem.get("/").and_then(toml::Value::as_str),
            Some("read")
        );
        assert_eq!(
            filesystem
                .get("/workspace/src")
                .and_then(toml::Value::as_str),
            Some("write")
        );
        assert_eq!(
            filesystem
                .get("/workspace/.git")
                .and_then(toml::Value::as_str),
            Some("read")
        );
        assert_eq!(
            filesystem
                .get("/run/secrets/api-key")
                .and_then(toml::Value::as_str),
            Some("deny")
        );
        assert_eq!(
            filesystem
                .get(config.codex_home.join("auth.json").to_str().unwrap())
                .and_then(toml::Value::as_str),
            Some("deny")
        );
        assert_eq!(
            value
                .get("permissions")
                .and_then(|value| value.get(PHARNESS_PERMISSION_PROFILE))
                .and_then(|value| value.get("network"))
                .and_then(|value| value.get("enabled"))
                .and_then(toml::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            value.get("web_search").and_then(toml::Value::as_str),
            Some("disabled")
        );
        assert_eq!(
            value
                .get("agents")
                .and_then(|value| value.get("enabled"))
                .and_then(toml::Value::as_bool),
            Some(false)
        );
        for feature in [
            "apps",
            "browser_use",
            "browser_use_external",
            "computer_use",
            "hooks",
            "image_generation",
            "in_app_browser",
            "multi_agent",
            "plugins",
            "remote_plugin",
            "skill_mcp_dependency_install",
            "tool_suggest",
            "workspace_dependencies",
        ] {
            assert_eq!(
                value
                    .get("features")
                    .and_then(|value| value.get(feature))
                    .and_then(toml::Value::as_bool),
                Some(false),
                "feature {feature} must be disabled"
            );
        }
        assert_eq!(
            value
                .get("shell_environment_policy")
                .and_then(|value| value.get("set"))
                .and_then(|value| value.get("PATH"))
                .and_then(toml::Value::as_str),
            Some(SAFE_COMMAND_PATH)
        );
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn permission_profile_preserves_snapshot_path() {
        let temporary = std::env::temp_dir().join(format!(
            "pharness-codex-permission-path-{}",
            uuid::Uuid::now_v7().simple()
        ));
        let mut environment = BTreeMap::new();
        environment.insert(
            "PATH".into(),
            "/workspace/.pharness-runtime/bin:/usr/bin:/bin".into(),
        );
        let config = AppServerConfig {
            codex_path: PathBuf::from("/usr/local/bin/codex"),
            codex_home: temporary.join("codex-home"),
            cwd: PathBuf::from("/workspace"),
            model: "gpt-5.6-sol".into(),
            reasoning_effort: "high".into(),
            prompt: "test".into(),
            output_schema: json!({"type":"object"}),
            workspace_write: false,
            writable_roots: Vec::new(),
            denied_read_paths: Vec::new(),
            environment,
            upstream_api_key: None,
        };
        write_command_environment_policy(&config).unwrap();
        let document = std::fs::read_to_string(config.codex_home.join("config.toml")).unwrap();
        let value = document.parse::<toml::Value>().unwrap();
        assert_eq!(
            value
                .get("shell_environment_policy")
                .and_then(|value| value.get("set"))
                .and_then(|value| value.get("PATH"))
                .and_then(toml::Value::as_str),
            Some("/workspace/.pharness-runtime/bin:/usr/bin:/bin")
        );
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn permission_profile_rejects_relative_sensitive_paths() {
        let config = AppServerConfig {
            codex_path: PathBuf::from("/usr/local/bin/codex"),
            codex_home: PathBuf::from("/var/lib/pharness-codex"),
            cwd: PathBuf::from("/workspace"),
            model: "gpt-5.6-sol".into(),
            reasoning_effort: "high".into(),
            prompt: "test".into(),
            output_schema: json!({"type":"object"}),
            workspace_write: false,
            writable_roots: Vec::new(),
            denied_read_paths: vec![PathBuf::from("relative-secret")],
            environment: BTreeMap::new(),
            upstream_api_key: None,
        };
        assert!(write_command_environment_policy(&config).is_err());
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
