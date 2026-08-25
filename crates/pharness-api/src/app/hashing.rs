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
    pharness_core::canonical_json_sha256(value)
        .map_err(|error| ApiError::internal(format!("failed to encode canonical hash: {error}")))
}
