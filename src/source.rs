//! Source definitions and source operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::UserConfig;
use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::store::{self, StorePaths};

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
    // Validate the id before anything touches the store: it is joined straight
    // into the checkout path and `git clone`d there, so an id like `../../evil`
    // or `a/b` would escape `sources/` to an attacker-chosen location.
    if !is_valid_source_id(id) {
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

    // From here on the checkout exists on disk. If persisting the source fails,
    // remove the clone so a later `source add` does not trip over an orphaned
    // checkout that is absent from config (InvalidStorePath).
    finish_team_source(paths, &mut config, source.clone()).inspect_err(|_| {
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
/// records and resolver output, so the same conservative rules as slot names
/// apply: non-empty, never the `.`/`..` traversal segments, and limited to a
/// `[A-Za-z0-9._-]` token (no `/` path separators).
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
) -> DaloResult<()> {
    config.sources.push(source);
    config.sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    store::write_config(paths, config)?;
    Ok(())
}

/// List configured sources.
#[must_use = "the source list report should be rendered or inspected"]
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::Command;

    use super::*;

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
    fn add_team_source_should_remove_checkout_when_persisting_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let repo = temp_dir.path().join("team-repo");
        create_git_repo(&repo);
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        // Force the config persist step to fail after the clone: cloning writes
        // under `sources/`, but atomic config writes need a temp file in the
        // store root, which is made read-only for this check.
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o555))
            .expect("store root should be made read-only");
        let checkout = paths.sources_dir.join("company").join("checkout");

        let error = add_team_source(&paths, "company", &repo.to_string_lossy(), false)
            .expect_err("add should fail when config cannot be recorded");

        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o755))
            .expect("store root permissions should be restored");

        assert!(matches!(error, DaloError::Io(_)));
        // The orphaned checkout must be gone so a later `source add` is not blocked
        // by a stale checkout, and config must not reference the half-added source.
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
        run_git(repo, &["commit", "-q", "-m", "initial"]);
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
