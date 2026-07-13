//! Catalog sources: inspect a multi-skill repository, select skills, and pin the
//! commit plus the resolved inventory snapshot in the source lock.

use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::inventory::{self, SkillRecord, SourceInventory};
use crate::source::{self, SourceConfig, SourceKind};
use crate::store::{self, StorePaths};

/// Current persisted source-lock schema version.
pub const SOURCE_LOCK_SCHEMA_VERSION: u32 = 2;
const MIN_SUPPORTED_SOURCE_LOCK_SCHEMA_VERSION: u32 = 1;

/// Source lock: pinned catalog commits, selections, and inventory snapshots.
///
/// Persisted as `source-lock.toml`. The inventory snapshot is what catalog drift
/// detection compares a freshly fetched inventory against.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLock {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Pinned catalog sources.
    #[serde(default)]
    pub catalogs: Vec<CatalogLock>,
}

/// One pinned catalog source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogLock {
    /// Catalog source ID.
    pub source_id: String,
    /// Pinned commit the inventory snapshot was taken from.
    pub commit: String,
    /// Selected skill references (stable ID preferred, else slot name).
    #[serde(default)]
    pub selected: Vec<String>,
    /// Inventory snapshot of the pinned commit, used for drift detection.
    #[serde(default)]
    pub inventory: Vec<CatalogEntry>,
}

/// One catalog inventory entry captured in the source lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    /// Stable frontmatter ID when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Physical install slot name.
    pub slot_name: String,
    /// Skill directory path relative to the catalog root.
    pub path: String,
    /// Content fingerprint over the skill directory.
    pub content_hash: String,
    /// Metadata fingerprint over the parsed frontmatter fields.
    pub metadata_hash: String,
    /// Declared dependencies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}

impl SourceLock {
    /// Empty source lock for a new store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: SOURCE_LOCK_SCHEMA_VERSION,
            catalogs: Vec::new(),
        }
    }

    /// The locked entry for a catalog source, if present.
    #[must_use]
    pub fn catalog(&self, source_id: &str) -> Option<&CatalogLock> {
        self.catalogs.iter().find(|c| c.source_id == source_id)
    }
}

/// One inspected candidate skill in a catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogCandidate {
    /// Stable frontmatter ID when present.
    pub id: Option<String>,
    /// Physical install slot name.
    pub slot_name: String,
    /// Skill directory path relative to the catalog root.
    pub path: String,
    /// Optional description.
    pub description: Option<String>,
    /// Declared dependencies.
    pub requires: Vec<String>,
    /// Whether this candidate is currently selected.
    pub selected: bool,
}

/// Catalog inspection report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogInspectReport {
    /// Catalog source ID.
    pub source_id: String,
    /// Discovered candidate skills, sorted by slot name.
    pub candidates: Vec<CatalogCandidate>,
}

/// Catalog select report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogSelectReport {
    /// Catalog source ID.
    pub source_id: String,
    /// Selected skill references after the operation.
    pub selected: Vec<String>,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// Add a catalog source and clone it into the store.
pub fn add_catalog_source(
    paths: &StorePaths,
    id: &str,
    url: &str,
    dry_run: bool,
) -> DaloResult<SourceConfig> {
    if !source::is_valid_source_id(id) {
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
        kind: SourceKind::Catalog,
        path: checkout.clone(),
        priority,
        // Catalog sources start with an empty selection: their skills are offers,
        // not wholesale dependencies, so nothing materializes until `source select`.
        enabled: true,
        // A catalog is an offer of third-party skills. Selecting a skill chooses
        // it for resolution, but does not approve its code to materialize.
        trusted: false,
        url: Some(url.to_owned()),
        branch: None,
        update_policy: Some("pin".to_owned()),
        selection: Vec::new(),
    };

    if dry_run {
        return Ok(source);
    }

    if let Some(parent) = checkout.parent() {
        fs::create_dir_all(parent)?;
    }
    git::clone_repo(url, &checkout).inspect_err(|_| {
        let _ = fs::remove_dir_all(&checkout);
    })?;

    // Pin the catalog commit and capture its inventory snapshot, then register the
    // source. On a later failure, remove the clone and the lock entry so config
    // never references a missing checkout.
    let persist = (|| -> DaloResult<()> {
        let commit = git::rev_parse_head(&checkout)?;
        let inventory = catalog_inventory(&checkout, &[])?;
        let mut lock = read_source_lock(paths)?;
        lock.catalogs.retain(|c| c.source_id != id);
        lock.catalogs.push(CatalogLock {
            source_id: id.to_owned(),
            commit,
            selected: Vec::new(),
            inventory,
        });
        lock.catalogs.sort_by(|a, b| a.source_id.cmp(&b.source_id));
        write_source_lock(paths, &lock)?;

        config.sources.push(source.clone());
        source::sort_sources(&mut config.sources);
        store::write_config(paths, &config)
    })();
    persist.inspect_err(|_| {
        let _ = fs::remove_dir_all(&checkout);
        if let Ok(mut lock) = read_source_lock(paths) {
            lock.catalogs.retain(|c| c.source_id != id);
            let _ = write_source_lock(paths, &lock);
        }
    })?;

    Ok(source)
}

/// Inspect a catalog source read-only: list its candidate skills and which are
/// currently selected. Does not change config, the lock, or the resolved set.
#[must_use = "the inspect report should be rendered"]
pub fn inspect_catalog(paths: &StorePaths, id: &str) -> DaloResult<CatalogInspectReport> {
    let source = catalog_source(paths, id)?;
    let selected = selection_set(&source.selection);
    let scan = scan_catalog(&source.path)?;
    let mut candidates = catalog_candidates_from_scan(&source.path, &scan);
    for candidate in &mut candidates {
        candidate.selected = candidate_is_selected(candidate, &selected);
    }
    Ok(CatalogInspectReport {
        source_id: id.to_owned(),
        candidates,
    })
}

/// Select skills from a catalog. Each ref must match an inventory entry by stable
/// ID or slot name; unknown refs are rejected. Refreshes the lock inventory
/// snapshot so subsequent drift detection compares against the current commit.
pub fn select_skills(
    paths: &StorePaths,
    id: &str,
    refs: &[String],
    unselect: bool,
    dry_run: bool,
) -> DaloResult<CatalogSelectReport> {
    let source_path = catalog_source(paths, id)?.path;
    let scan = scan_catalog(&source_path)?;
    let candidates = catalog_candidates_from_scan(&source_path, &scan);
    let mut resolved = Vec::with_capacity(refs.len());
    for reference in refs {
        resolved.push(resolve_candidate_reference(id, &candidates, reference)?);
    }

    // Validate and snapshot every durable input before changing either config or
    // the catalog lock. This keeps a malformed lock from becoming a partial
    // selection update and gives us a rollback baseline for a later config write.
    let original_lock = read_source_lock(paths)?;
    let mut lock = original_lock.clone();
    let mut config = store::read_config(paths)?;
    let known_sources = config
        .sources
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect();
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == id)
        .ok_or_else(|| DaloError::unknown_source(id, known_sources))?;
    for candidate in &resolved {
        source
            .selection
            .retain(|existing| !selection_matches_candidate(existing, candidate));
        if !unselect {
            source.selection.push(canonical_selection(candidate));
        }
    }
    source.selection.sort();
    source.selection.dedup();
    let selected = source.selection.clone();
    if !dry_run {
        let commit = git::rev_parse_head(&source_path)?;
        let inventory = if let Some(index) = lock
            .catalogs
            .iter()
            .position(|catalog| catalog.source_id == id)
        {
            if lock.schema_version == SOURCE_LOCK_SCHEMA_VERSION
                && lock.catalogs[index].commit == commit
            {
                let mut inventory = lock.catalogs[index].inventory.clone();
                hydrate_selected_content_hashes(&mut inventory, &source_path, &scan, &selected)?;
                inventory
            } else {
                catalog_inventory_from_scan(&source_path, &scan, &selected)?
            }
        } else {
            catalog_inventory_from_scan(&source_path, &scan, &selected)?
        };
        if let Some(index) = lock
            .catalogs
            .iter()
            .position(|catalog| catalog.source_id == id)
        {
            lock.catalogs[index].selected = selected.clone();
            lock.catalogs[index].commit = commit;
            lock.catalogs[index].inventory = inventory;
        } else {
            lock.catalogs.push(CatalogLock {
                source_id: id.to_owned(),
                commit,
                selected: selected.clone(),
                inventory,
            });
            lock.catalogs.sort_by(|a, b| a.source_id.cmp(&b.source_id));
        }
        lock.schema_version = SOURCE_LOCK_SCHEMA_VERSION;
        write_source_lock(paths, &lock)?;
        if let Err(error) = store::write_config(paths, &config) {
            if let Err(rollback) = write_source_lock(paths, &original_lock) {
                return Err(DaloError::Io(std::io::Error::other(format!(
                    "{error}; additionally failed to roll back source lock: {rollback}"
                ))));
            }
            return Err(error);
        }
    }

    Ok(CatalogSelectReport {
        source_id: id.to_owned(),
        selected,
        dry_run,
    })
}

/// Whether an inventory skill is part of a catalog source's selection.
#[must_use]
pub fn skill_is_selected(skill: &SkillRecord, selection: &[String], source_root: &Path) -> bool {
    selection.iter().any(|reference| {
        reference == &skill.slot_name
            || reference == &skill.source_ref
            || skill.id.as_deref() == Some(reference.as_str())
            || skill
                .path
                .strip_prefix(source_root)
                .is_ok_and(|path| path.to_string_lossy() == reference.as_str())
    })
}

/// Read the source lock, or an empty lock when it does not exist yet.
pub fn read_source_lock(paths: &StorePaths) -> DaloResult<SourceLock> {
    if !paths.source_lock_file.exists() {
        return Ok(SourceLock::empty());
    }
    let content = fs::read_to_string(&paths.source_lock_file)?;
    let lock: SourceLock = toml::from_str(&content).map_err(|error| DaloError::FileParse {
        path: paths.source_lock_file.clone(),
        reason: error.to_string(),
    })?;
    if !(MIN_SUPPORTED_SOURCE_LOCK_SCHEMA_VERSION..=SOURCE_LOCK_SCHEMA_VERSION)
        .contains(&lock.schema_version)
    {
        return Err(DaloError::UnsupportedSchema {
            path: paths.source_lock_file.clone(),
            version: lock.schema_version,
            supported: SOURCE_LOCK_SCHEMA_VERSION,
        });
    }
    Ok(lock)
}

/// Write the source lock atomically.
pub fn write_source_lock(paths: &StorePaths, lock: &SourceLock) -> DaloResult<()> {
    store::write_toml_atomic(&paths.source_lock_file, lock)
}

/// Build the inventory snapshot for a catalog checkout (used for the lock).
pub fn catalog_inventory(checkout: &Path, selection: &[String]) -> DaloResult<Vec<CatalogEntry>> {
    let inventory = scan_catalog(checkout)?;
    catalog_inventory_from_scan(checkout, &inventory, selection)
}

fn scan_catalog(checkout: &Path) -> DaloResult<SourceInventory> {
    inventory::scan_source("catalog", checkout)
}

fn catalog_inventory_from_scan(
    checkout: &Path,
    inventory: &SourceInventory,
    selection: &[String],
) -> DaloResult<Vec<CatalogEntry>> {
    let mut entries = Vec::with_capacity(inventory.skills.len());
    for skill in &inventory.skills {
        entries.push(CatalogEntry {
            id: skill.id.clone(),
            slot_name: skill.slot_name.clone(),
            path: relative_path(checkout, &skill.path),
            content_hash: skill_is_selected(skill, selection, checkout)
                .then(|| hash_directory(&skill.path))
                .transpose()?
                .unwrap_or_default(),
            metadata_hash: hash_metadata(skill),
            requires: skill.requires.clone(),
        });
    }
    entries.sort_by(|a, b| a.slot_name.cmp(&b.slot_name).then(a.path.cmp(&b.path)));
    Ok(entries)
}

fn hydrate_selected_content_hashes(
    entries: &mut [CatalogEntry],
    checkout: &Path,
    inventory: &SourceInventory,
    selection: &[String],
) -> DaloResult<()> {
    for entry in entries {
        let Some(skill) = inventory
            .skills
            .iter()
            .find(|skill| relative_path(checkout, &skill.path) == entry.path)
        else {
            continue;
        };
        if skill_is_selected(skill, selection, checkout) && entry.content_hash.is_empty() {
            entry.content_hash = hash_directory(&skill.path)?;
        }
    }
    Ok(())
}

fn catalog_candidates_from_scan(
    checkout: &Path,
    inventory: &SourceInventory,
) -> Vec<CatalogCandidate> {
    let mut candidates = inventory
        .skills
        .iter()
        .map(|skill| CatalogCandidate {
            id: skill.id.clone(),
            slot_name: skill.slot_name.clone(),
            path: relative_path(checkout, &skill.path),
            description: skill.description.clone(),
            requires: skill.requires.clone(),
            selected: false,
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.slot_name.cmp(&b.slot_name).then(a.path.cmp(&b.path)));
    candidates
}

fn catalog_source(paths: &StorePaths, id: &str) -> DaloResult<SourceConfig> {
    let config = store::read_config(paths)?;
    let known_sources = config
        .sources
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect();
    let source = config
        .sources
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| DaloError::unknown_source(id, known_sources))?;
    if source.kind != SourceKind::Catalog {
        return Err(DaloError::NotACatalogSource {
            source_id: id.to_owned(),
        });
    }
    Ok(source)
}

fn selection_set(selection: &[String]) -> Vec<String> {
    selection.to_vec()
}

fn candidate_is_selected(candidate: &CatalogCandidate, selection: &[String]) -> bool {
    selection
        .iter()
        .any(|reference| candidate_matches_ref(candidate, reference))
}

fn candidate_matches_ref(candidate: &CatalogCandidate, reference: &str) -> bool {
    candidate.slot_name == reference
        || candidate.path == reference
        || candidate.id.as_deref() == Some(reference)
}

fn resolve_candidate_reference(
    source_id: &str,
    candidates: &[CatalogCandidate],
    reference: &str,
) -> DaloResult<CatalogCandidate> {
    let matches = candidates
        .iter()
        .filter(|candidate| candidate_matches_ref(candidate, reference))
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(DaloError::skill_not_found(
            format!("{source_id}:{reference}"),
            candidates.iter().map(candidate_display).collect(),
            format!("dalo source inspect {source_id}"),
        )),
        [candidate] => Ok(candidate.clone()),
        _ => Err(DaloError::AmbiguousSkillReference {
            reference: reference.to_owned(),
            matches: matches
                .iter()
                .map(candidate_display)
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

fn canonical_selection(candidate: &CatalogCandidate) -> String {
    candidate
        .id
        .clone()
        .unwrap_or_else(|| candidate.path.clone())
}

fn selection_matches_candidate(selection: &str, candidate: &CatalogCandidate) -> bool {
    selection == candidate.slot_name
        || selection == candidate.path
        || candidate.id.as_deref() == Some(selection)
}

fn candidate_display(candidate: &CatalogCandidate) -> String {
    match &candidate.id {
        Some(id) => format!("{id} ({})", candidate.path),
        None => candidate.path.clone(),
    }
}

/// Drift outcome code (RFC 0001 §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftCode {
    /// Upstream added a skill that is not selected (informational).
    NewAvailable,
    /// A selected skill changed content or metadata (reviewable).
    SelectedChanged,
    /// A selected skill moved to a new path (auto-reconcilable on stable ID).
    SelectedMoved,
    /// A selected skill no longer exists (blocks scheduled sync for it).
    SelectedRemoved,
}

impl DriftCode {
    /// Whether this outcome blocks non-interactive sync for the selection.
    #[must_use]
    pub fn blocks_sync(self) -> bool {
        matches!(self, Self::SelectedRemoved)
    }

    /// Snake-case label, matching the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewAvailable => "new_available",
            Self::SelectedChanged => "selected_changed",
            Self::SelectedMoved => "selected_moved",
            Self::SelectedRemoved => "selected_removed",
        }
    }
}

/// One drift finding for a catalog skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriftEntry {
    /// Outcome code.
    pub code: DriftCode,
    /// Affected skill (slot name or stable ID).
    pub skill: String,
    /// Human-readable message.
    pub message: String,
}

/// Catalog drift report from a read-only check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogDrift {
    /// Catalog source ID.
    pub source_id: String,
    /// Pinned commit (from the lock).
    pub pinned_commit: String,
    /// Upstream commit observed by the check.
    pub upstream_commit: String,
    /// Classified drift outcomes.
    pub outcomes: Vec<DriftEntry>,
}

/// Compare a pinned inventory snapshot against a fresh inventory and classify the
/// four drift outcomes. Selected skills are matched by stable ID first (which
/// enables move detection), then by path.
#[must_use]
pub fn compare_catalog_inventory(
    locked: &[CatalogEntry],
    selected: &[String],
    fresh: &[CatalogEntry],
) -> Vec<DriftEntry> {
    let is_selected = |entry: &CatalogEntry| {
        selected.iter().any(|reference| {
            reference == &entry.slot_name
                || reference == &entry.path
                || entry.id.as_deref() == Some(reference.as_str())
        })
    };
    let mut outcomes = Vec::new();

    // new_available: a fresh entry absent from the locked snapshot and unselected.
    for fresh_entry in fresh {
        let known = locked.iter().any(|l| same_entry(l, fresh_entry));
        if !known && !is_selected(fresh_entry) {
            outcomes.push(DriftEntry {
                code: DriftCode::NewAvailable,
                skill: fresh_entry.slot_name.clone(),
                message: format!(
                    "`{}` is newly available (not selected)",
                    fresh_entry.slot_name
                ),
            });
        }
    }

    // changed / moved / removed for each selected locked entry.
    for locked_entry in locked.iter().filter(|entry| is_selected(entry)) {
        let by_id = locked_entry
            .id
            .as_deref()
            .and_then(|id| fresh.iter().find(|f| f.id.as_deref() == Some(id)));
        let by_path = fresh.iter().find(|f| f.path == locked_entry.path);
        let skill = locked_entry
            .id
            .clone()
            .unwrap_or_else(|| locked_entry.slot_name.clone());
        match by_id.or(by_path) {
            Some(fresh_entry) => {
                if by_id.is_some() && fresh_entry.path != locked_entry.path {
                    outcomes.push(DriftEntry {
                        code: DriftCode::SelectedMoved,
                        skill,
                        message: format!(
                            "`{}` moved from `{}` to `{}`",
                            locked_entry.slot_name, locked_entry.path, fresh_entry.path
                        ),
                    });
                } else if fresh_entry.content_hash != locked_entry.content_hash
                    || fresh_entry.metadata_hash != locked_entry.metadata_hash
                {
                    outcomes.push(DriftEntry {
                        code: DriftCode::SelectedChanged,
                        skill,
                        message: format!("`{}` changed upstream", locked_entry.slot_name),
                    });
                }
            }
            None => outcomes.push(DriftEntry {
                code: DriftCode::SelectedRemoved,
                skill,
                message: format!("`{}` was removed upstream", locked_entry.slot_name),
            }),
        }
    }

    outcomes.sort_by(|a, b| {
        a.code
            .as_str()
            .cmp(b.code.as_str())
            .then_with(|| a.skill.cmp(&b.skill))
    });
    outcomes
}

/// Read-only drift check: fetch the catalog's upstream ref and compare a fresh
/// inventory against the pinned snapshot. Does not advance the pin or change
/// config, the lock, or the resolved set.
pub fn check_catalog_drift(paths: &StorePaths, id: &str) -> DaloResult<CatalogDrift> {
    let source = catalog_source(paths, id)?;
    let mut lock = read_source_lock(paths)?;
    let mut catalog_lock = lock
        .catalog(id)
        .ok_or_else(|| DaloError::unknown_source(id, Vec::new()))?
        .clone();

    if lock.schema_version != SOURCE_LOCK_SCHEMA_VERSION {
        catalog_lock.inventory =
            catalog_inventory_at_commit(&source.path, &catalog_lock.commit, &source.selection)?;
        if let Some(stored) = lock
            .catalogs
            .iter_mut()
            .find(|catalog| catalog.source_id == id)
        {
            stored.inventory = catalog_lock.inventory.clone();
        }
        lock.schema_version = SOURCE_LOCK_SCHEMA_VERSION;
        write_source_lock(paths, &lock)?;
    }

    git::fetch(&source.path)?;
    git::prune_worktrees(&source.path)?;
    let upstream_commit = git::rev_parse(&source.path, "FETCH_HEAD")?;

    let fresh = if upstream_commit == catalog_lock.commit {
        catalog_lock.inventory.clone()
    } else {
        let temp = tempfile::tempdir()?;
        let worktree = temp.path().join("upstream");
        git::add_detached_worktree(&source.path, &worktree, &upstream_commit)?;
        let scanned = catalog_inventory(&worktree, &source.selection);
        let _ = git::remove_worktree(&source.path, &worktree);
        let _ = git::prune_worktrees(&source.path);
        scanned?
    };

    let outcomes = compare_catalog_inventory(&catalog_lock.inventory, &source.selection, &fresh);
    Ok(CatalogDrift {
        source_id: id.to_owned(),
        pinned_commit: catalog_lock.commit,
        upstream_commit,
        outcomes,
    })
}

fn catalog_inventory_at_commit(
    source_path: &Path,
    commit: &str,
    selection: &[String],
) -> DaloResult<Vec<CatalogEntry>> {
    let temp = tempfile::tempdir()?;
    let worktree = temp.path().join("pinned");
    git::add_detached_worktree(source_path, &worktree, commit)?;
    let scanned = catalog_inventory(&worktree, selection);
    let _ = git::remove_worktree(source_path, &worktree);
    let _ = git::prune_worktrees(source_path);
    scanned
}

fn same_entry(a: &CatalogEntry, b: &CatalogEntry) -> bool {
    match (a.id.as_deref(), b.id.as_deref()) {
        (Some(left), Some(right)) => left == right,
        _ => a.path == b.path,
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Stream a skill directory's files into a stable content fingerprint.
pub fn hash_directory(skill_dir: &Path) -> DaloResult<String> {
    let mut files = Vec::new();
    collect_files(skill_dir, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for file in &files {
        let relative = file.strip_prefix(skill_dir).unwrap_or(file.as_path());
        hash_framed(&mut hasher, relative.as_os_str().as_bytes());
        let metadata = fs::symlink_metadata(file)?;
        if metadata.file_type().is_symlink() {
            hasher.update(*b"L");
            let target = fs::read_link(file)?;
            hash_framed(&mut hasher, target.as_os_str().as_bytes());
        } else {
            hasher.update(*b"F");
            hasher.update(metadata.len().to_le_bytes());
            let mut handle = fs::File::open(file)?;
            let mut buffer = [0u8; 8192];
            loop {
                let read = handle.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        }
    }
    Ok(hex_digest(&hasher.finalize()))
}

fn hash_framed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hash_metadata(skill: &SkillRecord) -> String {
    let mut hasher = Sha256::new();
    hasher.update(skill.id.clone().unwrap_or_default().as_bytes());
    hasher.update([0]);
    hasher.update(skill.description.clone().unwrap_or_default().as_bytes());
    hasher.update([0]);
    for requirement in &skill.requires {
        hasher.update(requirement.as_bytes());
        hasher.update([0]);
    }
    hasher.update([1]);
    for tag in &skill.tags {
        hasher.update(tag.as_bytes());
        hasher.update([0]);
    }
    hex_digest(&hasher.finalize())
}

fn collect_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> DaloResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(&path, files)?;
        } else if file_type.is_file() || file_type.is_symlink() {
            files.push(path);
        }
    }
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn entry(id: Option<&str>, slot: &str, path: &str, content: &str) -> CatalogEntry {
        CatalogEntry {
            id: id.map(str::to_owned),
            slot_name: slot.to_owned(),
            path: path.to_owned(),
            content_hash: content.to_owned(),
            metadata_hash: "m".to_owned(),
            requires: Vec::new(),
        }
    }

    #[test]
    fn compare_should_flag_new_available_for_unselected_additions() {
        let locked = vec![entry(Some("a"), "alpha", "skills/alpha", "h1")];
        let fresh = vec![
            entry(Some("a"), "alpha", "skills/alpha", "h1"),
            entry(Some("b"), "beta", "skills/beta", "h2"),
        ];
        let outcomes = compare_catalog_inventory(&locked, &["alpha".to_owned()], &fresh);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].code, DriftCode::NewAvailable);
        assert_eq!(outcomes[0].skill, "beta");
    }

    #[test]
    fn compare_should_flag_selected_changed_on_content_hash() {
        let locked = vec![entry(Some("a"), "alpha", "skills/alpha", "h1")];
        let fresh = vec![entry(Some("a"), "alpha", "skills/alpha", "h2")];
        let outcomes = compare_catalog_inventory(&locked, &["alpha".to_owned()], &fresh);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].code, DriftCode::SelectedChanged);
    }

    #[test]
    fn compare_should_flag_selected_moved_via_stable_id() {
        let locked = vec![entry(Some("a"), "alpha", "skills/alpha", "h1")];
        let fresh = vec![entry(Some("a"), "alpha", "catalog/alpha", "h1")];
        let outcomes = compare_catalog_inventory(&locked, &["alpha".to_owned()], &fresh);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].code, DriftCode::SelectedMoved);
    }

    #[test]
    fn compare_should_flag_selected_removed_when_absent() {
        let locked = vec![entry(Some("a"), "alpha", "skills/alpha", "h1")];
        let fresh: Vec<CatalogEntry> = Vec::new();
        let outcomes = compare_catalog_inventory(&locked, &["alpha".to_owned()], &fresh);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].code, DriftCode::SelectedRemoved);
        assert!(outcomes[0].code.blocks_sync());
    }

    #[test]
    fn compare_should_ignore_unselected_changes() {
        let locked = vec![entry(Some("a"), "alpha", "skills/alpha", "h1")];
        let fresh = vec![entry(Some("a"), "alpha", "skills/alpha", "h2")];
        // `alpha` is not selected: no changed/removed outcome, and it is already
        // known, so no new_available either.
        let outcomes = compare_catalog_inventory(&locked, &[], &fresh);
        assert!(outcomes.is_empty());
    }

    #[test]
    fn hash_directory_should_include_symlink_targets() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("skill");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join("SKILL.md"), "# Skill\n").expect("skill file should be written");
        fs::write(skill_dir.join("a.md"), "A\n").expect("target a should be written");
        fs::write(skill_dir.join("b.md"), "B\n").expect("target b should be written");
        symlink("a.md", skill_dir.join("linked.md")).expect("symlink should be created");

        let first = hash_directory(&skill_dir).expect("hash should succeed");
        fs::remove_file(skill_dir.join("linked.md")).expect("symlink should be removed");
        symlink("b.md", skill_dir.join("linked.md")).expect("symlink should be recreated");
        let second = hash_directory(&skill_dir).expect("hash should succeed");

        assert_ne!(first, second);
    }
}
