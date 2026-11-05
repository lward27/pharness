//! What a Campus sandbox is given: an environment without credentials and
//! exactly the network the effective policy approves.

use super::CampusSessionAuthority;
use crate::{CampusEgressDestination, CampusNetworkMode};
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_SANDBOX_ENV_VARS: usize = 64;
const MAX_SANDBOX_ENV_VALUE_BYTES: usize = 4096;

/// Name fragments that mark a variable as carrying a credential.
const CREDENTIAL_NAME_FRAGMENTS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "API_KEY",
    "APIKEY",
    "PRIVATE_KEY",
    "ACCESS_KEY",
    "AUTH",
    "COOKIE",
    "SESSION",
];

/// Prefixes owned by the platform, its broker or a cloud provider.
const RESERVED_NAME_PREFIXES: &[&str] = &["PHARNESS", "AWS_", "AZURE_", "GOOGLE_", "GCP_"];
const RESERVED_NAMES: &[&str] = &["KUBECONFIG", "SSH_AUTH_SOCK", "DOCKER_CONFIG"];

/// The only network a session's sandbox may reach. It is derived from the
/// effective policy alone; nothing a tool call or a student sends changes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusSandboxNetwork {
    pub mode: CampusNetworkMode,
    pub egress: BTreeSet<CampusEgressDestination>,
}

impl CampusSessionAuthority {
    pub fn sandbox_network(&self) -> CampusSandboxNetwork {
        let egress = self.policy.constraints().egress.clone();
        CampusSandboxNetwork {
            mode: if egress.is_empty() {
                CampusNetworkMode::Denied
            } else {
                CampusNetworkMode::ApprovedEgress
            },
            egress,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusSandboxError {
    #[error("sandbox environment has more than {MAX_SANDBOX_ENV_VARS} variables")]
    TooManyVariables,
    #[error("sandbox environment variable name {0} is not a plain upper-case name")]
    InvalidName(String),
    #[error("sandbox environment variable {0} looks like a credential")]
    CredentialName(String),
    #[error("sandbox environment variable {0} is reserved for the platform")]
    ReservedName(String),
    #[error("sandbox environment variable {0} has an invalid value")]
    InvalidValue(String),
}

/// Refuse a sandbox environment that could carry a platform, broker or cloud
/// credential. Execution providers must call this on the exact environment
/// they hand the student's workload.
pub fn check_sandbox_environment<'a>(
    environment: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), CampusSandboxError> {
    let mut count = 0;
    for (name, value) in environment {
        count += 1;
        if count > MAX_SANDBOX_ENV_VARS {
            return Err(CampusSandboxError::TooManyVariables);
        }
        let plain = !name.is_empty()
            && name.len() <= 64
            && !name.starts_with(|character: char| character.is_ascii_digit())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        if !plain {
            return Err(CampusSandboxError::InvalidName(name.into()));
        }
        if RESERVED_NAMES.contains(&name)
            || RESERVED_NAME_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        {
            return Err(CampusSandboxError::ReservedName(name.into()));
        }
        if CREDENTIAL_NAME_FRAGMENTS
            .iter()
            .any(|fragment| name.contains(fragment))
        {
            return Err(CampusSandboxError::CredentialName(name.into()));
        }
        if value.len() > MAX_SANDBOX_ENV_VALUE_BYTES || value.contains('\0') {
            return Err(CampusSandboxError::InvalidValue(name.into()));
        }
    }
    Ok(())
}
