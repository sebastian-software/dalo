//! Source definitions and source operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Source kind supported by the V1 config schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Private local source in the skillmgr store.
    Local,
    /// Git-backed team source.
    Team,
}

/// Source entry in the user configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Stable source ID.
    pub id: String,
    /// Source kind.
    pub kind: SourceKind,
    /// Local checkout path.
    pub path: PathBuf,
    /// Source priority. Lower numbers win.
    pub priority: i32,
    /// Whether the source participates in resolution.
    pub enabled: bool,
    /// Whether this source is configured as trusted.
    pub trusted: bool,
    /// Optional Git URL for team sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional branch for team sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Update policy, such as `track`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_policy: Option<String>,
}
