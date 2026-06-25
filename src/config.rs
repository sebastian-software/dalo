//! User configuration schema and validation.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::source::{SourceConfig, SourceKind};

/// Current persisted config schema version.
pub const CONFIG_VERSION: u32 = 1;

/// User-authored dalo configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserConfig {
    /// Persisted schema version.
    pub version: u32,
    /// User-level settings.
    pub settings: Settings,
    /// Configured sources in priority order.
    pub sources: Vec<SourceConfig>,
}

/// User-level settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Whether scheduled autosync is enabled.
    pub autosync: bool,
    /// Optional sync interval label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_interval: Option<String>,
}

impl UserConfig {
    /// Build the default config for a newly initialized store.
    #[must_use]
    pub fn default_for_store(store_root: &Path) -> Self {
        let local_path = store_root.join("local");

        Self {
            version: CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "local".to_owned(),
                kind: SourceKind::Local,
                path: local_path,
                priority: 0,
                enabled: true,
                trusted: true,
                url: None,
                branch: None,
                update_policy: None,
                selection: Vec::new(),
            }],
        }
    }
}
