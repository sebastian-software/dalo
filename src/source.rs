//! Source definitions and source operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::catalog::{self, SourceLock};
use crate::config::UserConfig;
use crate::error::{DaloError, DaloResult};
use crate::git;
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
    /// Whether the checkout was retained at the user's request.
    pub kept_checkout: bool,
    /// Number of source-scoped approval records removed.
    pub removed_approvals: usize,
    /// Whether a catalog lock entry was removed.
    pub removed_catalog_lock: bool,
    /// Owned target links reconciled during removal.
    pub reconciled_links: Vec<PathBuf>,
    /// Durable store artifacts that the removal updates or cleans up.
    pub affected_paths: Vec<PathBuf>,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
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
        .ok_or_else(|| DaloError::UnknownSource {
            source_id: id.to_owned(),
        })?;
    if source.kind == SourceKind::Local {
        return Err(DaloError::InvalidSourceId {
            id: id.to_owned(),
            reason: "the built-in local source cannot be removed".to_owned(),
        });
    }
    let original_source_lock = catalog::read_source_lock(paths)?;
    let original_approvals = store::read_approvals(paths)?;
    let mut config = original_config.clone();
    config.sources.retain(|candidate| candidate.id != id);
    sort_sources(&mut config.sources);
    let mut source_lock = original_source_lock.clone();
    let before_catalogs = source_lock.catalogs.len();
    source_lock.catalogs.retain(|entry| entry.source_id != id);
    let removed_catalog_lock = source_lock.catalogs.len() != before_catalogs;
    let mut approvals = original_approvals.clone();
    let source_prefix = format!("{id}:");
    let before_approvals = approvals.approvals.len();
    approvals.approvals.retain(|approval| {
        !(approval.value.starts_with(&source_prefix)
            || approval.scope == "source" && approval.value == id)
    });
    let removed_approvals = before_approvals - approvals.approvals.len();

    let mut affected_paths = vec![
        paths.config_file.clone(),
        paths.source_lock_file.clone(),
        paths.approvals_file.clone(),
        paths.lock_file.clone(),
        paths.state_file.clone(),
    ];
    if !keep_checkout {
        affected_paths.push(source.path.clone());
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
            kept_checkout: keep_checkout,
            removed_approvals,
            removed_catalog_lock,
            reconciled_links: Vec::new(),
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
        selection: Vec::new(),
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
    clone_repo(url, &checkout).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&checkout);
    })?;

    // From here on the checkout exists on disk. If persisting the source fails,
    // remove the clone so a later `source add` does not trip over an orphaned
    // checkout that is absent from config (InvalidStorePath).
    finish_team_source(paths, &mut config, source.clone(), write_config).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&checkout);
    })?;

    Ok(SourceAddReport {
        source,
        dry_run: false,
    })
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
        sort_sources(&mut config.sources);
        store::write_config(paths, &config)?;
    }

    Ok(SourcePriorityReport { source, dry_run })
}

/// Refresh clean tracking team sources before sync.
///
/// Missing or unknown update policies are treated as pinned and are not pulled.
pub fn refresh_tracking_team_sources(paths: &StorePaths) -> DaloResult<()> {
    let config = store::read_config(paths)?;
    refresh_tracking_team_sources_from_config(&config)
}

/// Refresh clean tracking team sources from an already-read config.
///
/// Missing or unknown update policies are treated as pinned and are not pulled.
pub fn refresh_tracking_team_sources_from_config(config: &UserConfig) -> DaloResult<()> {
    for source in config.sources.iter().filter(|source| {
        source.enabled
            && source.kind == SourceKind::Team
            && source.update_policy.as_deref() == Some("track")
    }) {
        if git::is_dirty(&source.path)? {
            return Err(DaloError::DirtySource {
                source_id: source.id.clone(),
            });
        }
        git::pull_ff_only(&source.path)?;
    }

    Ok(())
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
