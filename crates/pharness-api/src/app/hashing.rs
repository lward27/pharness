use super::ApiError;
use sha2::{Digest, Sha256};

pub(in crate::app) fn material_hash(value: &serde_json::Value) -> Result<String, ApiError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|error| ApiError::internal(format!("failed to encode material hash: {error}")))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
}

/// Stable hashing for new versioned product contracts. Existing material
/// hashes intentionally keep their historical serialization behavior.
pub(in crate::app) fn canonical_material_hash(
    value: &serde_json::Value,
) -> Result<String, ApiError> {
    fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(values) => {
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                let mut canonical = serde_json::Map::new();
                for key in keys {
                    canonical.insert(key.clone(), canonicalize(&values[key]));
                }
                serde_json::Value::Object(canonical)
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.iter().map(canonicalize).collect())
            }
            other => other.clone(),
        }
    }

    material_hash(&canonicalize(value))
}
