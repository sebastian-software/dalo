//! User configuration schema and validation.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::source::{SourceConfig, SourceKind};

/// Current persisted config schema version.
pub const CONFIG_VERSION: u32 = 2;

/// User-authored dalo configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    /// Persisted schema version.
    pub version: u32,
    /// User-level settings.
    pub settings: Settings,
    /// Configured sources in priority order.
    pub sources: Vec<SourceConfig>,
    /// Direct user plugin selections.
    #[serde(default)]
    pub plugins: PluginConfig,
    /// Explicit local plugin policy decisions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugin_policy: Vec<PluginPolicy>,
}

/// Local plugin selection settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginConfig {
    /// Sorted set of canonical `<source-id>:<selector>` references.
    pub direct: Vec<String>,
}

/// Explicit user-local plugin policy decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPolicy {
    /// Policy layer; version 2 accepts only `user_local`.
    pub layer: PluginPolicyLayer,
    /// Stable lower-kebab rule identity.
    pub rule_id: String,
    /// Canonical source-qualified plugin reference.
    pub plugin: String,
    /// Version 2 accepts only decline.
    pub decision: PluginPolicyDecision,
    /// Required human audit context.
    pub reason: String,
}

/// Supported plugin policy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPolicyLayer {
    /// Policy authored in the local user config.
    UserLocal,
}

/// Supported plugin policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPolicyDecision {
    /// Keep intent and origins visible but suppress plugin activation.
    Decline,
}

/// User-level settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
                declared_by: None,
                declared_ref: None,
            }],
            plugins: PluginConfig::default(),
            plugin_policy: Vec::new(),
        }
    }
}
