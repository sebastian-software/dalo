//! Project discovery and isolated stores with portable, commit-pinned declarations.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::audit::{self, AuditOptions, AuditReport};
use crate::catalog::{self, CatalogLock, DriftEntry, SourceLock};
use crate::config::UserConfig;
use crate::error::{DaloError, DaloResult};
use crate::source::{self, SourceConfig, SourceKind};
use crate::{git, store, target};
use toml_edit::{Array as TomlArray, DocumentMut, Item as TomlItem, Value as TomlValue};

/// Separate from the existing team-source `dalo.toml` format.
pub const MANIFEST: &str = "dalo-project.toml";

/// A portable project declaration. Machine paths and approvals are never included.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Project schema, independently versioned from store and team schemas.
    pub schema_version: u32,
    /// Agent IDs whose project folders receive the resolved skills.
    pub targets: Vec<String>,
    /// External sources, in priority order.
    #[serde(default, rename = "source")]
    pub sources: Vec<ProjectSource>,
}

/// One immutable external skill collection.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSource {
    /// Stable source ID, unique within this project.
    pub id: String,
    /// Git remote URL or project-relative local repository path.
    pub url: String,
    /// Full Git commit ID; moving refs are deliberately not supported yet.
    pub commit: String,
    /// Explicit skill selectors, using the existing catalog selection rules.
    pub skills: Vec<String>,
}

/// Review-first preview or result for adding one source to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectAddReport {
    /// Canonical project directory.
    pub project: PathBuf,
    /// Stable source ID being added.
    pub source_id: String,
    /// Portable Git URL or project-relative local repository path.
    pub url: String,
    /// Requested branch, tag, or commit.
    pub requested_ref: String,
    /// Exact commit resolved from the requested revision.
    pub commit: String,
    /// Explicit skills resolved from this exact commit.
    pub skills: Vec<catalog::CatalogCandidate>,
    /// Existing project targets and their bounded folders.
    pub targets: Vec<ProjectAddTarget>,
    /// Exact TOML entry planned for the project declaration.
    pub declaration_change: String,
    /// Whether the declaration was written.
    pub applied: bool,
    /// Whether the caller requested a dry run.
    pub dry_run: bool,
    /// Suggested next CLI action.
    pub next_command: String,
}

/// Review-first preview or result for updating one pinned project source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectUpdateReport {
    /// Canonical project directory.
    pub project: PathBuf,
    /// Stable source ID within the project.
    pub source_id: String,
    /// Redacted source location.
    pub url: String,
    /// Requested branch, tag, or commit.
    pub requested_ref: String,
    /// Previously declared exact commit.
    pub previous_commit: String,
    /// Exact candidate commit.
    pub commit: String,
    /// Selection before and after this update.
    pub selection_before: Vec<String>,
    /// Selection after stable-ID reconciliation or explicit replacement.
    pub selection_after: Vec<String>,
    /// Inventory changes from the previous declared pin.
    pub outcomes: Vec<DriftEntry>,
    /// Changed dependency lists for selected skills.
    pub dependency_changes: Vec<ProjectDependencyChange>,
    /// Selected catalog entries and their same-source dependency closure.
    pub skills: Vec<catalog::CatalogCandidate>,
    /// Deterministic audits for the candidate source's selected skill closure.
    pub audits: Vec<AuditReport>,
    /// Existing project targets and their bounded folders.
    pub targets: Vec<ProjectAddTarget>,
    /// Reasons that prevent safely applying this candidate.
    pub blocking_reasons: Vec<String>,
    /// Exact TOML source entry shown for review.
    pub declaration_change: String,
    /// Whether the declaration was written.
    pub applied: bool,
    /// Whether the caller requested a dry run.
    pub dry_run: bool,
    /// Suggested next CLI action.
    pub next_command: String,
}

/// Dependency-list difference for one selected skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectDependencyChange {
    /// Stable skill ID or repository-relative path.
    pub skill: String,
    /// Previous same-source requirement references.
    pub previous: Vec<String>,
    /// Candidate same-source requirement references.
    pub current: Vec<String>,
}

/// Inputs for resolving and optionally declaring one project source.
pub struct ProjectAddRequest<'a> {
    /// Stable source ID within the project.
    pub id: &'a str,
    /// Git URL or project-relative local repository path.
    pub location: &'a str,
    /// Branch, tag, or commit to resolve.
    pub revision: &'a str,
    /// Explicit skill selectors.
    pub skill_refs: &'a [String],
    /// Reviewed exact commit required to apply a moving ref.
    pub expected_commit: Option<&'a str>,
    /// Whether to write the reviewed declaration entry.
    pub apply: bool,
    /// Whether the invocation is a dry run.
    pub dry_run: bool,
}

/// Inputs for reviewing and optionally declaring one project source update.
pub struct ProjectUpdateRequest<'a> {
    /// Stable source ID already declared in the project.
    pub id: &'a str,
    /// Branch, tag, or commit to resolve.
    pub revision: &'a str,
    /// Optional replacement selection. Empty preserves the existing selection.
    pub skill_refs: &'a [String],
    /// Reviewed exact commit required to apply a moving ref.
    pub expected_commit: Option<&'a str>,
    /// Whether to write the reviewed declaration change.
    pub apply: bool,
    /// Whether the invocation is a dry run.
    pub dry_run: bool,
}

/// One project target affected by a newly declared source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectAddTarget {
    /// Logical agent target ID.
    pub id: String,
    /// Project-relative skill output directory.
    pub directory: String,
}

/// Resolved project scope.
#[derive(Debug)]
pub struct Project {
    /// Canonical project directory.
    pub root: PathBuf,
    /// Independent local store below the project.
    pub store: PathBuf,
}

const PROJECT_UPDATE_JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectUpdateJournal {
    schema_version: u32,
    original_config: UserConfig,
    original_source_lock: SourceLock,
    original_approvals: store::ApprovalsFile,
}

struct ProjectPersistedSnapshot<'a> {
    config: &'a UserConfig,
    source_lock: &'a SourceLock,
    approvals: &'a store::ApprovalsFile,
}

fn invalid(reason: impl Into<String>) -> DaloError {
    DaloError::InvalidArgument {
        reason: reason.into(),
    }
}

fn is_full_commit_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn portable_source_location(location: &str, root: &Path) -> DaloResult<String> {
    let resolved = source::resolve_source_location(location, root);
    let resolved_path = Path::new(&resolved);
    if git::looks_like_remote_location(location) && !resolved_path.exists() {
        git::validate_remote_url(location)?;
        return Ok(location.to_owned());
    }

    let canonical = fs::canonicalize(resolved_path).map_err(|error| {
        invalid(format!(
            "local source `{location}` must exist before it can be added to a project: {error}"
        ))
    })?;
    if !canonical.is_dir() {
        return Err(invalid(format!(
            "local project source `{location}` must be a Git repository directory"
        )));
    }
    let relative = canonical.strip_prefix(root).map_err(|_| {
        invalid(format!(
            "local source `{location}` is outside this project; project declarations may only contain project-relative local paths"
        ))
    })?;
    if relative.as_os_str().is_empty() {
        Ok(".".to_owned())
    } else {
        Ok(relative.to_string_lossy().into_owned())
    }
}

fn project_source_entry(source: &ProjectSource) -> DaloResult<String> {
    let fields = toml::to_string(source)?;
    Ok(format!("[[source]]\n{fields}"))
}

fn project_add_command(
    root: &Path,
    id: &str,
    location: &str,
    revision: &str,
    skills: &[String],
    expected_commit: Option<&str>,
) -> String {
    let mut command = format!(
        "dalo --project {} project add {} {} --ref {}",
        crate::error::shell_quote_path(root),
        crate::error::shell_quote_argument(id),
        crate::error::shell_quote_argument(location),
        crate::error::shell_quote_argument(revision),
    );
    if let Some(expected) = expected_commit {
        command.push_str(" --expect-commit ");
        command.push_str(expected);
    }
    for skill in skills {
        command.push_str(" --skill ");
        command.push_str(&crate::error::shell_quote_argument(skill));
    }
    command.push_str(" --apply");
    command
}

fn project_update_command(
    root: &Path,
    id: &str,
    revision: &str,
    skills: &[String],
    expected_commit: Option<&str>,
) -> String {
    let mut command = format!(
        "dalo --project {} project update {} --ref {}",
        crate::error::shell_quote_path(root),
        crate::error::shell_quote_argument(id),
        crate::error::shell_quote_argument(revision),
    );
    if let Some(expected) = expected_commit {
        command.push_str(" --expect-commit ");
        command.push_str(expected);
    }
    for skill in skills {
        command.push_str(" --skill ");
        command.push_str(&crate::error::shell_quote_argument(skill));
    }
    command.push_str(" --apply");
    command
}

fn catalog_selection_closure(
    inventory: &[catalog::CatalogEntry],
    source_id: &str,
    selection: &[String],
) -> Vec<String> {
    let mut queue = VecDeque::from(selection.to_vec());
    let mut closure = BTreeSet::new();
    while let Some(reference) = queue.pop_front() {
        let lookup = reference
            .strip_prefix(&format!("{source_id}:"))
            .unwrap_or(&reference);
        let Some(entry) = inventory.iter().find(|entry| {
            entry.slot_name == lookup || entry.path == lookup || entry.id.as_deref() == Some(lookup)
        }) else {
            continue;
        };
        let canonical = entry.id.as_deref().unwrap_or(&entry.path).to_owned();
        if closure.insert(canonical) {
            queue.extend(entry.requires.iter().cloned());
        }
    }
    closure.into_iter().collect()
}

fn append_manifest_source(
    root: &Path,
    original_bytes: &[u8],
    source: &ProjectSource,
    addition: &str,
) -> DaloResult<()> {
    let original = std::str::from_utf8(original_bytes)
        .map_err(|_| invalid(format!("{MANIFEST} must contain valid UTF-8")))?;
    let current_manifest: Manifest = toml::from_str(original)
        .map_err(|error| invalid(format!("invalid {MANIFEST}: {error}")))?;
    if current_manifest
        .sources
        .iter()
        .any(|existing| existing.id == source.id)
    {
        return Err(invalid(format!(
            "project source `{}` was added while the preview was being prepared; no files were changed",
            source.id
        )));
    }

    let separator = if original.is_empty() || original.ends_with("\n\n") {
        ""
    } else if original.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let mut updated = String::with_capacity(original.len() + separator.len() + addition.len());
    updated.push_str(original);
    updated.push_str(separator);
    updated.push_str(addition);
    let _: Manifest = toml::from_str(&updated)
        .map_err(|error| invalid(format!("project declaration update is invalid: {error}")))?;

    write_manifest_if_unchanged(root, original_bytes, updated.as_bytes())
}

fn replace_manifest_source(
    root: &Path,
    original_bytes: &[u8],
    source_id: &str,
    commit: &str,
    skills: &[String],
) -> DaloResult<String> {
    let original = std::str::from_utf8(original_bytes)
        .map_err(|_| invalid(format!("{MANIFEST} must contain valid UTF-8")))?;
    let mut document = original
        .parse::<DocumentMut>()
        .map_err(|error| invalid(format!("invalid {MANIFEST}: {error}")))?;
    let sources = document
        .get_mut("source")
        .and_then(TomlItem::as_array_of_tables_mut)
        .ok_or_else(|| invalid(format!("{MANIFEST} has no source entries")))?;
    let table = sources
        .iter_mut()
        .find(|table| table.get("id").and_then(TomlItem::as_str) == Some(source_id))
        .ok_or_else(|| invalid(format!("project source `{source_id}` does not exist")))?;

    let commit_item = table
        .get_mut("commit")
        .and_then(TomlItem::as_value_mut)
        .ok_or_else(|| invalid(format!("project source `{source_id}` has no commit")))?;
    let commit_decor = commit_item.decor().clone();
    let mut new_commit = TomlValue::from(commit);
    *new_commit.decor_mut() = commit_decor;
    *commit_item = new_commit;

    let skills_item = table
        .get_mut("skills")
        .and_then(TomlItem::as_value_mut)
        .and_then(TomlValue::as_array_mut)
        .ok_or_else(|| invalid(format!("project source `{source_id}` has no skill array")))?;
    let old_array = skills_item.clone();
    let mut new_array = TomlArray::new();
    *new_array.decor_mut() = old_array.decor().clone();
    new_array.set_trailing_comma(old_array.trailing_comma());
    new_array.set_trailing(old_array.trailing().clone());
    for skill in skills {
        if let Some(existing) = old_array.iter().find(|item| item.as_str() == Some(skill)) {
            new_array.push_formatted(existing.clone());
        } else {
            new_array.push(skill);
        }
    }
    *skills_item = new_array;

    let updated = document.to_string();
    let _: Manifest = toml::from_str(&updated)
        .map_err(|error| invalid(format!("project declaration update is invalid: {error}")))?;
    write_manifest_if_unchanged(root, original_bytes, updated.as_bytes())?;
    Ok(updated)
}

fn write_manifest_if_unchanged(
    root: &Path,
    original_bytes: &[u8],
    updated: &[u8],
) -> DaloResult<()> {
    let manifest_path = root.join(MANIFEST);
    check_path(root, Path::new(MANIFEST))?;
    let permissions = fs::metadata(&manifest_path)?.permissions();
    let mut temporary = NamedTempFile::new_in(root)?;
    temporary.as_file().set_permissions(permissions)?;
    temporary.write_all(updated)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;

    check_path(root, Path::new(MANIFEST))?;
    if fs::read(&manifest_path)? != original_bytes {
        return Err(invalid(format!(
            "{MANIFEST} changed while the source was being prepared; no files were changed"
        )));
    }
    temporary
        .persist(&manifest_path)
        .map_err(|error| error.error)?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}

fn target_folder(id: &str) -> DaloResult<&'static str> {
    match id {
        "claude" => Ok(".claude/skills"),
        "codex" => Ok(".agents/skills"),
        "opencode" => Ok(".opencode/skills"),
        "hermes" => Ok(".hermes/skills"),
        "openclaw" => Ok(".agents/skills"),
        _ => Err(invalid(format!("unsupported project target `{id}`"))),
    }
}

/// Refuse redirected output directories, including dangling symlinks.
fn check_path(root: &Path, relative: &Path) -> DaloResult<()> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid(format!(
                    "project path `{}` must not be a symlink",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Find the nearest declaration, stopping at a Git checkout/worktree boundary.
/// A present but invalid declaration is returned so it cannot fall back globally.
pub fn discover(start: &Path) -> DaloResult<Option<PathBuf>> {
    let start = fs::canonicalize(start)?;
    for directory in start.ancestors() {
        if entry_exists(&directory.join(MANIFEST))? {
            return Ok(Some(directory.to_path_buf()));
        }
        if entry_exists(&directory.join(".git"))? {
            break;
        }
    }
    Ok(None)
}

/// Find the enclosing Git boundary for an interactive first-time scope choice.
pub fn repository_root(start: &Path) -> DaloResult<Option<PathBuf>> {
    let start = fs::canonicalize(start)?;
    for directory in start.ancestors() {
        if entry_exists(&directory.join(".git"))? {
            return Ok(Some(directory.to_path_buf()));
        }
    }
    Ok(None)
}

fn entry_exists(path: &Path) -> DaloResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn ensure_project_directory(root: &Path, relative: &Path) -> DaloResult<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid(format!(
                    "project store directory `{}` is not a real directory",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(invalid(format!(
                                "project store directory `{}` is not a real directory",
                                current.display()
                            )));
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

impl Project {
    /// Resolve only the explicitly selected directory; never search parent folders.
    pub fn new(root: &Path) -> DaloResult<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(invalid("--project requires an existing directory"));
        }
        check_path(&root, Path::new(".dalo"))?;
        if git::is_worktree(&root)? && git::is_tracked_file(&root, &root.join(".dalo"))? {
            return Err(invalid(
                "project store .dalo must not be tracked in Git; approvals and local state are machine-specific",
            ));
        }
        Ok(Self {
            store: root.join(".dalo"),
            root,
        })
    }

    /// Read a bounded, validated project manifest without changing state.
    pub fn manifest(&self) -> DaloResult<Manifest> {
        check_path(&self.root, Path::new(MANIFEST))?;
        let mut text = String::new();
        fs::File::open(self.root.join(MANIFEST))?
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        if text.len() > 1024 * 1024 {
            return Err(invalid("project manifest exceeds 1 MiB"));
        }
        let manifest: Manifest =
            toml::from_str(&text).map_err(|e| invalid(format!("invalid {MANIFEST}: {e}")))?;
        if manifest.schema_version != 1 {
            return Err(invalid("unsupported project schema_version; expected 1"));
        }
        if manifest.targets.is_empty() {
            return Err(invalid("project targets must not be empty"));
        }
        let mut targets = BTreeSet::new();
        for id in &manifest.targets {
            let folder = target_folder(id)?;
            if !targets.insert(folder) {
                return Err(invalid("project targets must have distinct folders"));
            }
            check_path(&self.root, Path::new(folder))?;
        }
        let mut ids = BTreeSet::new();
        for source in &manifest.sources {
            if source.id == "local"
                || !source::is_valid_source_id(&source.id)
                || !ids.insert(&source.id)
            {
                return Err(invalid(format!(
                    "invalid or duplicate project source `{}`",
                    source.id
                )));
            }
            if !matches!(source.commit.len(), 40 | 64)
                || !source
                    .commit
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(invalid(format!(
                    "project source `{}` requires a full lowercase commit ID",
                    source.id
                )));
            }
            git::validate_remote_url(&source.url)?;
            if source.skills.is_empty() || source.skills.iter().any(|s| s.trim().is_empty()) {
                return Err(invalid(format!(
                    "project source `{}` needs explicit skill selectors",
                    source.id
                )));
            }
        }
        Ok(manifest)
    }

    /// Resolve and optionally add one explicitly selected catalog to the
    /// portable project declaration. A moving ref can only be applied when the
    /// caller confirms the exact commit shown by a prior preview.
    pub fn add_source(&self, request: ProjectAddRequest<'_>) -> DaloResult<ProjectAddReport> {
        let ProjectAddRequest {
            id,
            location,
            revision,
            skill_refs,
            expected_commit,
            apply,
            dry_run,
        } = request;
        let manifest_path = self.root.join(MANIFEST);
        check_path(&self.root, Path::new(MANIFEST))?;
        if !manifest_path.is_file() {
            return Err(invalid(format!(
                "project definition is missing; initialize it with `dalo --project {} init` first",
                crate::error::shell_quote_path(&self.root)
            )));
        }

        let original_bytes = fs::read(&manifest_path)?;
        let manifest = self.manifest()?;
        self.validate_store(&manifest)?;
        if id == "local" || !source::is_valid_source_id(id) {
            return Err(invalid(format!(
                "invalid project source ID `{id}`; {}",
                source::SOURCE_ID_REQUIREMENTS
            )));
        }
        if manifest.sources.iter().any(|source| source.id == id) {
            return Err(invalid(format!(
                "project source `{id}` already exists; project update is a separate reviewed operation"
            )));
        }
        if skill_refs.is_empty()
            || skill_refs
                .iter()
                .any(|reference| reference.trim().is_empty())
        {
            return Err(invalid(
                "select at least one skill with `--skill`; project sources never select everything by default",
            ));
        }
        git::validate_manifest_revision(revision)?;
        if let Some(expected) = expected_commit
            && !is_full_commit_id(expected)
        {
            return Err(invalid(
                "--expect-commit requires a full lowercase Git commit ID",
            ));
        }
        if apply && !is_full_commit_id(revision) && expected_commit.is_none() {
            return Err(invalid(
                "applying a branch or tag requires `--expect-commit` from the preview so a moved ref cannot change the reviewed source",
            ));
        }

        let declaration_url = portable_source_location(location, &self.root)?;
        let clone_url = source::resolve_source_location(location, &self.root);
        git::validate_remote_url(&clone_url)?;
        let staging = tempfile::tempdir()?;
        let checkout = staging.path().join("checkout");
        source::clone_source_checkout(&clone_url, &checkout)?;
        let commit = git::resolve_manifest_revision(&checkout, revision)?;
        if let Some(expected) = expected_commit
            && commit != expected
        {
            return Err(invalid(format!(
                "ref `{revision}` now resolves to {commit}, not previewed commit {expected}; no project files were changed"
            )));
        }
        git::checkout_detached(&checkout, &commit)?;
        let selected = catalog::resolve_catalog_skill_references(id, &checkout, skill_refs)?;
        let canonical_skills = selected
            .iter()
            .map(|candidate| {
                candidate
                    .id
                    .clone()
                    .unwrap_or_else(|| candidate.path.clone())
            })
            .collect::<Vec<_>>();
        let new_source = ProjectSource {
            id: id.to_owned(),
            url: declaration_url.clone(),
            commit: commit.clone(),
            skills: canonical_skills,
        };
        let declaration_change = project_source_entry(&new_source)?;
        let targets = manifest
            .targets
            .iter()
            .map(|target_id| {
                Ok(ProjectAddTarget {
                    id: target_id.clone(),
                    directory: target_folder(target_id)?.to_owned(),
                })
            })
            .collect::<DaloResult<Vec<_>>>()?;
        let apply_command = project_add_command(
            &self.root,
            id,
            &declaration_url,
            revision,
            skill_refs,
            (!is_full_commit_id(revision)).then_some(commit.as_str()),
        );
        let mut report = ProjectAddReport {
            project: self.root.clone(),
            source_id: id.to_owned(),
            url: git::display_remote_url(&declaration_url),
            requested_ref: revision.to_owned(),
            commit,
            skills: selected,
            targets,
            declaration_change,
            applied: false,
            dry_run,
            next_command: if apply && !dry_run {
                "dalo install".to_owned()
            } else {
                apply_command
            },
        };

        if apply && !dry_run {
            append_manifest_source(
                &self.root,
                &original_bytes,
                &new_source,
                &report.declaration_change,
            )?;
            report.applied = true;
        }
        Ok(report)
    }

    /// Review a pinned project source at a new exact commit and optionally
    /// update only its portable declaration. Installing the new pin remains a
    /// separate, explicit `dalo install` operation.
    pub fn update_source(
        &self,
        request: ProjectUpdateRequest<'_>,
    ) -> DaloResult<ProjectUpdateReport> {
        let ProjectUpdateRequest {
            id,
            revision,
            skill_refs,
            expected_commit,
            apply,
            dry_run,
        } = request;
        let manifest_path = self.root.join(MANIFEST);
        check_path(&self.root, Path::new(MANIFEST))?;
        if !manifest_path.is_file() {
            return Err(invalid(format!(
                "project definition is missing; initialize it with `dalo --project {} init` first",
                crate::error::shell_quote_path(&self.root)
            )));
        }
        let original_bytes = fs::read(&manifest_path)?;
        let manifest = self.manifest()?;
        self.validate_store(&manifest)?;
        let declared = manifest
            .sources
            .iter()
            .find(|source| source.id == id)
            .ok_or_else(|| invalid(format!("project source `{id}` does not exist")))?;
        if skill_refs.iter().any(|skill| skill.trim().is_empty()) {
            return Err(invalid("--skill values must not be empty"));
        }
        git::validate_manifest_revision(revision)?;
        if let Some(expected) = expected_commit
            && !is_full_commit_id(expected)
        {
            return Err(invalid(
                "--expect-commit requires a full lowercase Git commit ID",
            ));
        }
        if apply && !is_full_commit_id(revision) && expected_commit.is_none() {
            return Err(invalid(
                "applying a branch or tag requires `--expect-commit` from the preview so a moved ref cannot change the reviewed source",
            ));
        }

        let url = source::resolve_source_location(&declared.url, &self.root);
        git::validate_remote_url(&url)?;
        let staging = tempfile::tempdir()?;
        let checkout = staging.path().join("checkout");
        source::clone_source_checkout(&url, &checkout)?;
        let commit = git::resolve_manifest_revision(&checkout, revision)?;
        if let Some(expected) = expected_commit
            && commit != expected
        {
            return Err(invalid(format!(
                "ref `{revision}` now resolves to {commit}, not previewed commit {expected}; no project files were changed"
            )));
        }

        git::checkout_detached(&checkout, &declared.commit)?;
        let old_inventory_metadata = catalog::catalog_inventory(&checkout, &[])?;
        let old_selection_closure =
            catalog_selection_closure(&old_inventory_metadata, id, &declared.skills);
        let old_inventory = catalog::catalog_inventory(&checkout, &old_selection_closure)?;
        git::checkout_detached(&checkout, &commit)?;
        let mut selection_after = if skill_refs.is_empty() {
            declared.skills.clone()
        } else {
            skill_refs.to_vec()
        };
        let new_inventory_metadata = catalog::catalog_inventory(&checkout, &[])?;
        let new_selection_closure =
            catalog_selection_closure(&new_inventory_metadata, id, &selection_after);
        let new_inventory = catalog::catalog_inventory(&checkout, &new_selection_closure)?;
        let outcomes = catalog::compare_catalog_inventory(
            &old_inventory,
            &old_selection_closure,
            &new_inventory,
        );

        let mut selected = Vec::new();
        let mut blocking_reasons = Vec::new();
        for skill in &selection_after {
            match catalog::resolve_catalog_skill_references(id, &checkout, std::slice::from_ref(skill)) {
                Ok(mut candidates) => selected.append(&mut candidates),
                Err(error) => blocking_reasons.push(format!(
                    "selected skill `{skill}` cannot be resolved at candidate commit {commit}: {error}"
                )),
            }
        }
        selected.sort_by(|left, right| {
            left.slot_name
                .cmp(&right.slot_name)
                .then(left.path.cmp(&right.path))
        });
        selected.dedup_by(|left, right| left.id == right.id && left.path == right.path);
        let canonical_selection = selected
            .iter()
            .map(|candidate| {
                candidate
                    .id
                    .clone()
                    .unwrap_or_else(|| candidate.path.clone())
            })
            .collect::<Vec<_>>();
        if blocking_reasons.is_empty() {
            selection_after = canonical_selection;
        }
        if selection_after.is_empty() {
            blocking_reasons.push(
                "a project source must keep at least one explicitly selected skill".to_owned(),
            );
        }
        if outcomes
            .iter()
            .any(|outcome| outcome.code == catalog::DriftCode::SelectedRemoved)
            && skill_refs.is_empty()
        {
            for outcome in outcomes
                .iter()
                .filter(|outcome| outcome.code == catalog::DriftCode::SelectedRemoved)
            {
                blocking_reasons.push(format!(
                    "selected skill `{}` was removed; pass an explicit replacement selection with `--skill`",
                    outcome.skill
                ));
            }
        }

        let audited_skills = match catalog::resolve_catalog_skill_references(
            id,
            &checkout,
            &new_selection_closure,
        ) {
            Ok(skills) => skills,
            Err(error) => {
                blocking_reasons.push(format!(
                    "candidate dependency closure cannot be resolved at commit {commit}: {error}"
                ));
                selected.clone()
            }
        };
        let audit_temp = tempfile::tempdir()?;
        let audit_paths = store::StorePaths::new(if self.store.exists() {
            self.store.clone()
        } else {
            audit_temp.path().join("store")
        });
        let audits = audited_skills
            .iter()
            .map(|candidate| {
                audit::audit_skill(
                    &audit_paths,
                    &format!("{id}:{}", candidate.slot_name),
                    &checkout.join(&candidate.path),
                    &AuditOptions {
                        persist: false,
                        exclude_root_source_metadata: true,
                        ..AuditOptions::default()
                    },
                )
            })
            .collect::<DaloResult<Vec<_>>>()?;
        for audit in &audits {
            if audit.is_blocking() {
                blocking_reasons.push(format!(
                    "security audit blocks candidate skill `{}`; review it before installing",
                    audit.source_ref
                ));
            }
        }

        let dependency_changes = audited_skills
            .iter()
            .filter_map(|candidate| {
                let reference = candidate.id.as_deref().unwrap_or(&candidate.path);
                let previous_requires = old_inventory
                    .iter()
                    .find(|entry| {
                        entry.id.as_deref() == Some(reference) || entry.path == candidate.path
                    })
                    .map(|entry| entry.requires.clone())
                    .unwrap_or_default();
                (previous_requires != candidate.requires).then(|| ProjectDependencyChange {
                    skill: reference.to_owned(),
                    previous: previous_requires,
                    current: candidate.requires.clone(),
                })
            })
            .collect::<Vec<_>>();
        let targets = manifest
            .targets
            .iter()
            .map(|target_id| {
                Ok(ProjectAddTarget {
                    id: target_id.clone(),
                    directory: target_folder(target_id)?.to_owned(),
                })
            })
            .collect::<DaloResult<Vec<_>>>()?;
        let replacement = ProjectSource {
            id: declared.id.clone(),
            url: declared.url.clone(),
            commit: commit.clone(),
            skills: selection_after.clone(),
        };
        let declaration_change = project_source_entry(&replacement)?;
        let can_apply = blocking_reasons.is_empty();
        let apply_command = project_update_command(
            &self.root,
            id,
            revision,
            skill_refs,
            (!is_full_commit_id(revision)).then_some(commit.as_str()),
        );
        let mut report = ProjectUpdateReport {
            project: self.root.clone(),
            source_id: id.to_owned(),
            url: git::display_remote_url(&declared.url),
            requested_ref: revision.to_owned(),
            previous_commit: declared.commit.clone(),
            commit,
            selection_before: declared.skills.clone(),
            selection_after,
            outcomes,
            dependency_changes,
            skills: audited_skills,
            audits,
            targets,
            blocking_reasons,
            declaration_change,
            applied: false,
            dry_run,
            next_command: if apply && !dry_run {
                if can_apply {
                    "dalo install".to_owned()
                } else {
                    "resolve the blocking reasons above, then retry this project update".to_owned()
                }
            } else {
                apply_command
            },
        };
        report.blocking_reasons.sort();
        report.blocking_reasons.dedup();

        if apply && !dry_run && report.blocking_reasons.is_empty() {
            replace_manifest_source(
                &self.root,
                &original_bytes,
                id,
                &report.commit,
                &report.selection_after,
            )?;
            report.applied = true;
        }
        Ok(report)
    }

    /// Create only the portable definition; existing files are never replaced.
    pub fn init(&self, dry_run: bool) -> DaloResult<()> {
        check_path(&self.root, Path::new(MANIFEST))?;
        if self.root.join(MANIFEST).exists() {
            self.manifest()?;
            return Ok(());
        }
        if !dry_run {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join(MANIFEST))?;
            file.write_all(b"schema_version = 1\ntargets = [\"claude\", \"codex\"]\n\n# Add [[source]] entries with id, url, commit and skills.\n")?;
        }
        Ok(())
    }

    /// Validate the existing local store against the declaration before using it.
    pub fn validate_store(&self, manifest: &Manifest) -> DaloResult<()> {
        self.validate_store_mode(manifest, false)
    }

    /// Validate local state before reconciling an explicitly updated project pin.
    pub fn validate_store_for_install(&self, manifest: &Manifest) -> DaloResult<()> {
        self.validate_store_mode(manifest, true)
    }

    fn validate_store_mode(&self, manifest: &Manifest, allow_pin_updates: bool) -> DaloResult<()> {
        if !self.store.exists() {
            return Ok(());
        }
        check_path(&self.root, Path::new(".dalo/project-owner"))?;
        if fs::read_to_string(self.store.join("project-owner"))
            .ok()
            .as_deref()
            != Some("dalo-project-v1\n")
        {
            return Err(invalid(
                "existing .dalo is not a project-owned store; preserve it before installing",
            ));
        }
        let paths = store::StorePaths::new(self.store.clone());
        for relative in [
            "config.toml",
            "state.toml",
            "lock.toml",
            "source-lock.toml",
            "project-update.toml",
            "approvals.toml",
            "local",
            "local/skills",
            "sources",
            "audits",
            "generated",
            "tools",
        ] {
            check_path(&self.root, &Path::new(".dalo").join(relative))?;
        }
        if self.store.join("project-update.toml").exists() {
            return Err(invalid(
                "project source update was interrupted; run `dalo install` to recover it before using the store",
            ));
        }
        let config = store::read_config(&paths)?;
        for configured in &config.sources {
            if configured.id == "local" {
                if configured.path != paths.local_dir || configured.kind != SourceKind::Local {
                    return Err(invalid(
                        "project local source must remain inside the project store",
                    ));
                }
                continue;
            }
            let declared = manifest.sources.iter().find(|s| s.id == configured.id)
                .ok_or_else(|| invalid("project source removal requires an explicit migration; install leaves existing sources untouched"))?;
            let expected_priority = manifest
                .sources
                .iter()
                .position(|s| s.id == configured.id)
                .expect("declared source")
                + 1;
            let url = source::resolve_source_location(&declared.url, &self.root);
            let expected_path = paths.sources_dir.join(&declared.id).join("checkout");
            if configured.kind != SourceKind::Catalog
                || configured.url.as_deref() != Some(&url)
                || !configured.enabled
                || configured.subpath.is_some()
                || configured.namespace.is_some()
                || usize::try_from(configured.priority).ok() != Some(expected_priority)
            {
                return Err(invalid(format!(
                    "project source `{}` differs from its declaration",
                    declared.id
                )));
            }
            let versioned_root = paths.sources_dir.join(&declared.id).join("checkouts");
            let is_known_checkout = configured.path == expected_path
                || configured.path.parent() == Some(versioned_root.as_path())
                    && configured
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(is_full_commit_id);
            if !is_known_checkout
                || (!allow_pin_updates
                    && configured.path != expected_path
                    && configured.path != versioned_root.join(&declared.commit))
            {
                return Err(invalid(format!(
                    "project source `{}` has an unmanaged checkout path",
                    declared.id
                )));
            }
            let configured_relative = configured.path.strip_prefix(&self.root).map_err(|_| {
                invalid(format!(
                    "project source `{}` checkout is outside the project store",
                    declared.id
                ))
            })?;
            check_path(&self.root, configured_relative)?;
            let lock = catalog::read_source_lock(&paths)?;
            let locked = lock
                .catalogs
                .iter()
                .find(|catalog| catalog.source_id == declared.id);
            let checkout_commit = git::rev_parse_head(&configured.path)?;
            let versioned_name_matches_head = configured.path == expected_path
                || configured.path.file_name().and_then(|name| name.to_str())
                    == Some(checkout_commit.as_str());
            if locked.map(|catalog| catalog.commit.as_str()) != Some(checkout_commit.as_str())
                || locked.is_some_and(|catalog| catalog.selected != configured.selection)
                || !versioned_name_matches_head
                || git::is_dirty(&configured.path)?
            {
                return Err(invalid(format!(
                    "project source `{}` has an inconsistent pin or local edits; install will not overwrite it",
                    declared.id
                )));
            }
            if !allow_pin_updates && checkout_commit != declared.commit {
                return Err(invalid(format!(
                    "project source `{}` has a changed pin; run `dalo install` to reconcile the reviewed declaration",
                    declared.id
                )));
            }
        }
        for declared in &manifest.sources {
            if let Some(configured) = config.sources.iter().find(|s| s.id == declared.id)
                && !allow_pin_updates
                && !configured.selection.is_empty()
                && configured.selection
                    != catalog::canonical_skill_selection(&paths, &declared.id, &declared.skills)?
            {
                return Err(invalid(
                    "project selection differs from its local installation; run `dalo install` to reconcile it",
                ));
            }
        }
        let state = store::read_state(&paths)?;
        for configured in &state.targets {
            if !manifest.targets.contains(&configured.id)
                || configured.path != self.root.join(target_folder(&configured.id)?)
                || configured.canonical_path != configured.path
                || !configured.enabled
            {
                return Err(invalid(
                    "project target differs from the declaration; install will not redirect it",
                ));
            }
        }
        Ok(())
    }

    /// Restore config and source-lock snapshots if a project pin update was
    /// interrupted between the two atomic file replacements.
    pub fn recover_interrupted_update(&self) -> DaloResult<()> {
        if !self.store.exists() {
            return Ok(());
        }
        check_path(&self.root, Path::new(".dalo/project-update.toml"))?;
        let paths = store::StorePaths::new(self.store.clone());
        check_path(&self.root, Path::new(".dalo/project-owner"))?;
        if fs::read_to_string(paths.root.join("project-owner"))?.as_str() != "dalo-project-v1\n" {
            return Err(invalid(
                "existing .dalo is not a project-owned store; preserve it before install recovery",
            ));
        }
        for path in ["config.toml", "source-lock.toml", "approvals.toml"] {
            check_path(&self.root, &Path::new(".dalo").join(path))?;
        }
        if !paths.root.join("project-update.toml").exists() {
            return Ok(());
        }
        let journal: ProjectUpdateJournal = toml::from_str(&fs::read_to_string(
            paths.root.join("project-update.toml"),
        )?)
        .map_err(|error| invalid(format!("invalid project update recovery record: {error}")))?;
        if journal.schema_version != PROJECT_UPDATE_JOURNAL_SCHEMA_VERSION {
            return Err(invalid(
                "unsupported project update recovery record version",
            ));
        }
        store::write_config(&paths, &journal.original_config)?;
        catalog::write_source_lock(&paths, &journal.original_source_lock)?;
        if store::read_approvals(&paths)? != journal.original_approvals {
            store::write_approvals(&paths, &journal.original_approvals)?;
        }
        fs::remove_file(paths.root.join("project-update.toml"))?;
        fs::File::open(&paths.root)?.sync_all()?;
        Ok(())
    }

    /// Prepare immutable, untrusted catalogs. The caller holds the store lock.
    pub fn prepare(&self, manifest: &Manifest) -> DaloResult<()> {
        self.validate_store_for_install(manifest)?;
        let paths = store::StorePaths::new(self.store.clone());
        let mut config = store::read_config(&paths)?;
        for (index, declared) in manifest.sources.iter().enumerate() {
            let url = source::resolve_source_location(&declared.url, &self.root);
            let mut source_lock = catalog::read_source_lock(&paths)?;
            let original_source_lock = source_lock.clone();
            let original_approvals = store::read_approvals(&paths)?;
            let configured_index = config
                .sources
                .iter()
                .position(|source| source.id == declared.id);
            let checkout = if let Some(source_index) = configured_index {
                let configured = config.sources[source_index].clone();
                let current_lock = source_lock
                    .catalogs
                    .iter()
                    .find(|entry| entry.source_id == declared.id)
                    .ok_or_else(|| {
                        invalid(format!(
                            "project source `{}` has no local pin record",
                            declared.id
                        ))
                    })?;
                let current_head = git::rev_parse_head(&configured.path)?;
                if current_lock.commit == declared.commit && current_head == declared.commit {
                    configured.path
                } else {
                    let mut previous_snapshot = current_lock.clone();
                    let old_selection = if configured.selection.is_empty() {
                        &current_lock.selected
                    } else {
                        &configured.selection
                    };
                    previous_snapshot.inventory =
                        catalog::catalog_inventory(&configured.path, old_selection)?;
                    let checkout = paths
                        .sources_dir
                        .join(&declared.id)
                        .join("checkouts")
                        .join(&declared.commit);
                    self.prepare_versioned_checkout(&url, declared, &checkout)?;
                    let inventory = catalog::catalog_inventory(&checkout, &[])?;
                    let original_approvals = store::read_approvals(&paths)?;
                    let mut next_approvals = original_approvals.clone();
                    Self::revoke_changed_skill_approvals(
                        &mut next_approvals,
                        &declared.id,
                        &previous_snapshot,
                        &catalog::catalog_inventory(&checkout, &declared.skills)?,
                    );
                    let mut next_config = config.clone();
                    let next_source = &mut next_config.sources[source_index];
                    next_source.path = checkout.clone();
                    next_source.selection.clear();
                    source_lock
                        .catalogs
                        .retain(|entry| entry.source_id != declared.id);
                    source_lock.catalogs.push(CatalogLock {
                        source_id: declared.id.clone(),
                        commit: declared.commit.clone(),
                        selected: Vec::new(),
                        inventory,
                    });
                    source_lock
                        .catalogs
                        .sort_by(|left, right| left.source_id.cmp(&right.source_id));
                    self.persist_source_update(
                        &paths,
                        ProjectPersistedSnapshot {
                            config: &config,
                            source_lock: &original_source_lock,
                            approvals: &original_approvals,
                        },
                        ProjectPersistedSnapshot {
                            config: &next_config,
                            source_lock: &source_lock,
                            approvals: &next_approvals,
                        },
                    )?;
                    config = next_config;
                    checkout
                }
            } else {
                if source_lock
                    .catalogs
                    .iter()
                    .any(|entry| entry.source_id == declared.id)
                {
                    return Err(invalid(format!(
                        "unregistered local pin for project source `{}` exists; preserve it before retrying",
                        declared.id
                    )));
                }
                let checkout = paths.sources_dir.join(&declared.id).join("checkout");
                self.prepare_versioned_checkout(&url, declared, &checkout)?;
                let inventory = catalog::catalog_inventory(&checkout, &[])?;
                let mut next_config = config.clone();
                next_config.sources.push(SourceConfig {
                    id: declared.id.clone(),
                    kind: SourceKind::Catalog,
                    path: checkout.clone(),
                    subpath: None,
                    priority: i32::try_from(index + 1)
                        .map_err(|_| invalid("too many project sources"))?,
                    namespace: None,
                    enabled: true,
                    trusted: false,
                    url: Some(url.clone()),
                    branch: None,
                    update_policy: Some("pin".into()),
                    selection: vec![],
                    declared_by: None,
                    declared_ref: None,
                });
                source_lock.catalogs.push(CatalogLock {
                    source_id: declared.id.clone(),
                    commit: declared.commit.clone(),
                    selected: Vec::new(),
                    inventory,
                });
                source_lock
                    .catalogs
                    .sort_by(|left, right| left.source_id.cmp(&right.source_id));
                self.persist_source_update(
                    &paths,
                    ProjectPersistedSnapshot {
                        config: &config,
                        source_lock: &original_source_lock,
                        approvals: &original_approvals,
                    },
                    ProjectPersistedSnapshot {
                        config: &next_config,
                        source_lock: &source_lock,
                        approvals: &original_approvals,
                    },
                )?;
                config = next_config;
                checkout
            };

            let configured = config
                .sources
                .iter()
                .find(|source| source.id == declared.id)
                .expect("registered project source");
            if configured.path != checkout {
                return Err(invalid(
                    "project source checkout changed during preparation",
                ));
            }
            let desired =
                catalog::canonical_skill_selection(&paths, &declared.id, &declared.skills)?;
            if configured.selection != desired {
                let old_selection = configured.selection.clone();
                if !old_selection.is_empty() {
                    catalog::select_skills(&paths, &declared.id, &old_selection, true, false)?;
                }
                catalog::select_skills(&paths, &declared.id, &declared.skills, false, false)?;
                config = store::read_config(&paths)?;
            }
        }
        for id in &manifest.targets {
            target::link_target(
                &self.store,
                id,
                Some(&self.root.join(target_folder(id)?)),
                false,
            )?;
        }
        Ok(())
    }

    fn prepare_versioned_checkout(
        &self,
        url: &str,
        declared: &ProjectSource,
        checkout: &Path,
    ) -> DaloResult<()> {
        let relative = checkout
            .strip_prefix(&self.root)
            .map_err(|_| invalid("project source checkout is outside the project store"))?;
        check_path(&self.root, relative)?;
        if checkout.exists() {
            let head = git::rev_parse_head(checkout)?;
            if head != declared.commit || git::is_dirty(checkout)? {
                return Err(invalid(format!(
                    "project checkout `{}` already exists with an unexpected pin or local edits; preserve it before retrying",
                    checkout.display()
                )));
            }
            return Ok(());
        }
        let staging = tempfile::tempdir_in(&self.store)?;
        let clone = staging.path().join("checkout");
        source::clone_source_checkout(url, &clone)?;
        let commit = git::resolve_manifest_revision(&clone, &declared.commit)?;
        if commit != declared.commit {
            return Err(invalid("resolved project commit differs from declaration"));
        }
        git::checkout_detached(&clone, &commit)?;
        let parent = checkout.parent().expect("project checkout parent");
        let parent_relative = parent
            .strip_prefix(&self.root)
            .map_err(|_| invalid("project checkout parent is outside the project store"))?;
        ensure_project_directory(&self.root, parent_relative)?;
        check_path(&self.root, relative)?;
        fs::rename(&clone, checkout)?;
        Ok(())
    }

    fn revoke_changed_skill_approvals(
        approvals: &mut store::ApprovalsFile,
        source_id: &str,
        previous: &CatalogLock,
        candidate: &[catalog::CatalogEntry],
    ) {
        let changed =
            catalog::compare_catalog_inventory(&previous.inventory, &previous.selected, candidate)
                .into_iter()
                .filter(|outcome| outcome.code == catalog::DriftCode::SelectedChanged)
                .map(|outcome| outcome.skill)
                .collect::<BTreeSet<_>>();
        if changed.is_empty() {
            return;
        }
        approvals.approvals.retain(|approval| {
            if approval.scope != "skill" {
                return true;
            }
            let Some((approved_source, approved_skill)) = approval.value.split_once(':') else {
                return true;
            };
            if approved_source != source_id {
                return true;
            }
            !previous.inventory.iter().any(|entry| {
                let identity = entry.id.as_deref().unwrap_or(&entry.slot_name);
                changed.contains(identity)
                    && (approved_skill == identity
                        || approved_skill == entry.slot_name
                        || approved_skill == entry.path)
            })
        });
    }

    fn persist_source_update(
        &self,
        paths: &store::StorePaths,
        original: ProjectPersistedSnapshot<'_>,
        next: ProjectPersistedSnapshot<'_>,
    ) -> DaloResult<()> {
        let journal_path = paths.root.join("project-update.toml");
        check_path(&self.root, Path::new(".dalo/project-update.toml"))?;
        if journal_path.exists() {
            return Err(invalid(
                "an interrupted project update needs recovery before another update",
            ));
        }
        let journal = ProjectUpdateJournal {
            schema_version: PROJECT_UPDATE_JOURNAL_SCHEMA_VERSION,
            original_config: original.config.clone(),
            original_source_lock: original.source_lock.clone(),
            original_approvals: original.approvals.clone(),
        };
        let mut temporary = NamedTempFile::new_in(&paths.root)?;
        temporary.write_all(toml::to_string(&journal)?.as_bytes())?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&journal_path)
            .map_err(|error| error.error)?;
        fs::File::open(&paths.root)?.sync_all()?;

        let update = (|| -> DaloResult<()> {
            catalog::write_source_lock(paths, next.source_lock)?;
            store::write_config(paths, next.config)?;
            if next.approvals != original.approvals {
                store::write_approvals(paths, next.approvals)?;
            }
            Ok(())
        })();
        if let Err(error) = update {
            let rollback = store::write_config(paths, original.config)
                .and_then(|()| catalog::write_source_lock(paths, original.source_lock))
                .and_then(|()| store::write_approvals(paths, original.approvals));
            if let Err(rollback_error) = rollback {
                return Err(DaloError::Io(std::io::Error::other(format!(
                    "{error}; additionally failed to restore project source state: {rollback_error}"
                ))));
            }
            let _ = fs::remove_file(&journal_path);
            return Err(error);
        }
        fs::remove_file(&journal_path)?;
        fs::File::open(&paths.root)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_unsupported_and_unsafe_declarations() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("project");
        fs::create_dir(&root).expect("project directory");
        let project = Project::new(&root).expect("project");

        let cases = [
            (
                "schema_version = 2\ntargets = [\"claude\"]\n",
                "unsupported project schema_version",
            ),
            (
                "schema_version = 1\ntargets = []\n",
                "targets must not be empty",
            ),
            (
                "schema_version = 1\ntargets = [\"codex\", \"openclaw\"]\n",
                "targets must have distinct folders",
            ),
            (
                "schema_version = 1\ntargets = [\"unknown\"]\n",
                "unsupported project target",
            ),
            (
                "schema_version = 1\ntargets = [\"claude\"]\n\n[[source]]\nid = \"local\"\nurl = \"https://example.com/skills.git\"\ncommit = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nskills = [\"review\"]\n",
                "invalid or duplicate project source",
            ),
            (
                "schema_version = 1\ntargets = [\"claude\"]\n\n[[source]]\nid = \"shared\"\nurl = \"https://example.com/skills.git\"\ncommit = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\nskills = [\"review\"]\n",
                "requires a full lowercase commit ID",
            ),
            (
                "schema_version = 1\ntargets = [\"claude\"]\n\n[[source]]\nid = \"shared\"\nurl = \"https://example.com/skills.git\"\ncommit = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nskills = []\n",
                "needs explicit skill selectors",
            ),
        ];

        for (manifest, expected) in cases {
            fs::write(root.join(MANIFEST), manifest).expect("write invalid manifest");
            let error = project
                .manifest()
                .expect_err("invalid manifest should be rejected")
                .to_string();
            assert!(
                error.contains(expected),
                "{error:?} should contain {expected:?}"
            );
        }

        fs::write(root.join(MANIFEST), vec![b' '; 1024 * 1024 + 1])
            .expect("write oversized manifest");
        let error = project
            .manifest()
            .expect_err("oversized manifest should be rejected")
            .to_string();
        assert!(error.contains("exceeds 1 MiB"));
    }

    #[test]
    fn interrupted_update_recovery_restores_config_source_lock_and_approvals() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("project");
        fs::create_dir(&root).expect("project directory");
        let project = Project::new(&root).expect("project");
        store::init_store(project.store.clone(), false).expect("initialize project store");
        fs::write(project.store.join("project-owner"), "dalo-project-v1\n")
            .expect("mark project store");
        let paths = store::StorePaths::new(project.store.clone());
        let original_config = store::read_config(&paths).expect("original config");
        let original_source_lock = catalog::read_source_lock(&paths).expect("original source lock");
        let original_approvals = store::read_approvals(&paths).expect("original approvals");

        let journal = ProjectUpdateJournal {
            schema_version: PROJECT_UPDATE_JOURNAL_SCHEMA_VERSION,
            original_config: original_config.clone(),
            original_source_lock: original_source_lock.clone(),
            original_approvals: original_approvals.clone(),
        };
        fs::write(
            paths.root.join("project-update.toml"),
            toml::to_string(&journal).expect("serialize recovery record"),
        )
        .expect("write recovery record");

        let mut interrupted_config = original_config.clone();
        interrupted_config.sources[0].trusted = true;
        store::write_config(&paths, &interrupted_config).expect("write interrupted config");
        let mut interrupted_source_lock = original_source_lock.clone();
        interrupted_source_lock.schema_version += 1;
        catalog::write_source_lock(&paths, &interrupted_source_lock)
            .expect("write interrupted source lock");
        let mut interrupted_approvals = original_approvals.clone();
        interrupted_approvals
            .approvals
            .push(store::ApprovalRecord::granted(
                "source".into(),
                "stale".into(),
            ));
        store::write_approvals(&paths, &interrupted_approvals)
            .expect("write interrupted approvals");

        project
            .recover_interrupted_update()
            .expect("recover project update");

        assert_eq!(store::read_config(&paths).unwrap(), original_config);
        assert_eq!(
            catalog::read_source_lock(&paths).unwrap(),
            original_source_lock
        );
        assert_eq!(store::read_approvals(&paths).unwrap(), original_approvals);
        assert!(!paths.root.join("project-update.toml").exists());
    }
}
