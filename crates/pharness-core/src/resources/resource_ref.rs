use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub provider: String,
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub uri: Option<String>,
    pub metadata: serde_json::Value,
}

impl ResourceRef {
    pub fn new(
        provider: impl Into<String>,
        kind: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            kind: kind.into(),
            name: name.into(),
            namespace: None,
            uri: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    pub fn local_file(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            provider: "local".to_string(),
            kind: "file".to_string(),
            name: path.clone(),
            namespace: None,
            uri: Some(format!("workspace://{path}")),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}
