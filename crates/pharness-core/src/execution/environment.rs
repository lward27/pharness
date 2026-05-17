use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentRef {
    pub id: String,
    pub name: String,
    pub tier: EnvironmentTier,
    pub cluster: Option<String>,
    pub namespace: Option<String>,
}

impl EnvironmentRef {
    pub fn local() -> Self {
        Self {
            id: "local".to_string(),
            name: "Local".to_string(),
            tier: EnvironmentTier::Local,
            cluster: None,
            namespace: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentTier {
    Local,
    Dev,
    Staging,
    Production,
}
