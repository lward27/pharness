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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunScope {
    pub namespace: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub work_item_id: Option<String>,
    pub workspace_id: Option<String>,
    pub work_plan_id: Option<String>,
    pub change_set_id: Option<String>,
    pub production_impacting: bool,
}

impl RunScope {
    pub fn is_empty(&self) -> bool {
        self.namespace.is_none()
            && self.repo.is_none()
            && self.branch.is_none()
            && self.work_item_id.is_none()
            && self.workspace_id.is_none()
            && self.work_plan_id.is_none()
            && self.change_set_id.is_none()
            && !self.production_impacting
    }

    pub fn to_optional_json(&self) -> Option<serde_json::Value> {
        if self.is_empty() {
            None
        } else {
            Some(
                serde_json::to_value(self)
                    .expect("RunScope contains only infallible JSON value types"),
            )
        }
    }

    pub fn from_execution_target(execution_target: &serde_json::Value) -> Option<Self> {
        execution_target
            .get("run_scope")
            .cloned()
            .and_then(|value| serde_json::from_value::<Self>(value).ok())
            .filter(|scope| !scope.is_empty())
    }
}
