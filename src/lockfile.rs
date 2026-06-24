//! Source lock and resolved user lock schemas.

use serde::{Deserialize, Serialize};

/// Current persisted user-lock schema version.
pub const USER_LOCK_SCHEMA_VERSION: u32 = 1;

/// Resolved user lock.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserLock {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Source commit snapshots known to the lock.
    pub sources: Vec<LockedSource>,
    /// Active skill records.
    pub active_skills: Vec<LockedSkill>,
    /// Managed skills present in the store but not linked into targets.
    pub unlinked_skills: Vec<LockedSkill>,
}

/// Source identity captured in the user lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedSource {
    /// Source ID.
    pub id: String,
    /// Optional commit ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// Skill identity captured in the user lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedSkill {
    /// Source-qualified ref.
    pub source_ref: String,
    /// Slot name.
    pub slot_name: String,
    /// Optional reason when the skill is unlinked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl UserLock {
    /// Empty lock for a new store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: USER_LOCK_SCHEMA_VERSION,
            sources: Vec::new(),
            active_skills: Vec::new(),
            unlinked_skills: Vec::new(),
        }
    }
}
