use crate::CapabilityKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
    pub capability: CapabilityKind,
}

impl ToolSpec {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: serde_json::Value,
        capability: CapabilityKind,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters_schema,
            capability,
        }
    }
}
