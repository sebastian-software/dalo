//! Team-owned composition manifests.
//!
//! A tracking team source may contain `dalo.toml`. The manifest turns pinned
//! external catalogs into ordinary resolver inputs while keeping execution
//! approval in each user's local store.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditOptions, AuditReport};
use crate::catalog::{self, CatalogLock, DriftCode, DriftEntry};
use crate::config::UserConfig;
use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::inventory::{self, SkillRecord};
use crate::source::{self, SourceConfig, SourceKind};
use crate::store::{self, StorePaths};

/// Team manifest filename at the root of a team source.
pub const TEAM_MANIFEST_FILE: &str = "dalo.toml";
/// Current team manifest schema version.
pub const TEAM_MANIFEST_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    TEAM_MANIFEST_SCHEMA_VERSION
}

/// Human-authored team composition manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamManifest {
    /// Manifest schema version. Omitted manifests are interpreted as v1 for
    /// compatibility with the original RFC example.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Optional descriptive source metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ManifestSource>,
    /// External multi-skill repositories composed into this team source.
    #[serde(default, rename = "catalog")]
    pub catalogs: Vec<ManifestCatalog>,
}

/// Optional descriptive metadata retained for RFC compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSource {
    /// Expected source ID, when the repository wants to assert it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Human-readable source name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Descriptive source kind; currently informational.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// One pinned external catalog declared by a team source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestCatalog {
    /// ID local to the team manifest. The persisted source is namespaced with
    /// the declaring team source ID.
    pub id: String,
    /// Git clone URL or a local path relative to the team checkout.
    pub url: String,
    /// Git commit, tag, or ref to pin. `ref` is accepted as an RFC-compatible
    /// alias; `version` is the preferred user-facing spelling.
    #[serde(alias = "ref")]
    pub version: String,
    /// Include/exclude filters. Empty means all. If any `+` or bare entry
    /// exists, the base set is empty; otherwise the base set is all. `-`
    /// always wins.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Optional global resolver priority. By default the catalog follows its
    /// declaring team source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

/// Team-manifest management action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamManifestAction {
    /// Created a new manifest.
    Initialized,
    /// Existing matching manifest needed no change.
    Unchanged,
    /// Added one catalog declaration.
    CatalogAdded,
    /// Replaced one catalog's skill filters.
    CatalogSkillsUpdated,
    /// Changed one catalog's requested version.
    CatalogVersionUpdated,
    /// Removed one catalog declaration.
    CatalogRemoved,
}

impl TeamManifestAction {
    /// Stable text label used by the human CLI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::Unchanged => "unchanged",
            Self::CatalogAdded => "catalog_added",
            Self::CatalogSkillsUpdated => "catalog_skills_updated",
            Self::CatalogVersionUpdated => "catalog_version_updated",
            Self::CatalogRemoved => "catalog_removed",
        }
    }

    /// Human label for a no-write preview.
    #[must_use]
    pub const fn planned_str(self) -> &'static str {
        match self {
            Self::Initialized => "initialize",
            Self::Unchanged => "leave_unchanged",
            Self::CatalogAdded => "add_catalog",
            Self::CatalogSkillsUpdated => "update_catalog_skills",
            Self::CatalogVersionUpdated => "update_catalog_version",
            Self::CatalogRemoved => "remove_catalog",
        }
    }
}

/// Result of a team-manifest management mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamManifestMutationReport {
    /// Manifest path.
    pub path: PathBuf,
    /// Applied or planned action.
    pub action: TeamManifestAction,
    /// Catalog affected by the action, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    /// Whether this was a no-write preview.
    pub dry_run: bool,
    /// Resulting manifest.
    pub manifest: TeamManifest,
}

/// Read-only view of a team manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamManifestView {
    /// Manifest path.
    pub path: PathBuf,
    /// Parsed manifest.
    pub manifest: TeamManifest,
}

/// Reviewed plan or applied update for one team-owned catalog pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamCatalogUpdateReport {
    /// Manifest path.
    pub path: PathBuf,
    /// Catalog ID local to the team manifest.
    pub catalog_id: String,
    /// Previous requested version from the manifest.
    pub old_version: String,
    /// Previous version resolved to an exact commit.
    pub old_commit: String,
    /// User-requested upstream branch, tag, or ref.
    pub from_ref: String,
    /// Candidate ref resolved to the exact commit written on success.
    pub candidate_commit: String,
    /// Selected-skill inventory changes.
    pub outcomes: Vec<DriftEntry>,
    /// Deterministic audits for candidate selected skills.
    pub audits: Vec<AuditReport>,
    /// Reasons preventing the manifest update.
    pub blocking_reasons: Vec<String>,
    /// Whether this was a no-write preview.
    pub dry_run: bool,
    /// Whether `dalo.toml` was changed.
    pub updated: bool,
    /// Resulting or currently persisted manifest.
    pub manifest: TeamManifest,
}

/// Summary of one manifest reconciliation pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ManifestReconcileReport {
    /// Manifest-derived sources added or changed.
    pub configured: Vec<String>,
    /// Manifest-derived sources no longer declared.
    pub removed: Vec<String>,
}

/// Rollback state retained until the enclosing sync commits materialization and
/// the resolved user lock.
#[derive(Debug, Clone)]
pub struct ManifestRollback {
    original_config: UserConfig,
    original_lock: catalog::SourceLock,
    original_approvals: store::ApprovalsFile,
    checkout_commits: Vec<(PathBuf, String)>,
    new_source_ids: Vec<String>,
}

impl ManifestRollback {
    /// Restore manifest-owned state after a later sync boundary fails.
    pub fn rollback(self, paths: &StorePaths) -> DaloResult<()> {
        let mut errors = Vec::new();
        for (checkout, commit) in self.checkout_commits.iter().rev() {
            if let Err(error) = git::checkout_detached(checkout, commit) {
                errors.push(format!("checkout `{}`: {error}", checkout.display()));
            }
        }
        for source_id in &self.new_source_ids {
            if let Err(error) = fs::remove_dir_all(paths.sources_dir.join(source_id))
                && error.kind() != std::io::ErrorKind::NotFound
            {
                errors.push(format!("new source `{source_id}`: {error}"));
            }
        }
        if let Err(error) = store::write_config(paths, &self.original_config) {
            errors.push(format!("config: {error}"));
        }
        if let Err(error) = catalog::write_source_lock(paths, &self.original_lock) {
            errors.push(format!("source lock: {error}"));
        }
        if let Err(error) = store::write_approvals(paths, &self.original_approvals) {
            errors.push(format!("approvals: {error}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(DaloError::Io(std::io::Error::other(format!(
                "manifest rollback incomplete: {}",
                errors.join("; ")
            ))))
        }
    }
}

/// Best-effort garbage collection for checkouts whose declarations disappeared.
///
/// Durable config, lock, approval, and materialization state is committed before
/// this runs. A failed deletion therefore remains harmless, inspectable debris
/// instead of rolling live state back to a removed declaration.
pub fn cleanup_removed_checkouts(paths: &StorePaths, source_ids: &[String]) {
    for source_id in source_ids {
        let source_dir = paths.sources_dir.join(source_id);
        let _ = fs::remove_dir_all(source_dir);
    }
}

/// Create a `dalo.toml` in a team repository.
pub fn init_team_manifest(
    repo: &Path,
    source_id: &str,
    name: Option<&str>,
    dry_run: bool,
) -> DaloResult<TeamManifestMutationReport> {
    if !source::is_valid_source_id(source_id) {
        return Err(DaloError::InvalidSourceId {
            id: source_id.to_owned(),
            reason: "must match `[A-Za-z0-9._-]+`".to_owned(),
        });
    }
    let path = team_manifest_path(repo)?;
    if path.exists() {
        reject_symlinked_manifest(&path)?;
        let manifest = read_managed_manifest(&path)?;
        let existing_id = manifest
            .source
            .as_ref()
            .and_then(|source| source.id.as_deref());
        if existing_id != Some(source_id) {
            return Err(DaloError::CheckFailed {
                reason: format!(
                    "team manifest `{}` already exists for source `{}`",
                    path.display(),
                    existing_id.unwrap_or("<missing>")
                ),
            });
        }
        return Ok(TeamManifestMutationReport {
            path,
            action: TeamManifestAction::Unchanged,
            catalog_id: None,
            dry_run,
            manifest,
        });
    }

    let manifest = TeamManifest {
        schema_version: TEAM_MANIFEST_SCHEMA_VERSION,
        source: Some(ManifestSource {
            id: Some(source_id.to_owned()),
            name: name.map(str::to_owned),
            kind: Some("team".to_owned()),
        }),
        catalogs: Vec::new(),
    };
    if !dry_run {
        write_manifest_atomic(&path, &manifest)?;
    }
    Ok(TeamManifestMutationReport {
        path,
        action: TeamManifestAction::Initialized,
        catalog_id: None,
        dry_run,
        manifest,
    })
}

/// Read a team repository's manifest for display.
pub fn show_team_manifest(repo: &Path) -> DaloResult<TeamManifestView> {
    let path = team_manifest_path(repo)?;
    let manifest = read_managed_manifest(&path)?;
    Ok(TeamManifestView { path, manifest })
}

/// Read and validate a manifest from an already configured team checkout.
pub fn load_team_manifest(repo: &Path, team_id: &str) -> DaloResult<TeamManifest> {
    let path = repo.join(TEAM_MANIFEST_FILE);
    let manifest = read_manifest(&path)?.ok_or_else(|| DaloError::CheckFailed {
        reason: format!("team manifest `{}` does not exist", path.display()),
    })?;
    validate_manifest(team_id, &path, &manifest)?;
    Ok(manifest)
}

/// Add one pinned catalog declaration to a team manifest.
pub fn add_team_catalog(
    repo: &Path,
    id: &str,
    url: &str,
    version: &str,
    skills: &[String],
    priority: Option<i32>,
    dry_run: bool,
) -> DaloResult<TeamManifestMutationReport> {
    if !source::is_valid_source_id(id) {
        return Err(DaloError::InvalidSourceId {
            id: id.to_owned(),
            reason: "must match `[A-Za-z0-9._-]+`".to_owned(),
        });
    }
    git::validate_remote_url(url)?;
    git::validate_manifest_revision(version)?;
    validate_filters(skills)?;
    mutate_team_manifest(
        repo,
        TeamManifestAction::CatalogAdded,
        Some(id),
        dry_run,
        |manifest| {
            if manifest.catalogs.iter().any(|catalog| catalog.id == id) {
                return Err(DaloError::SourceAlreadyExists {
                    source_id: id.to_owned(),
                });
            }
            manifest.catalogs.push(ManifestCatalog {
                id: id.to_owned(),
                url: url.to_owned(),
                version: version.to_owned(),
                skills: deduplicate_filters(skills),
                priority,
            });
            manifest
                .catalogs
                .sort_by(|left, right| left.id.cmp(&right.id));
            Ok(())
        },
    )
}

/// Replace one catalog declaration's include/exclude filters.
pub fn set_team_catalog_skills(
    repo: &Path,
    id: &str,
    skills: &[String],
    dry_run: bool,
) -> DaloResult<TeamManifestMutationReport> {
    validate_filters(skills)?;
    mutate_team_manifest(
        repo,
        TeamManifestAction::CatalogSkillsUpdated,
        Some(id),
        dry_run,
        |manifest| {
            manifest_catalog_mut(manifest, id)?.skills = deduplicate_filters(skills);
            Ok(())
        },
    )
}

/// Change one catalog declaration's requested Git version.
pub fn set_team_catalog_version(
    repo: &Path,
    id: &str,
    version: &str,
    dry_run: bool,
) -> DaloResult<TeamManifestMutationReport> {
    git::validate_manifest_revision(version)?;
    mutate_team_manifest(
        repo,
        TeamManifestAction::CatalogVersionUpdated,
        Some(id),
        dry_run,
        |manifest| {
            manifest_catalog_mut(manifest, id)?.version = version.to_owned();
            Ok(())
        },
    )
}

/// Resolve an upstream ref, audit the candidate, and write its exact commit.
pub fn update_team_catalog_pin(
    repo: &Path,
    id: &str,
    from_ref: &str,
    dry_run: bool,
) -> DaloResult<TeamCatalogUpdateReport> {
    git::validate_manifest_revision(from_ref)?;
    let path = team_manifest_path(repo)?;
    reject_symlinked_manifest(&path)?;
    let manifest = read_managed_manifest(&path)?;
    let declaration = manifest
        .catalogs
        .iter()
        .find(|catalog| catalog.id == id)
        .cloned()
        .ok_or_else(|| manifest_catalog_not_found(id, &manifest.catalogs))?;
    let team_id = manifest
        .source
        .as_ref()
        .and_then(|source| source.id.as_deref())
        .expect("managed manifest validation requires a source id");
    let source_id = derived_source_id(team_id, id);
    let manifest_root = path.parent().expect("team manifest path has a parent");
    let location = source::resolve_source_location(&declaration.url, manifest_root);
    git::validate_remote_url(&location)?;

    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    git::clone_repo(&location, &checkout)?;
    git::fetch_upstream(&checkout)?;
    let old_commit = git::resolve_manifest_revision(&checkout, &declaration.version)?;
    let candidate_commit = git::resolve_manifest_revision(&checkout, from_ref)?;
    let mut blocking_reasons = Vec::new();
    if old_commit != candidate_commit
        && git::revision_count(&checkout, &candidate_commit, &old_commit)? != 0
    {
        blocking_reasons.push(format!(
            "candidate commit {} is not a fast-forward from current commit {}",
            short_commit(&candidate_commit),
            short_commit(&old_commit)
        ));
    }

    git::checkout_detached(&checkout, &old_commit)?;
    let old_scan = inventory::scan_source(&source_id, &checkout)?;
    let selection_before =
        apply_skill_filters(&source_id, &checkout, &old_scan.skills, &declaration.skills)?;
    let old_inventory = catalog::catalog_inventory(&checkout, &selection_before)?;

    git::checkout_detached(&checkout, &candidate_commit)?;
    let candidate_scan = inventory::scan_source(&source_id, &checkout)?;
    let selection_after = match apply_skill_filters(
        &source_id,
        &checkout,
        &candidate_scan.skills,
        &declaration.skills,
    ) {
        Ok(selection) => selection,
        Err(error) => {
            blocking_reasons.push(format!("candidate selection is invalid: {error}"));
            selection_before.clone()
        }
    };
    let candidate_inventory = catalog::catalog_inventory(&checkout, &selection_after)?;
    let outcomes =
        catalog::compare_catalog_inventory(&old_inventory, &selection_before, &candidate_inventory);
    for outcome in &outcomes {
        if outcome.code == DriftCode::SelectedRemoved {
            blocking_reasons.push(format!(
                "selected skill `{}` was removed; update filters explicitly before changing the pin",
                outcome.skill
            ));
        }
    }
    let audit_paths = StorePaths::new(temp.path().join("audit-context"));
    let audits = audit_candidate_selection(
        &audit_paths,
        &source_id,
        &checkout,
        &candidate_scan.skills,
        &selection_after,
    )?;
    for report in &audits {
        if report.is_blocking() {
            blocking_reasons.push(format!(
                "security audit blocks candidate skill `{}`",
                report.source_ref
            ));
        }
    }
    blocking_reasons.sort();
    blocking_reasons.dedup();

    let mut resulting_manifest = manifest.clone();
    let pin_change = declaration.version != candidate_commit;
    let updated = !dry_run && pin_change && blocking_reasons.is_empty();
    if updated {
        let report = mutate_team_manifest(
            repo,
            TeamManifestAction::CatalogVersionUpdated,
            Some(id),
            false,
            |current| {
                let catalog = manifest_catalog_mut(current, id)?;
                if catalog != &declaration {
                    return Err(DaloError::CheckFailed {
                        reason: format!(
                            "team manifest catalog `{id}` changed while its update was being reviewed; retry against the current declaration"
                        ),
                    });
                }
                catalog.version.clone_from(&candidate_commit);
                Ok(())
            },
        )?;
        resulting_manifest = report.manifest;
    } else if dry_run && pin_change && blocking_reasons.is_empty() {
        manifest_catalog_mut(&mut resulting_manifest, id)?
            .version
            .clone_from(&candidate_commit);
    }

    Ok(TeamCatalogUpdateReport {
        path,
        catalog_id: id.to_owned(),
        old_version: declaration.version,
        old_commit,
        from_ref: from_ref.to_owned(),
        candidate_commit,
        outcomes,
        audits,
        blocking_reasons,
        dry_run,
        updated,
        manifest: resulting_manifest,
    })
}

/// Remove one catalog declaration from a team manifest.
pub fn remove_team_catalog(
    repo: &Path,
    id: &str,
    dry_run: bool,
) -> DaloResult<TeamManifestMutationReport> {
    mutate_team_manifest(
        repo,
        TeamManifestAction::CatalogRemoved,
        Some(id),
        dry_run,
        |manifest| {
            let before = manifest.catalogs.len();
            manifest.catalogs.retain(|catalog| catalog.id != id);
            if manifest.catalogs.len() == before {
                return Err(manifest_catalog_not_found(id, &manifest.catalogs));
            }
            Ok(())
        },
    )
}

fn mutate_team_manifest(
    repo: &Path,
    action: TeamManifestAction,
    catalog_id: Option<&str>,
    dry_run: bool,
    mutation: impl FnOnce(&mut TeamManifest) -> DaloResult<()>,
) -> DaloResult<TeamManifestMutationReport> {
    let path = team_manifest_path(repo)?;
    reject_symlinked_manifest(&path)?;
    let mut manifest = read_managed_manifest(&path)?;
    mutation(&mut manifest)?;
    validate_managed_manifest(&path, &manifest)?;
    if !dry_run {
        write_manifest_atomic(&path, &manifest)?;
    }
    Ok(TeamManifestMutationReport {
        path,
        action,
        catalog_id: catalog_id.map(str::to_owned),
        dry_run,
        manifest,
    })
}

fn team_manifest_path(repo: &Path) -> DaloResult<PathBuf> {
    let repo = fs::canonicalize(repo).map_err(|error| DaloError::InvalidStorePath {
        path: repo.to_path_buf(),
        reason: format!("team repository could not be resolved: {error}"),
    })?;
    if !repo.is_dir() {
        return Err(DaloError::InvalidStorePath {
            path: repo,
            reason: "team repository must be a directory".to_owned(),
        });
    }
    Ok(repo.join(TEAM_MANIFEST_FILE))
}

fn read_managed_manifest(path: &Path) -> DaloResult<TeamManifest> {
    let manifest = read_manifest(path)?.ok_or_else(|| DaloError::CheckFailed {
        reason: format!(
            "team manifest `{}` does not exist; run `dalo team init <source-id>` first",
            path.display()
        ),
    })?;
    validate_managed_manifest(path, &manifest)?;
    Ok(manifest)
}

fn validate_managed_manifest(path: &Path, manifest: &TeamManifest) -> DaloResult<()> {
    let source_id = manifest
        .source
        .as_ref()
        .and_then(|source| source.id.as_deref())
        .ok_or_else(|| DaloError::FileParse {
            path: path.to_path_buf(),
            reason: "managed team manifests require `[source].id`".to_owned(),
        })?;
    validate_manifest(source_id, path, manifest)
}

fn manifest_catalog_mut<'a>(
    manifest: &'a mut TeamManifest,
    id: &str,
) -> DaloResult<&'a mut ManifestCatalog> {
    let known = manifest.catalogs.clone();
    manifest
        .catalogs
        .iter_mut()
        .find(|catalog| catalog.id == id)
        .ok_or_else(|| manifest_catalog_not_found(id, &known))
}

fn manifest_catalog_not_found(id: &str, catalogs: &[ManifestCatalog]) -> DaloError {
    DaloError::unknown_source(
        id,
        catalogs.iter().map(|catalog| catalog.id.clone()).collect(),
    )
}

fn deduplicate_filters(filters: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    filters
        .iter()
        .filter(|filter| seen.insert((*filter).clone()))
        .cloned()
        .collect()
}

fn reject_symlinked_manifest(path: &Path) -> DaloResult<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "team manifest `{}` is a symlink; edit its real file explicitly",
                path.display()
            ),
        });
    }
    Ok(())
}

fn write_manifest_atomic(path: &Path, manifest: &TeamManifest) -> DaloResult<()> {
    reject_symlinked_manifest(path)?;
    let parent = path.parent().ok_or_else(|| DaloError::InvalidStorePath {
        path: path.to_path_buf(),
        reason: "team manifest has no parent directory".to_owned(),
    })?;
    let content = toml::to_string_pretty(manifest)?;
    let permissions = fs::metadata(path)
        .map(|metadata| metadata.permissions())
        .unwrap_or_else(|_| fs::Permissions::from_mode(0o644));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.as_file().set_permissions(permissions)?;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Build the manifest-derived config view that a dry-run can inspect without
/// fetching or changing store state.
///
/// A new catalog or version needs Git I/O and therefore cannot be represented
/// honestly by `sync --dry-run`; that case fails with an actionable message
/// instead of silently omitting the team declaration.
pub fn preview_team_manifests(paths: &StorePaths) -> DaloResult<UserConfig> {
    let mut config = store::read_config(paths)?;
    let lock = catalog::read_source_lock(paths)?;
    let team_sources = config
        .sources
        .iter()
        .filter(|candidate| candidate.enabled && candidate.kind == SourceKind::Team)
        .cloned()
        .collect::<Vec<_>>();
    let mut expected = BTreeSet::new();

    for team in &team_sources {
        let manifest_path = team.path.join(TEAM_MANIFEST_FILE);
        let Some(manifest) = read_manifest(&manifest_path)? else {
            continue;
        };
        validate_manifest(&team.id, &manifest_path, &manifest)?;
        for declaration in &manifest.catalogs {
            let source_id = derived_source_id(&team.id, &declaration.id);
            expected.insert(source_id.clone());
            let location = source::resolve_source_location(&declaration.url, &team.path);
            let Some(existing) = config
                .sources
                .iter()
                .find(|candidate| candidate.id == source_id)
                .cloned()
            else {
                return Err(dry_run_requires_sync(
                    &source_id,
                    "catalog is not cloned yet",
                ));
            };
            if existing.declared_by.as_deref() != Some(team.id.as_str()) {
                return Err(derived_source_conflict(
                    &config,
                    &existing,
                    team,
                    declaration,
                    &source_id,
                ));
            }
            if existing.url.as_deref() != Some(location.as_str()) {
                return Err(dry_run_requires_sync(&source_id, "catalog URL changed"));
            }
            if existing.declared_ref.as_deref() != Some(declaration.version.as_str()) {
                return Err(dry_run_requires_sync(&source_id, "catalog version changed"));
            }
            let locked = lock
                .catalog(&source_id)
                .ok_or_else(|| dry_run_requires_sync(&source_id, "catalog pin is missing"))?;
            if git::rev_parse_head(&existing.path)? != locked.commit {
                return Err(dry_run_requires_sync(
                    &source_id,
                    "catalog checkout does not match its pin",
                ));
            }
            let scanned = inventory::scan_source(&source_id, &existing.path)?;
            let selection = apply_skill_filters(
                &source_id,
                &existing.path,
                &scanned.skills,
                &declaration.skills,
            )?;
            let mut preview = existing;
            preview.priority = declaration.priority.unwrap_or(team.priority + 1);
            preview.selection = selection;
            upsert_derived_source(&mut config, preview, team, declaration)?;
        }
    }
    config
        .sources
        .retain(|candidate| candidate.declared_by.is_none() || expected.contains(&candidate.id));
    source::sort_sources(&mut config.sources);
    Ok(config)
}

fn dry_run_requires_sync(source_id: &str, reason: &str) -> DaloError {
    DaloError::CheckFailed {
        reason: format!(
            "cannot preview team catalog `{source_id}` without changing Git state ({reason}); run `dalo sync` to reconcile it first"
        ),
    }
}

/// Reconcile every enabled team source's `dalo.toml` into local managed state.
///
/// The team checkout remains the authority for URL, revision, priority, and
/// skill selection. Approvals remain local and are never granted here.
pub fn reconcile_team_manifests(
    paths: &StorePaths,
) -> DaloResult<(ManifestReconcileReport, ManifestRollback)> {
    let original_config = store::read_config(paths)?;
    let original_lock = catalog::read_source_lock(paths)?;
    let original_approvals = store::read_approvals(paths)?;
    let mut config = original_config.clone();
    let mut lock = original_lock.clone();
    let mut approvals = original_approvals.clone();
    let team_sources = original_config
        .sources
        .iter()
        .filter(|candidate| candidate.enabled && candidate.kind == SourceKind::Team)
        .cloned()
        .collect::<Vec<_>>();
    let mut expected = BTreeSet::new();
    let mut configured = Vec::new();
    let mut checkout_commits = Vec::new();
    let mut new_source_ids = Vec::new();

    for team in &team_sources {
        let manifest_path = team.path.join(TEAM_MANIFEST_FILE);
        let Some(manifest) = read_manifest(&manifest_path)? else {
            continue;
        };
        validate_manifest(&team.id, &manifest_path, &manifest)?;

        for declaration in &manifest.catalogs {
            let source_id = derived_source_id(&team.id, &declaration.id);
            migrate_legacy_derived_state(
                team,
                declaration,
                &source_id,
                &config,
                &mut lock,
                &mut approvals,
            );
            expected.insert(source_id.clone());
            let checkout_existed = paths
                .sources_dir
                .join(&source_id)
                .join("checkout/.git")
                .exists();
            let reconciled =
                match reconcile_catalog(paths, team, declaration, &source_id, &config, &mut lock) {
                    Ok(reconciled) => reconciled,
                    Err(error) => {
                        rollback_checkouts(&checkout_commits);
                        cleanup_removed_checkouts(paths, &new_source_ids);
                        if !checkout_existed {
                            cleanup_removed_checkouts(paths, std::slice::from_ref(&source_id));
                        }
                        return Err(error);
                    }
                };
            if let Some(previous) = reconciled.previous_commit {
                checkout_commits.push((reconciled.source.path.clone(), previous));
            }
            if reconciled.new_checkout {
                new_source_ids.push(reconciled.source.id.clone());
            }
            if let Err(error) =
                upsert_derived_source(&mut config, reconciled.source.clone(), team, declaration)
            {
                rollback_checkouts(&checkout_commits);
                cleanup_removed_checkouts(paths, &new_source_ids);
                return Err(error);
            }
            configured.push(reconciled.source.id);
        }
    }

    let removed = config
        .sources
        .iter()
        .filter(|candidate| candidate.declared_by.is_some() && !expected.contains(&candidate.id))
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        let removed_set = removed.iter().cloned().collect::<BTreeSet<_>>();
        config
            .sources
            .retain(|candidate| !removed_set.contains(&candidate.id));
        lock.catalogs
            .retain(|candidate| !removed_set.contains(&candidate.source_id));
        approvals.approvals.retain(|approval| {
            !removed_set.iter().any(|source_id| {
                approval.value == *source_id || approval.value.starts_with(&format!("{source_id}:"))
            })
        });
    }

    source::sort_sources(&mut config.sources);
    configured.sort();
    configured.dedup();
    if let Err(error) = persist_reconciled_state(
        paths,
        &original_config,
        &original_lock,
        &original_approvals,
        &config,
        &lock,
        &approvals,
    ) {
        rollback_checkouts(&checkout_commits);
        cleanup_removed_checkouts(paths, &new_source_ids);
        return Err(error);
    }

    Ok((
        ManifestReconcileReport {
            configured,
            removed,
        },
        ManifestRollback {
            original_config,
            original_lock,
            original_approvals,
            checkout_commits,
            new_source_ids,
        },
    ))
}

fn rollback_checkouts(checkouts: &[(PathBuf, String)]) {
    for (checkout, commit) in checkouts.iter().rev() {
        let _ = git::checkout_detached(checkout, commit);
    }
}

fn read_manifest(path: &Path) -> DaloResult<Option<TeamManifest>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    toml::from_str(&content)
        .map(Some)
        .map_err(|error| DaloError::FileParse {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })
}

fn validate_manifest(team_id: &str, path: &Path, manifest: &TeamManifest) -> DaloResult<()> {
    if manifest.schema_version != TEAM_MANIFEST_SCHEMA_VERSION {
        return Err(DaloError::UnsupportedSchema {
            path: path.to_path_buf(),
            version: manifest.schema_version,
            supported: TEAM_MANIFEST_SCHEMA_VERSION,
        });
    }
    if let Some(source) = &manifest.source
        && let Some(id) = &source.id
        && id != team_id
    {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "team manifest `{}` declares source id `{id}`, but it was added as `{team_id}`",
                path.display()
            ),
        });
    }
    let mut ids = BTreeSet::new();
    for catalog in &manifest.catalogs {
        if !source::is_valid_source_id(&catalog.id) {
            return Err(DaloError::InvalidSourceId {
                id: catalog.id.clone(),
                reason: "team catalog ids must match `[A-Za-z0-9._-]+`".to_owned(),
            });
        }
        if !ids.insert(&catalog.id) {
            return Err(DaloError::CheckFailed {
                reason: format!(
                    "team manifest `{}` declares catalog `{}` more than once",
                    path.display(),
                    catalog.id
                ),
            });
        }
        git::validate_remote_url(&catalog.url)?;
        validate_filters(&catalog.skills)?;
    }
    Ok(())
}

fn validate_filters(filters: &[String]) -> DaloResult<()> {
    for filter in filters {
        let reference = filter
            .strip_prefix('+')
            .or_else(|| filter.strip_prefix('-'))
            .unwrap_or(filter);
        if reference.is_empty() {
            return Err(DaloError::CheckFailed {
                reason: "team manifest skill filters must name a skill after `+` or `-`".to_owned(),
            });
        }
    }
    Ok(())
}

pub(crate) fn derived_source_id(team_id: &str, catalog_id: &str) -> String {
    // Preserve the original readable form for ordinary IDs. If either
    // component contains the legacy separator, encode both into a point-free
    // alphabet whose fixed delimiter cannot occur in the hex payload.
    if !team_id.contains('.') && !catalog_id.contains('.') {
        return legacy_derived_source_id(team_id, catalog_id);
    }
    format!(
        "team-{}-catalog-{}",
        encode_source_id_component(team_id),
        encode_source_id_component(catalog_id)
    )
}

fn legacy_derived_source_id(team_id: &str, catalog_id: &str) -> String {
    format!("{team_id}.{catalog_id}")
}

fn encode_source_id_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub(crate) fn source_id_matches_declaration(
    source_id: &str,
    team_id: &str,
    catalog_id: &str,
) -> bool {
    source_id == derived_source_id(team_id, catalog_id)
        || source_id == legacy_derived_source_id(team_id, catalog_id)
}

fn migrate_legacy_derived_state(
    team: &SourceConfig,
    declaration: &ManifestCatalog,
    source_id: &str,
    config: &UserConfig,
    lock: &mut catalog::SourceLock,
    approvals: &mut store::ApprovalsFile,
) {
    let legacy_id = legacy_derived_source_id(&team.id, &declaration.id);
    if legacy_id == source_id
        || !config.sources.iter().any(|source| {
            source.id == legacy_id && source.declared_by.as_deref() == Some(team.id.as_str())
        })
    {
        return;
    }

    if lock.catalog(source_id).is_none() {
        if let Some(legacy) = lock
            .catalogs
            .iter_mut()
            .find(|entry| entry.source_id == legacy_id)
        {
            legacy.source_id = source_id.to_owned();
        }
    } else {
        lock.catalogs.retain(|entry| entry.source_id != legacy_id);
    }

    let legacy_prefix = format!("{legacy_id}:");
    for approval in &mut approvals.approvals {
        if approval.scope == "source" && approval.value == legacy_id {
            approval.value = source_id.to_owned();
        } else if let Some(suffix) = approval.value.strip_prefix(&legacy_prefix) {
            approval.value = format!("{source_id}:{suffix}");
        }
    }
    let mut seen = BTreeSet::new();
    approvals
        .approvals
        .retain(|approval| seen.insert((approval.scope.clone(), approval.value.clone())));
}

fn derived_source_conflict(
    config: &UserConfig,
    existing: &SourceConfig,
    team: &SourceConfig,
    declaration: &ManifestCatalog,
    source_id: &str,
) -> DaloError {
    let incoming_manifest = team.path.join(TEAM_MANIFEST_FILE);
    let reason = if let Some((existing_manifest, existing_catalog_id)) =
        existing_manifest_declaration(config, existing)
    {
        format!(
            "team manifest `{}` catalog `{}` derives source `{source_id}`, which conflicts with team manifest `{}` catalog `{existing_catalog_id}`; rename one catalog id and run `dalo sync`",
            incoming_manifest.display(),
            declaration.id,
            existing_manifest.display()
        )
    } else if let Some(existing_team) = existing.declared_by.as_deref() {
        format!(
            "team manifest `{}` catalog `{}` derives source `{source_id}`, which conflicts with stale state owned by team manifest `{}`; reconcile or remove the stale derived source before retrying",
            incoming_manifest.display(),
            declaration.id,
            config
                .sources
                .iter()
                .find(|source| source.id == existing_team)
                .map_or_else(
                    || PathBuf::from(existing_team),
                    |source| source.path.join(TEAM_MANIFEST_FILE)
                )
                .display()
        )
    } else {
        format!(
            "team manifest `{}` catalog `{}` derives source `{source_id}`, which conflicts with independently configured source `{}`; rename the catalog or remove the independent source before retrying",
            incoming_manifest.display(),
            declaration.id,
            existing.id
        )
    };
    DaloError::CheckFailed { reason }
}

fn existing_manifest_declaration(
    config: &UserConfig,
    source: &SourceConfig,
) -> Option<(PathBuf, String)> {
    let team_id = source.declared_by.as_deref()?;
    let team = config.sources.iter().find(|team| team.id == team_id)?;
    let manifest_path = team.path.join(TEAM_MANIFEST_FILE);
    let manifest = read_manifest(&manifest_path).ok().flatten()?;
    let declaration = manifest
        .catalogs
        .iter()
        .find(|declaration| source_id_matches_declaration(&source.id, team_id, &declaration.id))?;
    Some((manifest_path, declaration.id.clone()))
}

struct ReconciledCatalog {
    source: SourceConfig,
    previous_commit: Option<String>,
    new_checkout: bool,
}

fn reconcile_catalog(
    paths: &StorePaths,
    team: &SourceConfig,
    declaration: &ManifestCatalog,
    source_id: &str,
    config: &UserConfig,
    lock: &mut catalog::SourceLock,
) -> DaloResult<ReconciledCatalog> {
    let location = source::resolve_source_location(&declaration.url, &team.path);
    let checkout = paths.sources_dir.join(source_id).join("checkout");
    let existing = config
        .sources
        .iter()
        .find(|candidate| candidate.id == source_id);
    if let Some(existing) = existing {
        if existing.declared_by.as_deref() != Some(team.id.as_str()) {
            return Err(derived_source_conflict(
                config,
                existing,
                team,
                declaration,
                source_id,
            ));
        }
        if existing.url.as_deref() != Some(location.as_str()) {
            return Err(DaloError::CheckFailed {
                reason: format!(
                    "team manifest changed the URL for `{source_id}`; remove that catalog declaration, sync once, then add the reviewed replacement URL"
                ),
            });
        }
    }

    let new_checkout = !checkout.join(".git").exists();
    if new_checkout {
        source::clone_source_checkout(&location, &checkout)?;
    }

    let locked = lock.catalog(source_id).cloned();
    let desired_commit = if existing.is_some_and(|candidate| {
        candidate.declared_ref.as_deref() == Some(declaration.version.as_str())
    }) {
        locked
            .as_ref()
            .map(|entry| entry.commit.clone())
            .unwrap_or_else(|| declaration.version.clone())
    } else {
        git::fetch_upstream(&checkout)?;
        declaration.version.clone()
    };
    let desired_commit = git::resolve_manifest_revision(&checkout, &desired_commit)?;
    let current_commit = git::rev_parse_head(&checkout)?;
    let (scan_root, staging) = if current_commit == desired_commit {
        (checkout.clone(), None)
    } else {
        if git::is_dirty(&checkout)? {
            return Err(DaloError::DirtySource {
                source_id: source_id.to_owned(),
                path: checkout,
            });
        }
        let staging_root = paths.sources_dir.join(".manifest-staging");
        fs::create_dir_all(&staging_root)?;
        let staging = staging_root.join(format!("{source_id}-{desired_commit}"));
        if staging.exists() {
            let _ = git::remove_worktree(&checkout, &staging);
            let _ = fs::remove_dir_all(&staging);
            git::prune_worktrees(&checkout)?;
        }
        git::add_detached_worktree(&checkout, &staging, &desired_commit)?;
        (staging.clone(), Some(staging))
    };

    let candidate = (|| -> DaloResult<(Vec<String>, Vec<catalog::CatalogEntry>)> {
        let scanned = inventory::scan_source(source_id, &scan_root)?;
        let selection =
            apply_skill_filters(source_id, &scan_root, &scanned.skills, &declaration.skills)?;
        audit_selected(paths, source_id, &scan_root, &scanned.skills, &selection)?;
        let inventory = catalog::catalog_inventory(&scan_root, &selection)?;
        Ok((selection, inventory))
    })();
    let cleanup = if let Some(staging) = &staging {
        let cleanup = git::remove_worktree(&checkout, staging);
        if let Some(staging_root) = staging.parent() {
            let _ = fs::remove_dir(staging_root);
        }
        cleanup
    } else {
        Ok(())
    };
    let (selection, inventory) = finish_candidate_after_cleanup(candidate, cleanup)?;
    let previous_commit = if current_commit == desired_commit {
        None
    } else {
        git::checkout_detached(&checkout, &desired_commit)?;
        Some(current_commit)
    };
    lock.catalogs.retain(|entry| entry.source_id != source_id);
    lock.catalogs.push(CatalogLock {
        source_id: source_id.to_owned(),
        commit: desired_commit,
        selected: selection.clone(),
        inventory,
    });
    lock.catalogs
        .sort_by(|left, right| left.source_id.cmp(&right.source_id));

    Ok(ReconciledCatalog {
        source: SourceConfig {
            id: source_id.to_owned(),
            kind: SourceKind::Catalog,
            path: checkout,
            priority: declaration.priority.unwrap_or(team.priority + 1),
            enabled: true,
            trusted: false,
            url: Some(location),
            branch: None,
            update_policy: Some("manifest".to_owned()),
            selection,
            declared_by: Some(team.id.clone()),
            declared_ref: Some(declaration.version.clone()),
        },
        previous_commit,
        new_checkout,
    })
}

fn apply_skill_filters(
    source_id: &str,
    checkout: &Path,
    skills: &[SkillRecord],
    filters: &[String],
) -> DaloResult<Vec<String>> {
    let candidates = skills
        .iter()
        .map(|skill| {
            let path = skill
                .path
                .strip_prefix(checkout)
                .unwrap_or(&skill.path)
                .to_string_lossy()
                .into_owned();
            let canonical = skill.id.clone().unwrap_or_else(|| path.clone());
            (skill, path, canonical)
        })
        .collect::<Vec<_>>();
    let has_positive = filters.iter().any(|filter| !filter.starts_with('-'));
    let mut selected = if filters.is_empty() || !has_positive {
        candidates
            .iter()
            .map(|(_, _, canonical)| canonical.clone())
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let mut excluded = BTreeSet::new();

    for filter in filters {
        let exclude = filter.starts_with('-');
        let reference = filter
            .strip_prefix('+')
            .or_else(|| filter.strip_prefix('-'))
            .unwrap_or(filter);
        let matches = candidates
            .iter()
            .filter(|(skill, path, _)| {
                skill.slot_name == reference
                    || skill.source_ref == reference
                    || skill.id.as_deref() == Some(reference)
                    || path == reference
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => {
                return Err(DaloError::skill_not_found(
                    format!("{source_id}:{reference}"),
                    candidates
                        .iter()
                        .map(|(skill, path, _)| {
                            skill
                                .id
                                .as_ref()
                                .map_or_else(|| path.clone(), |id| format!("{id} ({path})"))
                        })
                        .collect(),
                    format!("edit {TEAM_MANIFEST_FILE} in the declaring team repository"),
                ));
            }
            [(_, _, canonical)] => {
                if exclude {
                    excluded.insert(canonical.clone());
                } else {
                    selected.insert(canonical.clone());
                }
            }
            _ => {
                return Err(DaloError::AmbiguousSkillReference {
                    reference: reference.to_owned(),
                    matches: matches
                        .iter()
                        .map(|(_, path, _)| path.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                });
            }
        }
    }
    selected.retain(|candidate| !excluded.contains(candidate));
    Ok(selected.into_iter().collect())
}

fn finish_candidate_after_cleanup<T>(
    candidate: DaloResult<T>,
    cleanup: DaloResult<()>,
) -> DaloResult<T> {
    match candidate {
        Ok(value) => {
            cleanup?;
            Ok(value)
        }
        Err(error) => Err(error),
    }
}

fn audit_candidate_selection(
    paths: &StorePaths,
    source_id: &str,
    checkout: &Path,
    skills: &[SkillRecord],
    selection: &[String],
) -> DaloResult<Vec<AuditReport>> {
    let selected = selection.iter().cloned().collect::<BTreeSet<_>>();
    let mut reports = Vec::new();
    for skill in skills {
        let path = skill
            .path
            .strip_prefix(checkout)
            .unwrap_or(&skill.path)
            .to_path_buf();
        let canonical = skill
            .id
            .clone()
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        if !selected.contains(&canonical) {
            continue;
        }
        let mut report = audit::audit_skill(
            paths,
            &format!("{source_id}:{}", skill.slot_name),
            &skill.path,
            &AuditOptions {
                persist: false,
                ..AuditOptions::default()
            },
        )?;
        report.skill_path = path;
        reports.push(report);
    }
    reports.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    Ok(reports)
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn audit_selected(
    paths: &StorePaths,
    source_id: &str,
    checkout: &Path,
    skills: &[SkillRecord],
    selection: &[String],
) -> DaloResult<()> {
    let selected = selection.iter().cloned().collect::<BTreeSet<_>>();
    let by_canonical = skills
        .iter()
        .map(|skill| {
            let path = skill
                .path
                .strip_prefix(checkout)
                .unwrap_or(&skill.path)
                .to_string_lossy()
                .into_owned();
            (skill.id.clone().unwrap_or(path), skill)
        })
        .collect::<BTreeMap<_, _>>();
    let mut blocked = Vec::new();
    for reference in selected {
        let Some(skill) = by_canonical.get(&reference) else {
            continue;
        };
        let report = audit::audit_skill(
            paths,
            &format!("{source_id}:{}", skill.slot_name),
            &skill.path,
            &AuditOptions::default(),
        )?;
        if report.is_blocking() {
            blocked.push(format!("{source_id}:{}", skill.slot_name));
        }
    }
    if blocked.is_empty() {
        Ok(())
    } else {
        Err(DaloError::AuditBlocked {
            reason: format!(
                "team manifest selected blocked skill{}: {}",
                if blocked.len() == 1 { "" } else { "s" },
                blocked.join(", ")
            ),
        })
    }
}

fn upsert_derived_source(
    config: &mut UserConfig,
    source: SourceConfig,
    team: &SourceConfig,
    declaration: &ManifestCatalog,
) -> DaloResult<()> {
    if let Some(position) = config
        .sources
        .iter()
        .position(|candidate| candidate.id == source.id)
    {
        if config.sources[position].declared_by != source.declared_by {
            let existing = config.sources[position].clone();
            return Err(derived_source_conflict(
                config,
                &existing,
                team,
                declaration,
                &source.id,
            ));
        }
        config.sources[position] = source;
    } else {
        config.sources.push(source);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn persist_reconciled_state(
    paths: &StorePaths,
    original_config: &UserConfig,
    original_lock: &catalog::SourceLock,
    original_approvals: &store::ApprovalsFile,
    config: &UserConfig,
    lock: &catalog::SourceLock,
    approvals: &store::ApprovalsFile,
) -> DaloResult<()> {
    if original_config == config && original_lock == lock && original_approvals == approvals {
        return Ok(());
    }
    catalog::write_source_lock(paths, lock)?;
    if let Err(error) = store::write_config(paths, config) {
        let _ = catalog::write_source_lock(paths, original_lock);
        return Err(error);
    }
    if let Err(error) = store::write_approvals(paths, approvals) {
        let _ = store::write_config(paths, original_config);
        let _ = catalog::write_source_lock(paths, original_lock);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ApprovalRecord;

    fn skill(source: &str, slot: &str, id: Option<&str>, path: &str) -> SkillRecord {
        SkillRecord {
            source_id: source.to_owned(),
            source_ref: format!("{source}:{slot}"),
            id: id.map(str::to_owned),
            slot_name: slot.to_owned(),
            path: std::path::PathBuf::from("/catalog").join(path),
            skill_file: std::path::PathBuf::from("/catalog")
                .join(path)
                .join("SKILL.md"),
            description: None,
            requires: Vec::new(),
            owners: Vec::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn empty_filters_select_everything() {
        let skills = vec![
            skill("team.marketing", "copy", Some("marketing.copy"), "copy"),
            skill("team.marketing", "seo", None, "seo"),
        ];
        let selected = apply_skill_filters("team.marketing", Path::new("/catalog"), &skills, &[])
            .expect("empty filter should select all");
        assert_eq!(selected, ["marketing.copy", "seo"]);
    }

    #[test]
    fn minus_only_uses_all_as_its_base() {
        let skills = vec![
            skill("team.marketing", "copy", None, "copy"),
            skill("team.marketing", "seo", None, "seo"),
        ];
        let selected = apply_skill_filters(
            "team.marketing",
            Path::new("/catalog"),
            &skills,
            &["-seo".to_owned()],
        )
        .expect("blacklist should filter all");
        assert_eq!(selected, ["copy"]);
    }

    #[test]
    fn plus_switches_to_whitelist_and_minus_wins() {
        let skills = vec![
            skill("team.marketing", "copy", None, "copy"),
            skill("team.marketing", "seo", None, "seo"),
        ];
        let selected = apply_skill_filters(
            "team.marketing",
            Path::new("/catalog"),
            &skills,
            &["+copy".to_owned(), "+seo".to_owned(), "-seo".to_owned()],
        )
        .expect("minus should win");
        assert_eq!(selected, ["copy"]);
    }

    #[test]
    fn bare_entry_switches_to_whitelist() {
        let skills = vec![
            skill("team.marketing", "copy", None, "copy"),
            skill("team.marketing", "seo", None, "seo"),
        ];
        let selected = apply_skill_filters(
            "team.marketing",
            Path::new("/catalog"),
            &skills,
            &["copy".to_owned()],
        )
        .expect("bare positive should select from an empty base");
        assert_eq!(selected, ["copy"]);
    }

    #[test]
    fn candidate_error_wins_when_cleanup_also_fails() {
        let result: DaloResult<()> = finish_candidate_after_cleanup(
            Err(DaloError::AuditBlocked {
                reason: "blocked skill".to_owned(),
            }),
            Err(DaloError::CheckFailed {
                reason: "cleanup failed".to_owned(),
            }),
        );
        assert!(matches!(result, Err(DaloError::AuditBlocked { .. })));
    }

    #[test]
    fn derived_source_ids_should_preserve_simple_names_and_disambiguate_dots() {
        assert_eq!(
            derived_source_id("company", "marketing"),
            "company.marketing"
        );

        let catalog_dot = derived_source_id("company", "x.y");
        let team_dot = derived_source_id("company.x", "y");
        assert_eq!(catalog_dot, "team-636f6d70616e79-catalog-782e79");
        assert_eq!(team_dot, "team-636f6d70616e792e78-catalog-79");
        assert_ne!(catalog_dot, team_dot);
        assert!(!catalog_dot.contains('.'));
        assert!(!team_dot.contains('.'));
        assert!(source::is_valid_source_id(&catalog_dot));
        assert!(source::is_valid_source_id(&team_dot));
    }

    #[test]
    fn dotted_namespace_migration_should_preserve_locks_and_approvals() {
        let team = SourceConfig {
            id: "company".to_owned(),
            kind: SourceKind::Team,
            path: PathBuf::from("/team"),
            priority: 10,
            enabled: true,
            trusted: true,
            url: None,
            branch: None,
            update_policy: Some("track".to_owned()),
            selection: Vec::new(),
            declared_by: None,
            declared_ref: None,
        };
        let declaration = ManifestCatalog {
            id: "x.y".to_owned(),
            url: "https://example.invalid/catalog.git".to_owned(),
            version: "main".to_owned(),
            skills: Vec::new(),
            priority: None,
        };
        let legacy_id = legacy_derived_source_id(&team.id, &declaration.id);
        let source_id = derived_source_id(&team.id, &declaration.id);
        let derived = SourceConfig {
            id: legacy_id.clone(),
            kind: SourceKind::Catalog,
            path: PathBuf::from("/catalog"),
            priority: 11,
            enabled: true,
            trusted: false,
            url: Some(declaration.url.clone()),
            branch: None,
            update_policy: Some("manifest".to_owned()),
            selection: Vec::new(),
            declared_by: Some(team.id.clone()),
            declared_ref: Some(declaration.version.clone()),
        };
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: crate::config::Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![team.clone(), derived],
        };
        let mut lock = catalog::SourceLock::empty();
        lock.catalogs.push(CatalogLock {
            source_id: legacy_id.clone(),
            commit: "abc123".to_owned(),
            selected: vec!["copy".to_owned()],
            inventory: Vec::new(),
        });
        let mut approvals = store::ApprovalsFile::empty();
        approvals.approvals.extend([
            ApprovalRecord {
                scope: "source".to_owned(),
                value: legacy_id.clone(),
            },
            ApprovalRecord {
                scope: "skill".to_owned(),
                value: format!("{legacy_id}:copy"),
            },
        ]);

        migrate_legacy_derived_state(
            &team,
            &declaration,
            &source_id,
            &config,
            &mut lock,
            &mut approvals,
        );

        assert!(lock.catalog(&legacy_id).is_none());
        assert_eq!(
            lock.catalog(&source_id).map(|entry| entry.commit.as_str()),
            Some("abc123")
        );
        assert!(
            approvals
                .approvals
                .iter()
                .any(|approval| { approval.scope == "source" && approval.value == source_id })
        );
        assert!(approvals.approvals.iter().any(|approval| {
            approval.scope == "skill" && approval.value == format!("{source_id}:copy")
        }));
    }

    #[test]
    fn derived_source_conflict_should_name_both_manifest_declarations() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let existing_root = temp.path().join("company");
        let incoming_root = temp.path().join("company-x");
        fs::create_dir_all(&existing_root).expect("existing team root should exist");
        fs::create_dir_all(&incoming_root).expect("incoming team root should exist");
        let existing_manifest = TeamManifest {
            schema_version: TEAM_MANIFEST_SCHEMA_VERSION,
            source: None,
            catalogs: vec![ManifestCatalog {
                id: "x.y".to_owned(),
                url: "https://example.invalid/existing.git".to_owned(),
                version: "main".to_owned(),
                skills: Vec::new(),
                priority: None,
            }],
        };
        fs::write(
            existing_root.join(TEAM_MANIFEST_FILE),
            toml::to_string(&existing_manifest).expect("manifest should serialize"),
        )
        .expect("existing manifest should be written");
        let team_source = |id: &str, path: PathBuf| SourceConfig {
            id: id.to_owned(),
            kind: SourceKind::Team,
            path,
            priority: 10,
            enabled: true,
            trusted: true,
            url: None,
            branch: None,
            update_policy: Some("track".to_owned()),
            selection: Vec::new(),
            declared_by: None,
            declared_ref: None,
        };
        let existing_team = team_source("company", existing_root.clone());
        let incoming_team = team_source("company.x", incoming_root.clone());
        let existing = SourceConfig {
            id: "company.x.y".to_owned(),
            kind: SourceKind::Catalog,
            path: PathBuf::from("/catalog"),
            priority: 11,
            enabled: true,
            trusted: false,
            url: Some("https://example.invalid/existing.git".to_owned()),
            branch: None,
            update_policy: Some("manifest".to_owned()),
            selection: Vec::new(),
            declared_by: Some("company".to_owned()),
            declared_ref: Some("main".to_owned()),
        };
        let incoming_declaration = ManifestCatalog {
            id: "y".to_owned(),
            url: "https://example.invalid/incoming.git".to_owned(),
            version: "main".to_owned(),
            skills: Vec::new(),
            priority: None,
        };
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: crate::config::Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![existing_team, incoming_team.clone(), existing.clone()],
        };

        let error = derived_source_conflict(
            &config,
            &existing,
            &incoming_team,
            &incoming_declaration,
            "company.x.y",
        );
        let message = error.to_string();

        assert!(message.contains(&incoming_root.join(TEAM_MANIFEST_FILE).display().to_string()));
        assert!(message.contains(&existing_root.join(TEAM_MANIFEST_FILE).display().to_string()));
        assert!(message.contains("catalog `y`"));
        assert!(message.contains("catalog `x.y`"));
        assert!(!message.contains("independently configured"));
    }
}
