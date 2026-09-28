//! Project discovery and isolated stores with portable, commit-pinned declarations.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::catalog::{self, CatalogLock};
use crate::error::{DaloError, DaloResult};
use crate::source::{self, SourceConfig, SourceKind};
use crate::{git, store, target};

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

fn append_manifest_source(
    root: &Path,
    original_bytes: &[u8],
    source: &ProjectSource,
    addition: &str,
) -> DaloResult<()> {
    let manifest_path = root.join(MANIFEST);
    check_path(root, Path::new(MANIFEST))?;
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

    let permissions = fs::metadata(&manifest_path)?.permissions();
    let mut temporary = NamedTempFile::new_in(root)?;
    temporary.as_file().set_permissions(permissions)?;
    temporary.write_all(updated.as_bytes())?;
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
                || configured.path != expected_path
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
            check_path(
                &self.root,
                expected_path
                    .strip_prefix(&self.root)
                    .expect("store below project"),
            )?;
            let lock = catalog::read_source_lock(&paths)?;
            if lock
                .catalogs
                .iter()
                .find(|c| c.source_id == declared.id)
                .map(|c| c.commit.as_str())
                != Some(declared.commit.as_str())
                || git::rev_parse_head(&expected_path)? != declared.commit
                || git::is_dirty(&expected_path)?
            {
                return Err(invalid(format!(
                    "project source `{}` has a changed pin or local edits; install will not overwrite it",
                    declared.id
                )));
            }
        }
        for declared in &manifest.sources {
            if let Some(configured) = config.sources.iter().find(|s| s.id == declared.id)
                && !configured.selection.is_empty()
                && configured.selection
                    != catalog::canonical_skill_selection(&paths, &declared.id, &declared.skills)?
            {
                return Err(invalid(
                    "project selection changed; update reconciliation is not supported yet",
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

    /// Prepare immutable, untrusted catalogs. The caller holds the store lock.
    pub fn prepare(&self, manifest: &Manifest) -> DaloResult<()> {
        self.validate_store(manifest)?;
        let paths = store::StorePaths::new(self.store.clone());
        let mut config = store::read_config(&paths)?;
        for (index, declared) in manifest.sources.iter().enumerate() {
            if !config.sources.iter().any(|s| s.id == declared.id) {
                let url = source::resolve_source_location(&declared.url, &self.root);
                let staging = tempfile::tempdir_in(&paths.root)?;
                let clone = staging.path().join("checkout");
                source::clone_source_checkout(&url, &clone)?;
                let commit = git::resolve_manifest_revision(&clone, &declared.commit)?;
                if commit != declared.commit {
                    return Err(invalid("resolved project commit differs from declaration"));
                }
                git::checkout_detached(&clone, &commit)?;
                let inventory = catalog::catalog_inventory(&clone, &[])?;
                let checkout = paths.sources_dir.join(&declared.id).join("checkout");
                check_path(
                    &self.root,
                    checkout
                        .strip_prefix(&self.root)
                        .expect("store below project"),
                )?;
                if checkout.exists() {
                    return Err(invalid(
                        "unregistered project checkout exists; preserve it before retrying",
                    ));
                }
                fs::create_dir_all(checkout.parent().expect("checkout parent"))?;
                fs::rename(&clone, &checkout)?;
                let mut lock = catalog::read_source_lock(&paths)?;
                lock.catalogs.retain(|c| c.source_id != declared.id);
                lock.catalogs.push(CatalogLock {
                    source_id: declared.id.clone(),
                    commit,
                    selected: vec![],
                    inventory,
                });
                catalog::write_source_lock(&paths, &lock)?;
                config.sources.push(SourceConfig {
                    id: declared.id.clone(),
                    kind: SourceKind::Catalog,
                    path: checkout,
                    subpath: None,
                    priority: i32::try_from(index + 1)
                        .map_err(|_| invalid("too many project sources"))?,
                    namespace: None,
                    enabled: true,
                    trusted: false,
                    url: Some(url),
                    branch: None,
                    update_policy: Some("pin".into()),
                    selection: vec![],
                    declared_by: None,
                    declared_ref: None,
                });
                store::write_config(&paths, &config)?;
            }
            let configured = config
                .sources
                .iter()
                .find(|s| s.id == declared.id)
                .expect("source registered");
            if configured.selection.is_empty() {
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
}
