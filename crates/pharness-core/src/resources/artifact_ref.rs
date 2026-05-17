use crate::{ArtifactId, ResourceRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub kind: String,
    pub label: String,
    pub uri: Option<String>,
    pub resource_ref: Option<ResourceRef>,
}

impl ArtifactRef {
    pub fn new(
        artifact_id: impl Into<ArtifactId>,
        kind: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            kind: kind.into(),
            label: label.into(),
            uri: None,
            resource_ref: None,
        }
    }
}
