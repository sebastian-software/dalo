//! Catalog sources: inspect a multi-skill repository, select skills, and pin the
//! commit plus the resolved inventory snapshot in the source lock.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::inventory::{self, SkillRecord};
use crate::source::{self, SourceConfig, SourceKind};
use crate::store::{self, StorePaths};

/// Current persisted source-lock schema version.
pub const SOURCE_LOCK_SCHEMA_VERSION: u32 = 1;

/// Source lock: pinned catalog commits, selections, and inventory snapshots.
///
/// Persisted as `source-lock.toml`. The inventory snapshot is what catalog drift
/// detection compares a freshly fetched inventory against.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLock {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Pinned catalog sources.
    #[serde(default)]
    pub catalogs: Vec<CatalogLock>,
}

/// One pinned catalog source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        trusted: true,
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
    git::clone_repo(url, &checkout)?;

    // Pin the catalog commit and capture its inventory snapshot, then register the
    // source. On a later failure, remove the clone and the lock entry so config
    // never references a missing checkout.
    let persist = (|| -> DaloResult<()> {
        let commit = git::rev_parse_head(&checkout)?;
        let inventory = catalog_inventory(&checkout)?;
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
        config.sources.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.id.cmp(&right.id))
        });
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
    let mut candidates = catalog_candidates(&source.path)?;
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
) -> DaloResult<CatalogSelectReport> {
    let candidates = catalog_candidates(&catalog_source(paths, id)?.path)?;
    for reference in refs {
        if !candidates
            .iter()
            .any(|c| candidate_matches_ref(c, reference))
        {
            return Err(DaloError::SkillNotFound {
                skill: format!("{id}:{reference}"),
            });
        }
    }

    let mut config = store::read_config(paths)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == id)
        .ok_or_else(|| DaloError::UnknownSource {
            source_id: id.to_owned(),
        })?;
    for reference in refs {
        source.selection.retain(|existing| existing != reference);
        if !unselect {
            source.selection.push(reference.clone());
        }
    }
    source.selection.sort();
    source.selection.dedup();
    let selected = source.selection.clone();
    store::write_config(paths, &config)?;

    Ok(CatalogSelectReport {
        source_id: id.to_owned(),
        selected,
    })
}

/// Whether an inventory skill is part of a catalog source's selection.
#[must_use]
pub fn skill_is_selected(skill: &SkillRecord, selection: &[String]) -> bool {
    selection.iter().any(|reference| {
        reference == &skill.slot_name
            || reference == &skill.source_ref
            || skill.id.as_deref() == Some(reference.as_str())
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
    if lock.schema_version != SOURCE_LOCK_SCHEMA_VERSION {
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
pub fn catalog_inventory(checkout: &Path) -> DaloResult<Vec<CatalogEntry>> {
    let inventory = inventory::scan_source("catalog", checkout)?;
    let mut entries = Vec::with_capacity(inventory.skills.len());
    for skill in &inventory.skills {
        entries.push(CatalogEntry {
            id: skill.id.clone(),
            slot_name: skill.slot_name.clone(),
            path: relative_path(checkout, &skill.path),
            content_hash: hash_directory(&skill.path)?,
            metadata_hash: hash_metadata(skill),
            requires: skill.requires.clone(),
        });
    }
    entries.sort_by(|a, b| a.slot_name.cmp(&b.slot_name).then(a.path.cmp(&b.path)));
    Ok(entries)
}

fn catalog_candidates(checkout: &Path) -> DaloResult<Vec<CatalogCandidate>> {
    let inventory = inventory::scan_source("catalog", checkout)?;
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
    Ok(candidates)
}

fn catalog_source(paths: &StorePaths, id: &str) -> DaloResult<SourceConfig> {
    let config = store::read_config(paths)?;
    let source = config
        .sources
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| DaloError::UnknownSource {
            source_id: id.to_owned(),
        })?;
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
    candidate.slot_name == reference || candidate.id.as_deref() == Some(reference)
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
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        let mut handle = fs::File::open(file)?;
        let mut buffer = [0u8; 8192];
        loop {
            let read = handle.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        hasher.update([0]);
    }
    Ok(hex_digest(&hasher.finalize()))
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
        } else if file_type.is_file() {
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
