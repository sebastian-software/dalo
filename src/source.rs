//! Source definitions and source operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::store::{self, ApprovalRecord, StorePaths};

/// Source kind supported by the V1 config schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Private local source in the dalo store.
    Local,
    /// Git-backed team source.
    Team,
}

impl SourceKind {
    /// Lowercase label matching the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Team => "team",
        }
    }
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
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

/// Source add report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceAddReport {
    /// Added source.
    pub source: SourceConfig,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// Source list report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceListReport {
    /// Configured sources.
    pub sources: Vec<SourceConfig>,
}

/// Source priority report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourcePriorityReport {
    /// Updated source.
    pub source: SourceConfig,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// Add a team source and clone it into the store.
///
/// When `dry_run` is set, the planned source is computed and returned without
/// cloning, writing config, or recording an approval.
pub fn add_team_source(
    paths: &StorePaths,
    id: &str,
    url: &str,
    dry_run: bool,
) -> DaloResult<SourceAddReport> {
    let mut config = store::read_config(paths)?;
    if config.sources.iter().any(|source| source.id == id) {
        return Err(DaloError::SourceAlreadyExists {
            source_id: id.to_owned(),
        });
    }

    let checkout = paths.sources_dir.join(id).join("checkout");
    if checkout.exists() {
        return Err(DaloError::InvalidStorePath {
            path: checkout,
            reason: "source checkout path already exists".to_owned(),
        });
    }

    let priority = config
        .sources
        .iter()
        .map(|source| source.priority)
        .max()
        .unwrap_or(0)
        + 10;
    let source = SourceConfig {
        id: id.to_owned(),
        kind: SourceKind::Team,
        path: checkout.clone(),
        priority,
        enabled: true,
        trusted: true,
        url: Some(url.to_owned()),
        branch: None,
        update_policy: Some("track".to_owned()),
    };

    if dry_run {
        return Ok(SourceAddReport {
            source,
            dry_run: true,
        });
    }

    if let Some(parent) = checkout.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git::clone_repo(url, &checkout)?;
    config.sources.push(source.clone());
    config.sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    store::write_config(paths, &config)?;
    approve_added_source(paths, id)?;

    Ok(SourceAddReport {
        source,
        dry_run: false,
    })
}

/// List configured sources.
pub fn list_sources(paths: &StorePaths) -> DaloResult<SourceListReport> {
    let mut sources = store::read_config(paths)?.sources;
    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(SourceListReport { sources })
}

/// Update source priority.
///
/// When `dry_run` is set, the updated source is returned without writing config.
pub fn set_source_priority(
    paths: &StorePaths,
    id: &str,
    priority: i32,
    dry_run: bool,
) -> DaloResult<SourcePriorityReport> {
    let mut config = store::read_config(paths)?;
    let Some(source) = config.sources.iter_mut().find(|source| source.id == id) else {
        return Err(DaloError::UnknownSource {
            source_id: id.to_owned(),
        });
    };
    // The local source is the guaranteed override (priority 0); refuse to move it,
    // otherwise a team skill could shadow a locally adapted one.
    if source.kind == SourceKind::Local {
        return Err(DaloError::LocalSourcePriorityFixed {
            source_id: id.to_owned(),
        });
    }
    source.priority = priority;
    let source = source.clone();

    if !dry_run {
        config.sources.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.id.cmp(&right.id))
        });
        store::write_config(paths, &config)?;
    }

    Ok(SourcePriorityReport { source, dry_run })
}

/// Refresh clean tracking team sources before sync.
pub fn refresh_tracking_team_sources(paths: &StorePaths) -> DaloResult<()> {
    let config = store::read_config(paths)?;
    for source in config
        .sources
        .iter()
        .filter(|source| source.enabled && source.kind == SourceKind::Team)
    {
        if git::is_dirty(&source.path)? {
            return Err(DaloError::DirtySource {
                source_id: source.id.clone(),
            });
        }
        git::pull_ff_only(&source.path)?;
    }

    Ok(())
}

fn approve_added_source(paths: &StorePaths, id: &str) -> DaloResult<()> {
    let mut approvals = store::read_approvals(paths)?;
    if !approvals
        .approvals
        .iter()
        .any(|approval| approval.scope == "source" && approval.value == id)
    {
        approvals.approvals.push(ApprovalRecord {
            scope: "source".to_owned(),
            value: id.to_owned(),
        });
        approvals.approvals.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then_with(|| left.value.cmp(&right.value))
        });
        store::write_approvals(paths, &approvals)?;
    }

    Ok(())
}
