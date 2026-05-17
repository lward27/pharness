use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionTarget {
    LocalProcess {
        cwd: Utf8PathBuf,
        shell: String,
    },
    KubernetesJob {
        cluster: String,
        namespace: String,
        service_account: String,
        workspace: WorkspaceMount,
        network_profile: String,
        resource_profile: String,
    },
}

impl Default for ExecutionTarget {
    fn default() -> Self {
        Self::LocalProcess {
            cwd: Utf8PathBuf::from("."),
            shell: "sh".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceMount {
    LocalPath {
        path: Utf8PathBuf,
    },
    EmptyDir {
        mount_path: Utf8PathBuf,
    },
    PersistentVolumeClaim {
        claim_name: String,
        mount_path: Utf8PathBuf,
    },
}
