//! Source definitions and source operations.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditReport};
use crate::catalog::{self, SourceLock};
use crate::config::UserConfig;
use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::materialize::MaterializeOperationKind;
use crate::store::{self, ApprovalsFile, StorePaths};

/// Source kind supported by the V1 config schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Private local source in the dalo store.
    Local,
    /// Git-backed team source.
    Team,
    /// Git-backed catalog source: a multi-skill repository whose skills are
    /// individually selected rather than taken wholesale.
    Catalog,
}

impl SourceKind {
    /// Lowercase label matching the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Team => "team",
            Self::Catalog => "catalog",
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
#[serde(deny_unknown_fields)]
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
    /// Selected skill references for a catalog source. Each entry is a stable
    /// frontmatter ID, a `<source-id>:<slot>` ref, or a slot name. Always empty
    /// for non-catalog sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection: Vec<String>,
    /// Team source whose `dalo.toml` declaration manages this source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<String>,
    /// Revision requested by the declaring team manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_ref: Option<String>,
}

/// Source add report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceAddReport {
    /// Added source.
    pub source: SourceConfig,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
    /// Deterministic preflight reports for every discovered skill.
    pub audits: Vec<AuditReport>,
}

/// Tracking source that could not be refreshed during sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackingSourceRefreshFailure {
    /// Configured source ID.
    pub id: String,
    /// Local checkout path.
    pub path: PathBuf,
    /// Actionable refresh failure detail.
    pub reason: String,
}

/// Source list report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceListReport {
    /// Configured sources.
    pub sources: Vec<SourceListEntry>,
}

/// One source plus read-only provenance assembled from config, lock, and Git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceListEntry {
    /// Existing source configuration fields remain at the top level for JSON
    /// compatibility.
    #[serde(flatten)]
    pub source: SourceConfig,
    /// Origin and pin information.
    pub provenance: SourceProvenance,
}

/// Whether a source is directly configured or owned by a team manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceManagement {
    /// Configured directly in the user's store.
    Direct,
    /// Generated from a team repository's `dalo.toml`.
    TeamManifest,
}

impl SourceManagement {
    /// Stable human-readable and serialized label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::TeamManifest => "team_manifest",
        }
    }
}

/// Read-only source origin and pin information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceProvenance {
    /// Configuration authority.
    pub management: SourceManagement,
    /// Team source that owns this declaration, when manifest-managed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<String>,
    /// Credential-redacted configured origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_url: Option<String>,
    /// Branch or manifest version requested by configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_ref: Option<String>,
    /// Canonical resolved commit from `source-lock.toml` or the team checkout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    /// Commit currently checked out on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_commit: Option<String>,
}

/// Source priority report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourcePriorityReport {
    /// Updated source.
    pub source: SourceConfig,
    /// Whether the priority differs from its previous value.
    pub changed: bool,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// All validated data needed to remove one non-local source safely.
#[derive(Debug, Clone)]
pub struct SourceRemovalPlan {
    /// Configuration before removal.
    pub original_config: UserConfig,
    /// Catalog lock before removal.
    pub original_source_lock: SourceLock,
    /// Approval records before removal.
    pub original_approvals: ApprovalsFile,
    /// Configuration after removal.
    pub config: UserConfig,
    /// Catalog lock after removal.
    pub source_lock: SourceLock,
    /// Approval records after removal.
    pub approvals: ApprovalsFile,
    /// User-facing report before materialized links are added.
    pub report: SourceRemoveReport,
}

/// Result of removing a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRemoveReport {
    /// Removed source ID.
    pub source_id: String,
    /// Checkout path associated with the source.
    pub checkout_path: PathBuf,
    /// Manifest-derived child source IDs removed with a team source.
    pub cascaded_sources: Vec<String>,
    /// Manifest-derived child checkouts removed with a team source.
    pub cascaded_checkout_paths: Vec<PathBuf>,
    /// Whether the checkout was retained at the user's request.
    pub kept_checkout: bool,
    /// Number of source-scoped approval records removed.
    pub removed_approvals: usize,
    /// Whether a catalog lock entry was removed.
    pub removed_catalog_lock: bool,
    /// Owned target links reconciled during removal.
    pub reconciled_links: Vec<SourceRemoveLink>,
    /// Previously active skills deactivated by removing this source.
    pub deactivated_skills: Vec<String>,
    /// Non-fatal checkout cleanup failures after metadata committed.
    pub cleanup_warnings: Vec<String>,
    /// Durable store artifacts that the removal updates or cleans up.
    pub affected_paths: Vec<PathBuf>,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// One owned target link reconciled while removing a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRemoveLink {
    /// Materialization action applied to the link.
    pub kind: MaterializeOperationKind,
    /// Target link path.
    pub path: PathBuf,
}

/// Validate and prepare every durable artifact for source removal.
pub fn plan_remove_source(
    paths: &StorePaths,
    id: &str,
    keep_checkout: bool,
    dry_run: bool,
) -> DaloResult<SourceRemovalPlan> {
    let original_config = store::read_config(paths)?;
    let source = original_config
        .sources
        .iter()
        .find(|source| source.id == id)
        .cloned()
        .ok_or_else(|| {
            DaloError::unknown_source(
                id,
                original_config
                    .sources
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect(),
            )
        })?;
    if let Some(team) = &source.declared_by {
        return Err(DaloError::StateError {
            reason: format!(
                "source `{id}` is managed by `{team}`; edit `{}` in that team repository",
                crate::team_manifest::TEAM_MANIFEST_FILE
            ),
        });
    }
    if source.kind == SourceKind::Local {
        return Err(DaloError::InvalidSourceId {
            id: id.to_owned(),
            reason: "the built-in local source cannot be removed".to_owned(),
        });
    }
    let original_source_lock = catalog::read_source_lock(paths)?;
    let original_approvals = store::read_approvals(paths)?;
    let mut removed_sources = original_config
        .sources
        .iter()
        .filter(|candidate| candidate.id == id || candidate.declared_by.as_deref() == Some(id))
        .cloned()
        .collect::<Vec<_>>();
    removed_sources.sort_by(|left, right| left.id.cmp(&right.id));
    let removed_source_ids = removed_sources
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<BTreeSet<_>>();
    let mut config = original_config.clone();
    config
        .sources
        .retain(|candidate| !removed_source_ids.contains(&candidate.id));
    sort_sources(&mut config.sources);
    let mut source_lock = original_source_lock.clone();
    let before_catalogs = source_lock.catalogs.len();
    source_lock
        .catalogs
        .retain(|entry| !removed_source_ids.contains(&entry.source_id));
    let removed_catalog_lock = source_lock.catalogs.len() != before_catalogs;
    let mut approvals = original_approvals.clone();
    let before_approvals = approvals.approvals.len();
    approvals.approvals.retain(|approval| {
        !removed_source_ids.iter().any(|source_id| {
            approval.value.starts_with(&format!("{source_id}:"))
                || approval.scope == "source" && approval.value == *source_id
        })
    });
    let removed_approvals = before_approvals - approvals.approvals.len();

    let mut affected_paths = vec![
        paths.config_file.clone(),
        paths.source_lock_file.clone(),
        paths.approvals_file.clone(),
        paths.lock_file.clone(),
        paths.state_file.clone(),
    ];
    let cascaded = removed_sources
        .iter()
        .filter(|candidate| candidate.id != id)
        .cloned()
        .collect::<Vec<_>>();
    if !keep_checkout {
        affected_paths.extend(removed_sources.iter().map(|candidate| {
            candidate
                .path
                .parent()
                .map_or_else(|| candidate.path.clone(), Path::to_path_buf)
        }));
    }

    Ok(SourceRemovalPlan {
        original_config,
        original_source_lock,
        original_approvals,
        config,
        source_lock,
        approvals,
        report: SourceRemoveReport {
            source_id: id.to_owned(),
            checkout_path: source.path,
            cascaded_sources: cascaded.iter().map(|source| source.id.clone()).collect(),
            cascaded_checkout_paths: cascaded.iter().map(|source| source.path.clone()).collect(),
            kept_checkout: keep_checkout,
            removed_approvals,
            removed_catalog_lock,
            reconciled_links: Vec::new(),
            deactivated_skills: Vec::new(),
            cleanup_warnings: Vec::new(),
            affected_paths,
            dry_run,
        },
    })
}

/// Sort source configs by precedence and then stable ID.
pub fn sort_sources(sources: &mut [SourceConfig]) {
    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
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
    add_team_source_with_config_writer(paths, id, url, dry_run, store::write_config)
}

/// Resolve a local source location against the caller's working directory.
///
/// Git URL and SCP-style remote syntax is kept verbatim unless it names an
/// existing local path. Everything else that is relative is made absolute
/// before `git clone` changes its working directory to the store checkout
/// parent.
#[must_use]
pub fn resolve_source_location(location: &str, cwd: &Path) -> String {
    let path = Path::new(location);
    if path.is_absolute() {
        return location.to_owned();
    }

    let local_path = normalize_local_path(&cwd.join(path));
    if looks_like_remote_location(location) && !local_path.exists() {
        return location.to_owned();
    }

    local_path.to_string_lossy().into_owned()
}

fn normalize_local_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push("..");
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn looks_like_remote_location(location: &str) -> bool {
    if location.contains("://") {
        return true;
    }
    let Some(colon) = location.find(':') else {
        return false;
    };
    colon > 0
        && !location[..colon].contains('/')
        && location
            .get(colon + 1..)
            .is_some_and(|suffix| !suffix.is_empty())
}

fn add_team_source_with_config_writer<F>(
    paths: &StorePaths,
    id: &str,
    url: &str,
    dry_run: bool,
    write_config: F,
) -> DaloResult<SourceAddReport>
where
    F: FnOnce(&StorePaths, &UserConfig) -> DaloResult<()>,
{
    add_team_source_with_config_writer_and_cloner(
        paths,
        id,
        url,
        dry_run,
        write_config,
        git::clone_repo,
    )
}

fn add_team_source_with_config_writer_and_cloner<F, C>(
    paths: &StorePaths,
    id: &str,
    url: &str,
    dry_run: bool,
    write_config: F,
    clone_repo: C,
) -> DaloResult<SourceAddReport>
where
    F: FnOnce(&StorePaths, &UserConfig) -> DaloResult<()>,
    C: FnOnce(&str, &std::path::Path) -> DaloResult<()>,
{
    // Validate the id before anything touches the store: it is joined straight
    // into the checkout path and `git clone`d there, so an id like `../../evil`
    // or `a/b` would escape `sources/` to an attacker-chosen location.
    if !is_valid_source_id(id) {
        return Err(DaloError::InvalidSourceId {
            id: id.to_owned(),
            reason: "must be non-empty, not `.`/`..`, and only contain `[A-Za-z0-9._-]`".to_owned(),
        });
    }
    git::validate_remote_url(url)?;

    let mut config = store::read_config(paths)?;
    if config.sources.iter().any(|source| source.id == id) {
        return Err(DaloError::SourceAlreadyExists {
            source_id: id.to_owned(),
        });
    }

    let checkout = paths.sources_dir.join(id).join("checkout");
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
        selection: Vec::new(),
        declared_by: None,
        declared_ref: None,
    };

    if dry_run {
        return Ok(SourceAddReport {
            source,
            dry_run: true,
            audits: Vec::new(),
        });
    }

    clone_source_checkout_with(url, &checkout, clone_repo)?;

    let audits = audit_source_checkout(paths, id, &checkout).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&checkout);
        remove_empty_source_dir(&checkout);
    })?;

    // From here on the checkout exists on disk. If persisting the source fails,
    // remove the clone so a later `source add` does not trip over an orphaned
    // checkout that is absent from config.
    finish_team_source(paths, &mut config, source.clone(), write_config).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&checkout);
        remove_empty_source_dir(&checkout);
    })?;

    Ok(SourceAddReport {
        source,
        dry_run: false,
        audits,
    })
}

fn audit_source_checkout(
    paths: &StorePaths,
    source_id: &str,
    checkout: &Path,
) -> DaloResult<Vec<AuditReport>> {
    let inventory = crate::inventory::scan_source(source_id, checkout)?;
    inventory
        .skills
        .iter()
        .map(|skill| {
            audit::audit_skill(
                paths,
                &skill.source_ref,
                &skill.path,
                &audit::AuditOptions {
                    exclude_root_source_metadata: store::comparable_path(&skill.path)
                        == store::comparable_path(checkout),
                    ..audit::AuditOptions::default()
                },
            )
        })
        .collect()
}

/// Clone a source through a temporary sibling and atomically publish it.
///
/// The caller must hold the store lock. Interrupted-clone directories are safe
/// to remove because no configured source ever points at them.
pub fn clone_source_checkout(url: &str, checkout: &Path) -> DaloResult<()> {
    clone_source_checkout_with(url, checkout, git::clone_repo)
}

fn clone_source_checkout_with<C>(url: &str, checkout: &Path, clone_repo: C) -> DaloResult<()>
where
    C: FnOnce(&str, &Path) -> DaloResult<()>,
{
    // Legacy interrupted clones were written directly to `checkout`. Only
    // remove an obviously incomplete directory. A Git checkout can contain
    // user work, so it always requires an explicit recovery decision.
    if checkout.exists() {
        if checkout.join(".git").exists() {
            return Err(DaloError::SourceCheckoutExists {
                path: checkout.to_path_buf(),
                reason: "restore its source config or move/remove the checkout before retrying"
                    .to_owned(),
            });
        }
        std::fs::remove_dir_all(checkout)?;
    }

    let Some(parent) = checkout.parent() else {
        return Err(DaloError::InvalidStorePath {
            path: checkout.to_path_buf(),
            reason: "source checkout has no parent directory".to_owned(),
        });
    };
    std::fs::create_dir_all(parent)?;
    remove_interrupted_clone_dirs(parent)?;

    let temporary_checkout =
        checkout.with_file_name(format!(".checkout-tmp-{}", std::process::id()));
    clone_repo(url, &temporary_checkout).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&temporary_checkout);
        remove_empty_source_dir(checkout);
    })?;
    std::fs::rename(&temporary_checkout, checkout).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&temporary_checkout);
        remove_empty_source_dir(checkout);
    })?;
    Ok(())
}

fn remove_interrupted_clone_dirs(parent: &Path) -> DaloResult<()> {
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".checkout-tmp-")
        {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn remove_empty_source_dir(checkout: &Path) {
    if let Some(parent) = checkout.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

/// Return whether a source ID is safe to use as a store path component.
///
/// A source ID becomes a single directory below `sources/` and feeds approval
/// records and resolver output, so it is limited to a conservative path token:
/// non-empty, never the `.`/`..` traversal segments, and limited to
/// `[A-Za-z0-9._-]` (no `/` path separators). Skill slot names are stricter
/// because they are materialized into user-facing target directories.
#[must_use]
pub fn is_valid_source_id(value: &str) -> bool {
    if value.is_empty() || value == "." || value == ".." {
        return false;
    }

    value.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || character == '-'
            || character == '_'
            || character == '.'
    })
}

/// Persist a freshly cloned team source into config.
///
/// User-added sources are `trusted: true`, and the resolver treats trusted
/// sources as approved, so no approval record is needed. If the config write
/// fails, the caller removes the clone so `config.toml` never references a
/// checkout that does not exist.
fn finish_team_source(
    paths: &StorePaths,
    config: &mut UserConfig,
    source: SourceConfig,
    write_config: impl FnOnce(&StorePaths, &UserConfig) -> DaloResult<()>,
) -> DaloResult<()> {
    config.sources.push(source);
    sort_sources(&mut config.sources);
    write_config(paths, config)?;
    Ok(())
}

/// List configured sources.
#[must_use = "the source list report should be rendered or inspected"]
pub fn list_sources(paths: &StorePaths) -> DaloResult<SourceListReport> {
    let mut sources = store::read_config(paths)?.sources;
    sort_sources(&mut sources);
    let source_lock = catalog::read_source_lock(paths).ok();
    let sources = sources
        .into_iter()
        .map(|source| SourceListEntry {
            provenance: source_provenance(&source, source_lock.as_ref()),
            source,
        })
        .collect();
    Ok(SourceListReport { sources })
}

/// Assemble stable provenance without mutating or fetching a source.
#[must_use]
pub fn source_provenance(
    source: &SourceConfig,
    source_lock: Option<&SourceLock>,
) -> SourceProvenance {
    let checkout_commit = (source.kind != SourceKind::Local)
        .then(|| git::rev_parse_head(&source.path).ok())
        .flatten();
    let resolved_commit = match source.kind {
        SourceKind::Catalog => source_lock
            .and_then(|lock| lock.catalog(&source.id))
            .map(|lock| lock.commit.clone()),
        SourceKind::Team => checkout_commit.clone(),
        SourceKind::Local => None,
    };
    SourceProvenance {
        management: if source.declared_by.is_some() {
            SourceManagement::TeamManifest
        } else {
            SourceManagement::Direct
        },
        declared_by: source.declared_by.clone(),
        origin_url: source.url.as_deref().map(git::display_remote_url),
        requested_ref: source
            .declared_ref
            .clone()
            .or_else(|| source.branch.clone()),
        resolved_commit,
        checkout_commit,
    }
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
        return Err(DaloError::unknown_source(
            id,
            config
                .sources
                .iter()
                .map(|candidate| candidate.id.clone())
                .collect(),
        ));
    };
    if let Some(team) = &source.declared_by {
        return Err(DaloError::StateError {
            reason: format!(
                "source `{id}` is managed by `{team}`; edit `{}` in that team repository",
                crate::team_manifest::TEAM_MANIFEST_FILE
            ),
        });
    }
    // The local source is the guaranteed override (priority 0); refuse to move it,
    // otherwise a team skill could shadow a locally adapted one.
    if source.kind == SourceKind::Local {
        return Err(DaloError::LocalSourcePriorityFixed {
            source_id: id.to_owned(),
        });
    }
    let changed = source.priority != priority;
    source.priority = priority;
    let source = source.clone();

    if changed && !dry_run {
        sort_sources(&mut config.sources);
        store::write_config(paths, &config)?;
    }

    Ok(SourcePriorityReport {
        source,
        changed,
        dry_run,
    })
}

/// Refresh clean tracking team sources before sync.
///
/// Missing or unknown update policies are treated as pinned and are not pulled.
pub fn refresh_tracking_team_sources(
    paths: &StorePaths,
) -> DaloResult<Vec<TrackingSourceRefreshFailure>> {
    let config = store::read_config(paths)?;
    refresh_tracking_team_sources_from_config(paths, &config)
}

/// Refresh clean tracking team sources from an already-read config.
///
/// Missing or unknown update policies are treated as pinned and are not pulled.
pub fn refresh_tracking_team_sources_from_config(
    paths: &StorePaths,
    config: &UserConfig,
) -> DaloResult<Vec<TrackingSourceRefreshFailure>> {
    let mut failures = Vec::new();
    for source in config.sources.iter().filter(|source| {
        source.enabled
            && source.kind == SourceKind::Team
            && source.update_policy.as_deref() == Some("track")
    }) {
        match git::is_dirty(&source.path) {
            Ok(true) => {
                return Err(DaloError::DirtySource {
                    source_id: source.id.clone(),
                    path: source.path.clone(),
                });
            }
            Ok(false) => {}
            Err(error) => {
                failures.push(TrackingSourceRefreshFailure {
                    id: source.id.clone(),
                    path: source.path.clone(),
                    reason: format!("could not inspect tracking checkout: {error}"),
                });
                continue;
            }
        }
        if let Err(error) = stage_audit_and_fast_forward(paths, source) {
            if matches!(error, DaloError::AuditBlocked { .. }) {
                return Err(error);
            }
            failures.push(TrackingSourceRefreshFailure {
                id: source.id.clone(),
                path: source.path.clone(),
                reason: format!("could not refresh tracking source: {error}"),
            });
        }
    }
    Ok(failures)
}

fn stage_audit_and_fast_forward(paths: &StorePaths, source: &SourceConfig) -> DaloResult<()> {
    git::fetch_upstream(&source.path)?;
    let upstream = git::rev_parse(&source.path, "@{upstream}")?;
    let incoming = git::revision_count(&source.path, "HEAD", &upstream)?;
    if incoming == 0 {
        return Ok(());
    }
    if git::revision_count(&source.path, &upstream, "HEAD")? != 0 {
        return Err(DaloError::TrackingSourceNotFastForward {
            source_id: source.id.clone(),
            path: source.path.clone(),
        });
    }

    let staging_root = paths.sources_dir.join(".audit-staging");
    fs::create_dir_all(&staging_root)?;
    let staging_path = staging_root.join(format!("{}-{upstream}", source.id));
    cleanup_obsolete_staging_worktrees(source, &staging_root, &staging_path)?;
    let staging_matches = staging_path.exists()
        && git::rev_parse_head(&staging_path).is_ok_and(|commit| commit == upstream);
    if !staging_matches {
        if staging_path.exists() {
            let _ = git::remove_worktree(&source.path, &staging_path);
            let _ = fs::remove_dir_all(&staging_path);
            git::prune_worktrees(&source.path)?;
        }
        git::add_detached_worktree(&source.path, &staging_path, &upstream)?;
    }

    let audit_result = (|| -> DaloResult<()> {
        let inventory = crate::inventory::scan_source(&source.id, &staging_path)?;
        let mut blocked = Vec::new();
        for skill in inventory.skills {
            let report = audit::audit_skill(
                paths,
                &skill.source_ref,
                &skill.path,
                &audit::AuditOptions {
                    exclude_root_source_metadata: store::comparable_path(&skill.path)
                        == store::comparable_path(&staging_path),
                    ..audit::AuditOptions::default()
                },
            )?;
            if report.is_blocking() {
                blocked.push((skill.source_ref, skill.path));
            }
        }
        if blocked.is_empty() {
            Ok(())
        } else {
            Err(DaloError::AuditBlocked {
                reason: format!(
                    "staged security audit blocked upstream commit {upstream} ({})",
                    blocked
                        .iter()
                        .map(|(source_ref, path)| format!(
                            "{source_ref}; review with `dalo audit '{}' --accept-risk <reason>`",
                            path.display()
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
        }
    })();
    if let Err(error) = audit_result {
        if matches!(error, DaloError::AuditBlocked { .. }) {
            // Keep the immutable staged worktree so the user can inspect and
            // explicitly accept this exact hash. The next sync reuses it.
            return Err(error);
        }
        if let Err(cleanup) = git::remove_worktree(&source.path, &staging_path) {
            return Err(DaloError::Io(std::io::Error::other(format!(
                "{error}; additionally failed to remove staged audit worktree: {cleanup}"
            ))));
        }
        return Err(error);
    }
    git::remove_worktree(&source.path, &staging_path)?;
    let _ = fs::remove_dir(&staging_root);
    git::fast_forward_to(&source.path, &upstream)
}

fn cleanup_obsolete_staging_worktrees(
    source: &SourceConfig,
    staging_root: &Path,
    keep: &Path,
) -> DaloResult<()> {
    for entry in fs::read_dir(staging_root)? {
        let entry = entry?;
        let path = entry.path();
        if path == keep || !staging_entry_belongs_to_source(&entry.file_name(), &source.id) {
            continue;
        }
        if git::remove_worktree(&source.path, &path).is_err() {
            fs::remove_dir_all(&path)?;
        }
    }
    git::prune_worktrees(&source.path)
}

pub(crate) fn staging_entry_belongs_to_source(name: &std::ffi::OsStr, source_id: &str) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(commit) = name
        .strip_prefix(source_id)
        .and_then(|suffix| suffix.strip_prefix('-'))
    else {
        return false;
    };
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
fn refresh_tracking_team_sources_with<D, P>(
    config: &UserConfig,
    mut is_dirty: D,
    mut pull_ff_only: P,
) -> DaloResult<Vec<TrackingSourceRefreshFailure>>
where
    D: FnMut(&Path) -> DaloResult<bool>,
    P: FnMut(&Path) -> DaloResult<()>,
{
    let mut failures = Vec::new();
    for source in config.sources.iter().filter(|source| {
        source.enabled
            && source.kind == SourceKind::Team
            && source.update_policy.as_deref() == Some("track")
    }) {
        match is_dirty(&source.path) {
            Ok(true) => {
                return Err(DaloError::DirtySource {
                    source_id: source.id.clone(),
                    path: source.path.clone(),
                });
            }
            Ok(false) => {}
            Err(error) => {
                failures.push(TrackingSourceRefreshFailure {
                    id: source.id.clone(),
                    path: source.path.clone(),
                    reason: format!("could not inspect tracking checkout: {error}"),
                });
                continue;
            }
        }
        if let Err(error) = pull_ff_only(&source.path) {
            failures.push(TrackingSourceRefreshFailure {
                id: source.id.clone(),
                path: source.path.clone(),
                reason: format!("could not refresh tracking source: {error}"),
            });
        }
    }

    Ok(failures)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use proptest::prelude::*;

    #[test]
    fn is_valid_source_id_should_reject_traversal_and_slash_ids() {
        assert!(!is_valid_source_id(".."));
        assert!(!is_valid_source_id("a/b"));
        assert!(!is_valid_source_id(""));
    }

    #[test]
    fn is_valid_source_id_should_accept_plain_id() {
        assert!(is_valid_source_id("company"));
    }

    #[test]
    fn staging_entry_match_should_disambiguate_dash_prefixed_source_ids() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let sha256_commit = format!("{commit}0123456789abcdef01234567");
        assert!(staging_entry_belongs_to_source(
            std::ffi::OsStr::new(&format!("team-{commit}")),
            "team"
        ));
        assert!(!staging_entry_belongs_to_source(
            std::ffi::OsStr::new(&format!("team-eu-{commit}")),
            "team"
        ));
        assert!(staging_entry_belongs_to_source(
            std::ffi::OsStr::new(&format!("team-eu-{commit}")),
            "team-eu"
        ));
        assert!(staging_entry_belongs_to_source(
            std::ffi::OsStr::new(&format!("team-{sha256_commit}")),
            "team"
        ));
        assert!(!staging_entry_belongs_to_source(
            std::ffi::OsStr::new("team-not-a-commit"),
            "team"
        ));
    }

    proptest! {
        #[test]
        fn valid_source_ids_should_stay_single_path_components(value in "\\PC{0,64}") {
            if is_valid_source_id(&value) {
                prop_assert!(!value.is_empty());
                prop_assert_ne!(value.as_str(), ".");
                prop_assert_ne!(value.as_str(), "..");
                prop_assert!(!value.contains('/'));
                let portable = value.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || character == '-'
                        || character == '_'
                        || character == '.'
                });
                prop_assert!(portable);
            }
        }
    }

    #[test]
    fn add_team_source_should_reject_traversal_id_without_cloning() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);

        let error = add_team_source(
            &paths,
            "../../evil",
            "https://example.invalid/repo.git",
            false,
        )
        .expect_err("traversal id should be rejected");

        assert!(matches!(error, DaloError::InvalidSourceId { .. }));
        // The id must be rejected before any clone target is touched, so nothing
        // may have been written below (or alongside) the sources directory.
        assert!(
            !paths
                .sources_dir
                .join("../../evil")
                .join("checkout")
                .exists()
        );
        assert!(
            std::fs::read_dir(&paths.sources_dir)
                .expect("sources dir should exist")
                .next()
                .is_none()
        );
    }

    #[test]
    fn add_team_source_should_reject_url_userinfo_before_cloning_or_persisting() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);

        let error = add_team_source(
            &paths,
            "company",
            "https://octo:token-value@example.invalid/repo.git",
            false,
        )
        .expect_err("credential-bearing URL should be rejected");

        assert!(matches!(error, DaloError::UnsafeRemoteUrl));
        assert!(!paths.sources_dir.join("company").exists());
        let config = store::read_config(&paths).expect("config should remain readable");
        assert!(!config.sources.iter().any(|source| source.id == "company"));
    }

    #[test]
    fn add_team_source_should_remove_checkout_when_persisting_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let repo = temp_dir.path().join("team-repo");
        create_git_repo(&repo);
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let checkout = paths.sources_dir.join("company").join("checkout");

        let error = add_team_source_with_config_writer(
            &paths,
            "company",
            &repo.to_string_lossy(),
            false,
            |_, _| Err(DaloError::Io(std::io::Error::other("persist failed"))),
        )
        .expect_err("add should fail when config cannot be recorded");

        assert!(matches!(error, DaloError::Io(_)));
        // The orphaned checkout must be gone so a later `source add` is not blocked
        // by a stale checkout, and config must not reference the half-added source.
        assert!(!checkout.exists());
        let config = store::read_config(&paths).expect("config should remain readable");
        assert!(!config.sources.iter().any(|source| source.id == "company"));
    }

    #[test]
    fn add_team_source_should_remove_checkout_when_clone_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let checkout = paths.sources_dir.join("company").join("checkout");

        let error = add_team_source_with_config_writer_and_cloner(
            &paths,
            "company",
            "https://example.invalid/repo.git",
            false,
            store::write_config,
            |_, checkout| {
                std::fs::create_dir_all(checkout.join(".git"))?;
                std::fs::write(checkout.join("PARTIAL"), "left by clone")?;
                Err(DaloError::CommandFailed {
                    program: "git".to_owned(),
                    args: "clone".to_owned(),
                    cwd: checkout
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .to_path_buf(),
                    status: "timed out after 1s".to_owned(),
                    stderr: "git command timed out".to_owned(),
                })
            },
        )
        .expect_err("clone failure should fail source add");

        assert!(matches!(error, DaloError::CommandFailed { .. }));
        assert!(!checkout.exists());
        let config = store::read_config(&paths).expect("config should remain readable");
        assert!(!config.sources.iter().any(|source| source.id == "company"));
    }

    #[test]
    fn add_team_source_should_replace_an_orphaned_legacy_checkout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let repo = temp_dir.path().join("team-repo");
        create_git_repo(&repo);
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let checkout = paths.sources_dir.join("company").join("checkout");
        std::fs::create_dir_all(&checkout).expect("legacy checkout should be created");
        std::fs::write(checkout.join("PARTIAL"), "interrupted clone")
            .expect("legacy marker should be written");

        let report = add_team_source(&paths, "company", &repo.to_string_lossy(), false)
            .expect("orphaned checkout should not block a retry");

        assert!(report.source.path.join(".git").is_dir());
        assert!(!report.source.path.join("PARTIAL").exists());
    }

    #[test]
    fn add_team_source_should_not_remove_an_unconfigured_git_checkout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let checkout = paths.sources_dir.join("company").join("checkout");
        std::fs::create_dir_all(checkout.join(".git")).expect("git checkout should be created");
        std::fs::write(checkout.join("LOCAL"), "keep this work")
            .expect("local work marker should be written");

        let error = add_team_source_with_config_writer_and_cloner(
            &paths,
            "company",
            "https://example.invalid/company.git",
            false,
            store::write_config,
            |_, _| panic!("a preserved checkout must not be replaced"),
        )
        .expect_err("unconfigured Git checkout should require explicit recovery");

        assert!(matches!(error, DaloError::SourceCheckoutExists { .. }));
        assert!(checkout.join("LOCAL").is_file());
        assert!(error.to_string().contains("restore its source config"));
    }

    #[test]
    fn resolve_source_location_should_absolutize_local_paths_and_preserve_remotes() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let cwd = temp_dir.path().join("project");
        let colon_path = cwd.join("team:skills");
        std::fs::create_dir_all(&colon_path).expect("local colon path should be created");

        assert_eq!(resolve_source_location(".", &cwd), cwd.to_string_lossy());
        assert_eq!(
            resolve_source_location("../skills", &cwd),
            temp_dir.path().join("skills").to_string_lossy()
        );
        assert_eq!(
            resolve_source_location("team:skills", &cwd),
            colon_path.to_string_lossy()
        );
        assert_eq!(
            resolve_source_location("https://example.invalid/repo.git", &cwd),
            "https://example.invalid/repo.git"
        );
        assert_eq!(
            resolve_source_location("git@example.invalid:org/repo.git", &cwd),
            "git@example.invalid:org/repo.git"
        );
    }

    #[test]
    fn clone_source_checkout_should_sweep_interrupted_clone_dirs() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let repo = temp_dir.path().join("team-repo");
        create_git_repo(&repo);
        let source_dir = temp_dir.path().join("store/sources/company");
        let checkout = source_dir.join("checkout");
        for name in [".checkout-tmp-111", ".checkout-tmp-222"] {
            std::fs::create_dir_all(source_dir.join(name))
                .expect("interrupted clone dir should be created");
        }

        clone_source_checkout(&repo.to_string_lossy(), &checkout)
            .expect("source clone should succeed");

        assert!(checkout.join(".git").is_dir());
        assert!(!source_dir.join(".checkout-tmp-111").exists());
        assert!(!source_dir.join(".checkout-tmp-222").exists());
    }

    #[test]
    fn refresh_tracking_sources_should_degrade_pull_failures_but_keep_going() {
        let source_path = PathBuf::from("/store/sources/company/checkout");
        let config = UserConfig {
            version: 1,
            settings: crate::config::Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "company".to_owned(),
                kind: SourceKind::Team,
                path: source_path.clone(),
                priority: 10,
                enabled: true,
                trusted: true,
                url: Some("https://example.invalid/company.git".to_owned()),
                branch: None,
                update_policy: Some("track".to_owned()),
                selection: Vec::new(),
                declared_by: None,
                declared_ref: None,
            }],
        };

        let failures = refresh_tracking_team_sources_with(
            &config,
            |_| Ok(false),
            |_| Err(DaloError::Io(std::io::Error::other("network unavailable"))),
        )
        .expect("an unavailable remote should be represented as a degraded source");

        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].id, "company");
        assert_eq!(failures[0].path, source_path);
        assert!(failures[0].reason.contains("network unavailable"));
    }

    #[test]
    fn refresh_tracking_sources_should_still_stop_for_a_dirty_checkout() {
        let config = UserConfig {
            version: 1,
            settings: crate::config::Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "company".to_owned(),
                kind: SourceKind::Team,
                path: PathBuf::from("/store/sources/company/checkout"),
                priority: 10,
                enabled: true,
                trusted: true,
                url: Some("https://example.invalid/company.git".to_owned()),
                branch: None,
                update_policy: Some("track".to_owned()),
                selection: Vec::new(),
                declared_by: None,
                declared_ref: None,
            }],
        };

        let error = refresh_tracking_team_sources_with(&config, |_| Ok(true), |_| Ok(()))
            .expect_err("dirty checkouts must still block sync");

        assert!(matches!(error, DaloError::DirtySource { .. }));
    }

    #[test]
    fn source_provenance_should_distinguish_manifest_management_and_redact_origin() {
        let source = SourceConfig {
            id: "company.marketing".to_owned(),
            kind: SourceKind::Catalog,
            path: PathBuf::from("/missing/checkout"),
            priority: 11,
            enabled: true,
            trusted: false,
            url: Some("https://user:secret@example.com/marketing.git".to_owned()),
            branch: None,
            update_policy: Some("manifest".to_owned()),
            selection: vec!["copy".to_owned()],
            declared_by: Some("company".to_owned()),
            declared_ref: Some("main".to_owned()),
        };
        let lock = SourceLock {
            schema_version: catalog::SOURCE_LOCK_SCHEMA_VERSION,
            catalogs: vec![catalog::CatalogLock {
                source_id: source.id.clone(),
                commit: "0123456789abcdef".to_owned(),
                selected: Vec::new(),
                inventory: Vec::new(),
            }],
        };

        let provenance = source_provenance(&source, Some(&lock));

        assert_eq!(provenance.management, SourceManagement::TeamManifest);
        assert_eq!(provenance.declared_by.as_deref(), Some("company"));
        assert_eq!(
            provenance.origin_url.as_deref(),
            Some("https://***@example.com/marketing.git")
        );
        let origin = provenance
            .origin_url
            .as_deref()
            .expect("origin should exist");
        assert!(!origin.contains("user"));
        assert!(!origin.contains("secret"));
        assert_eq!(provenance.requested_ref.as_deref(), Some("main"));
        assert_eq!(
            provenance.resolved_commit.as_deref(),
            Some("0123456789abcdef")
        );
        assert!(provenance.checkout_commit.is_none());
    }

    #[test]
    fn add_team_source_should_persist_source_without_leftovers_on_success() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let repo = temp_dir.path().join("team-repo");
        create_git_repo(&repo);
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);

        let report = add_team_source(&paths, "company", &repo.to_string_lossy(), false)
            .expect("add should succeed against a local repo");

        // The checkout exists and a second add of the same id is rejected as a
        // clean duplicate rather than tripping over a stale checkout.
        assert!(report.source.path.join(".git").is_dir());
        let second = add_team_source(&paths, "company", &repo.to_string_lossy(), false)
            .expect_err("a second add of the same id should be a clean duplicate error");
        assert!(matches!(second, DaloError::SourceAlreadyExists { .. }));
    }

    fn create_git_repo(repo: &Path) {
        std::fs::create_dir_all(repo).expect("repo dir should be created");
        run_git(repo, &["init", "-q"]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("README.md"), "# Repo\n").expect("file should be written");
        run_git(repo, &["add", "."]);
        run_git(
            repo,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "initial",
            ],
        );
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(repo)
            .args(args)
            .status()
            .expect("git should run");
        assert!(status.success(), "git {args:?} should succeed");
    }
}
