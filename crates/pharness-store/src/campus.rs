use serde::{Deserialize, Serialize};

/// A course role granted by a tenant administrator. Tenant administration is
/// a separate tenant-scoped grant, never a course role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusCourseRole {
    Student,
    Instructor,
}

impl CampusCourseRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Student => "student",
            Self::Instructor => "instructor",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "student" => Some(Self::Student),
            "instructor" => Some(Self::Instructor),
            _ => None,
        }
    }
}

/// The identity tuple shared by identities, administrators and memberships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusIdentityKey {
    pub tenant_id: String,
    pub issuer: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCampusLoginAttempt {
    pub state_hash: String,
    pub tenant_id: String,
    pub browser_binding_hash: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub now: i64,
    pub expires_at: i64,
}

/// A login attempt that was atomically marked consumed by this call.
#[derive(Clone, PartialEq, Eq)]
pub struct ConsumedCampusLoginAttempt {
    pub tenant_id: String,
    pub browser_binding_hash: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub expires_at: i64,
}

impl std::fmt::Debug for ConsumedCampusLoginAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConsumedCampusLoginAttempt")
            .field("tenant_id", &self.tenant_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertCampusIdentity {
    /// Used only when the identity tuple has never logged in before.
    pub candidate_id: String,
    pub key: CampusIdentityKey,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusIdentity {
    pub id: String,
    pub tenant_id: String,
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_login_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCampusSession {
    pub id_hash: String,
    pub tenant_id: String,
    pub identity_id: String,
    pub csrf_hash: String,
    pub now: i64,
    pub idle_expires_at: i64,
    pub absolute_expires_at: i64,
}

/// A session that was valid at `now` and whose idle window was extended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveCampusSession {
    pub tenant_id: String,
    pub csrf_hash: String,
    pub idle_expires_at: i64,
    pub absolute_expires_at: i64,
    pub identity: StoredCampusIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusMembership {
    pub tenant_id: String,
    pub course_id: String,
    pub issuer: String,
    pub subject: String,
    pub role: CampusCourseRole,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub granted_by_identity_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutCampusMembership {
    pub key: CampusIdentityKey,
    pub course_id: String,
    pub role: CampusCourseRole,
    pub granted_by_identity_id: String,
    pub now: i64,
}
