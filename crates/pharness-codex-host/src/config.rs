use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Standalone,
    Kubernetes,
}

impl ExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Kubernetes => "kubernetes",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    pub api_url: String,
    pub identity_file: PathBuf,
    pub state_dir: PathBuf,
    pub workspace_root: PathBuf,
    pub execution_mode: ExecutionMode,
    pub codex_path: PathBuf,
    #[serde(default = "default_podman_path")]
    pub podman_path: PathBuf,
    #[serde(default)]
    pub codex_auth_file: Option<PathBuf>,
    #[serde(default)]
    pub api_key_file: Option<PathBuf>,
    #[serde(default)]
    pub source_reader_token_file: Option<PathBuf>,
    pub authentication_class: String,
    pub runner_images: BTreeMap<String, String>,
    #[serde(default = "default_slots")]
    pub available_slots: u32,
    #[serde(default = "default_poll_seconds")]
    pub poll_seconds: u64,
}

fn default_podman_path() -> PathBuf {
    PathBuf::from("/usr/bin/podman")
}

fn default_slots() -> u32 {
    1
}

fn default_poll_seconds() -> u64 {
    10
}

impl HostConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read host configuration {}", path.display()))?;
        let config: Self = toml::from_str(&raw).context("host configuration is invalid TOML")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let url = reqwest::Url::parse(&self.api_url).context("api_url is invalid")?;
        if url.scheme() != "https"
            && !is_loopback_http(&url)
            && !(self.execution_mode == ExecutionMode::Kubernetes && is_cluster_service_http(&url))
        {
            anyhow::bail!("api_url must use HTTPS except for an exact loopback development URL");
        }
        if url.username() != ""
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            anyhow::bail!("api_url must not contain credentials, a query, or a fragment");
        }
        if self.available_slots != 1 {
            anyhow::bail!("portable host V1 supports exactly one active stage chain");
        }
        if !(2..=60).contains(&self.poll_seconds) {
            anyhow::bail!("poll_seconds must be between 2 and 60");
        }
        if self.runner_images.is_empty()
            || self
                .runner_images
                .values()
                .any(|image| !digest_pinned(image))
        {
            anyhow::bail!("every configured runner image must be digest pinned");
        }
        if self.authentication_class != "chatgpt_session"
            && self.authentication_class != "api_key"
            && self.authentication_class != "workload_identity"
        {
            anyhow::bail!("authentication_class is unsupported");
        }
        if self.execution_mode == ExecutionMode::Kubernetes
            && self.authentication_class == "chatgpt_session"
        {
            anyhow::bail!("Kubernetes mode cannot use a ChatGPT session");
        }
        match self.authentication_class.as_str() {
            "chatgpt_session" if self.codex_auth_file.is_none() => {
                anyhow::bail!("chatgpt_session requires codex_auth_file")
            }
            "api_key" if self.api_key_file.is_none() => {
                anyhow::bail!("api_key authentication requires api_key_file")
            }
            "workload_identity" => {
                anyhow::bail!("workload_identity authentication is not implemented")
            }
            _ => {}
        }
        for path in [
            &self.identity_file,
            &self.state_dir,
            &self.workspace_root,
            &self.codex_path,
        ] {
            if !path.is_absolute() {
                anyhow::bail!("host paths must be absolute: {}", path.display());
            }
        }
        for path in [self.codex_auth_file.as_ref(), self.api_key_file.as_ref()]
            .into_iter()
            .flatten()
        {
            if !path.is_absolute() {
                anyhow::bail!("host paths must be absolute: {}", path.display());
            }
        }
        Ok(())
    }
}

fn is_loopback_http(url: &reqwest::Url) -> bool {
    url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "::1" | "localhost"))
}

fn is_cluster_service_http(url: &reqwest::Url) -> bool {
    url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".svc.cluster.local"))
}

fn digest_pinned(image: &str) -> bool {
    image
        .rsplit_once("@sha256:")
        .is_some_and(|(repository, hash)| {
            !repository.is_empty()
                && hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostIdentity {
    pub host_id: String,
    pub host_credential: String,
    pub display_name: String,
    pub host_pool: String,
}

impl HostIdentity {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read host identity {}", path.display()))?;
        serde_json::from_slice(&bytes).context("host identity is invalid")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseExecutionConfig {
    pub api_url: String,
    pub host_id: String,
    pub lease_id: String,
    pub lease_token: String,
    pub workspace_path: PathBuf,
    pub codex_path: PathBuf,
    pub codex_home: PathBuf,
    pub authentication_class: String,
    #[serde(default)]
    pub api_key_file: Option<PathBuf>,
    #[serde(default)]
    pub remote_thread_id: Option<String>,
    /// Number of App Server process restarts already consumed for this lease.
    /// This is persisted in the host-owned lease file and copied into the
    /// runner configuration on resume.
    #[serde(default)]
    pub protocol_restart_count: u32,
    #[serde(default)]
    pub context_repositories: Vec<ContextRepositoryMount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRepositoryMount {
    pub repository_id: String,
    pub source_commit: String,
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> HostConfig {
        HostConfig {
            api_url: "https://pharness.example.test".into(),
            identity_file: "/var/lib/pharness-codex-host/identity.json".into(),
            state_dir: "/var/lib/pharness-codex-host".into(),
            workspace_root: "/var/lib/pharness-codex-host/workspaces".into(),
            execution_mode: ExecutionMode::Standalone,
            codex_path: "/usr/lib/pharness-codex-host/codex".into(),
            podman_path: "/usr/bin/podman".into(),
            codex_auth_file: Some("/var/lib/pharness-codex-host/session/auth.json".into()),
            api_key_file: None,
            source_reader_token_file: None,
            authentication_class: "chatgpt_session".into(),
            runner_images: BTreeMap::from([(
                "python-3.11".into(),
                format!("registry.example/runner@sha256:{}", "a".repeat(64)),
            )]),
            available_slots: 1,
            poll_seconds: 10,
        }
    }

    #[test]
    fn accepts_portable_standalone_configuration() {
        config().validate().unwrap();
    }

    #[test]
    fn distributed_example_configuration_is_valid_toml() {
        let raw = include_str!("../../../deploy/codex-host/config.toml.example");
        let parsed: HostConfig = toml::from_str(raw).expect("example config must remain loadable");
        assert_eq!(
            parsed.runner_images.keys().cloned().collect::<Vec<_>>(),
            vec!["node-24".to_string(), "python-3.11".to_string()]
        );
    }

    #[test]
    fn distributed_installer_keeps_config_directory_traversable() {
        let installer = include_str!("../../../deploy/codex-host/install.sh");
        assert!(installer.contains("install -d -o root -g root -m 0755 /etc/pharness-codex-host"));
        assert!(installer.contains(
            "install -m 0640 -o root -g pharness-codex \"$bundle_root/etc/config.toml.example\" /etc/pharness-codex-host/config.toml"
        ));
        assert!(installer.contains("env PATH=/usr/local/bin:/usr/bin:/bin"));
        assert!(installer.contains("bwrap --help 2>&1"));
        assert!(installer.contains("--as-pid-1"));
        assert!(installer.contains("--perms"));
        assert!(installer.contains("$bundle_root/bin/codex-linux-sandbox"));
        assert!(installer.contains(
            "ln -sfn /opt/pharness-codex-host/current/bin/codex-linux-sandbox /usr/local/bin/codex-linux-sandbox"
        ));

        let codex_installer = include_str!("../../../deploy/docker/install-codex.sh");
        assert!(codex_installer.contains("ln \"$DESTINATION\" \"$SANDBOX_ALIAS\""));
        assert!(codex_installer.contains("cmp -s \"$DESTINATION\" \"$SANDBOX_ALIAS\""));
        assert!(codex_installer.contains("\"$SANDBOX_ALIAS\" --help >/dev/null"));
    }

    #[test]
    fn rejects_chatgpt_session_in_kubernetes() {
        let mut value = config();
        value.execution_mode = ExecutionMode::Kubernetes;
        assert!(value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("Kubernetes"));
    }

    #[test]
    fn rejects_mutable_runner_images() {
        let mut value = config();
        value
            .runner_images
            .insert("node-24".into(), "runner:latest".into());
        assert!(value.validate().unwrap_err().to_string().contains("digest"));
    }
}
