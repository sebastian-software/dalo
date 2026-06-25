//! Store path resolution and managed state layout.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::config::UserConfig;
use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::lockfile::{USER_LOCK_SCHEMA_VERSION, UserLock};

/// Environment variable used to override the default store path.
pub const STORE_ENV_VAR: &str = "DALO_STORE";

/// Default store directory name below the user's home directory.
pub const DEFAULT_STORE_DIR: &str = ".dalo";

/// Current internal state schema version.
pub const STATE_SCHEMA_VERSION: u32 = 1;

/// Current approval schema version.
pub const APPROVALS_SCHEMA_VERSION: u32 = 1;

/// Fully resolved store path layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePaths {
    /// Store root.
    pub root: PathBuf,
    /// User config path.
    pub config_file: PathBuf,
    /// Resolved user lock path.
    pub lock_file: PathBuf,
    /// Internal state path.
    pub state_file: PathBuf,
    /// Local approval state path.
    pub approvals_file: PathBuf,
    /// Coarse store lock path.
    pub lock_guard_file: PathBuf,
    /// Local source root.
    pub local_dir: PathBuf,
    /// Local skills directory.
    pub local_skills_dir: PathBuf,
    /// Local instruction pack directory.
    pub local_instructions_dir: PathBuf,
    /// Source checkout root.
    pub sources_dir: PathBuf,
    /// Catalog source lock path (pinned commits, selections, inventory snapshot).
    pub source_lock_file: PathBuf,
}

impl StorePaths {
    /// Build the path layout from a store root.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        let local_dir = root.join("local");

        Self {
            config_file: root.join("config.toml"),
            lock_file: root.join("lock.toml"),
            state_file: root.join("state.toml"),
            approvals_file: root.join("approvals.toml"),
            lock_guard_file: root.join(".lock"),
            local_skills_dir: local_dir.join("skills"),
            local_instructions_dir: local_dir.join("instructions"),
            sources_dir: root.join("sources"),
            source_lock_file: root.join("source-lock.toml"),
            local_dir,
            root,
        }
    }
}

/// Internal materialization state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateFile {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Configured logical targets.
    pub targets: Vec<TargetState>,
    /// Canonical materialization directories derived from logical targets.
    #[serde(default)]
    pub materialization_dirs: Vec<MaterializationDirState>,
    /// Recorded owned skill symlinks.
    pub owned_skills: Vec<OwnedSkillState>,
    /// Explicitly protected unmanaged skills.
    #[serde(default)]
    pub protected_skills: Vec<ProtectedSkillState>,
}

/// Configured target state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetState {
    /// Logical target ID.
    pub id: String,
    /// Target directory.
    pub path: PathBuf,
    /// Canonical target directory used for de-duplication.
    pub canonical_path: PathBuf,
    /// Whether the target is enabled.
    pub enabled: bool,
}

/// One physical materialization directory shared by one or more logical targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializationDirState {
    /// Canonical physical directory.
    pub path: PathBuf,
    /// Logical target IDs that use this directory.
    pub logical_targets: Vec<String>,
}

/// Recorded owned skill symlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedSkillState {
    /// Target ID.
    pub target_id: String,
    /// Skill slot name.
    pub slot_name: String,
    /// Symlink path.
    pub link_path: PathBuf,
    /// Store path the symlink should point to.
    pub store_path: PathBuf,
}

/// Explicitly protected unmanaged skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedSkillState {
    /// Skill slot name.
    pub slot_name: String,
    /// Protected target path.
    pub path: PathBuf,
}

impl StateFile {
    /// Empty state for a new store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            targets: Vec::new(),
            materialization_dirs: Vec::new(),
            owned_skills: Vec::new(),
            protected_skills: Vec::new(),
        }
    }

    /// Recompute the canonical materialization directories from the enabled
    /// logical targets. `logical_targets` is sorted so a directory shared by
    /// several targets gets a deterministic representative.
    pub fn rebuild_materialization_dirs(&mut self) {
        let mut grouped: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        for target in self.targets.iter().filter(|target| target.enabled) {
            grouped
                .entry(target.canonical_path.clone())
                .or_default()
                .push(target.id.clone());
        }
        self.materialization_dirs = grouped
            .into_iter()
            .map(|(path, mut logical_targets)| {
                logical_targets.sort();
                MaterializationDirState {
                    path,
                    logical_targets,
                }
            })
            .collect();
    }
}

/// Local approval state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalsFile {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Approval records.
    pub approvals: Vec<ApprovalRecord>,
}

/// One local approval record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    /// Approval scope, such as `skill`, `source`, `author`, or `org`.
    pub scope: String,
    /// Approved identifier.
    pub value: String,
}

impl ApprovalsFile {
    /// Empty approvals for a new store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: APPROVALS_SCHEMA_VERSION,
            approvals: Vec::new(),
        }
    }
}

/// Init operation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitOperationStatus {
    /// Operation would run in dry-run mode.
    Planned,
    /// Operation created or initialized something.
    Created,
    /// Requested state already existed.
    Existing,
}

impl InitOperationStatus {
    /// Status label for text output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Created => "created",
            Self::Existing => "existing",
        }
    }
}

/// Init operation action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitOperationAction {
    /// Directory creation.
    CreateDir,
    /// TOML file write.
    WriteFile,
    /// Git repository initialization.
    GitInit,
}

impl InitOperationAction {
    /// Action label for text output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateDir => "create_dir",
            Self::WriteFile => "write_file",
            Self::GitInit => "git_init",
        }
    }
}

/// One init operation report entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitOperation {
    /// Operation action.
    pub action: InitOperationAction,
    /// Operation target path.
    pub path: PathBuf,
    /// Operation status.
    pub status: InitOperationStatus,
}

/// Init command report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitReport {
    /// Store root.
    pub store: PathBuf,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
    /// Operation results.
    pub operations: Vec<InitOperation>,
}

/// Resolve the store path from CLI input, environment, or default.
pub fn resolve_store_path(explicit: Option<&Path>) -> DaloResult<PathBuf> {
    let candidate = if let Some(path) = explicit {
        path.to_path_buf()
    } else if let Some(path) = env::var_os(STORE_ENV_VAR) {
        PathBuf::from(path)
    } else {
        home_dir()?.join(DEFAULT_STORE_DIR)
    };

    expand_user_path(&candidate)
}

/// Initialize the dalo store.
pub fn init_store(store_root: PathBuf, dry_run: bool) -> DaloResult<InitReport> {
    validate_store_path(&store_root)?;

    let paths = StorePaths::new(store_root);
    let mut operations = Vec::new();

    for directory in [
        &paths.root,
        &paths.local_dir,
        &paths.local_skills_dir,
        &paths.local_instructions_dir,
        &paths.sources_dir,
    ] {
        operations.push(ensure_dir(directory, dry_run)?);
    }

    operations.push(ensure_toml_file(
        &paths.config_file,
        &UserConfig::default_for_store(&paths.root),
        dry_run,
    )?);
    operations.push(ensure_toml_file(
        &paths.lock_file,
        &UserLock::empty(),
        dry_run,
    )?);
    operations.push(ensure_toml_file(
        &paths.state_file,
        &StateFile::empty(),
        dry_run,
    )?);
    operations.push(ensure_toml_file(
        &paths.approvals_file,
        &ApprovalsFile::empty(),
        dry_run,
    )?);
    operations.push(ensure_git_repo(&paths.local_dir, dry_run)?);

    Ok(InitReport {
        store: paths.root,
        dry_run,
        operations,
    })
}

/// Read the initialized store state file.
pub fn read_state(paths: &StorePaths) -> DaloResult<StateFile> {
    if !paths.state_file.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.state_file)?;
    let mut state: StateFile = parse_store_toml(&paths.state_file, &content)?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(DaloError::UnsupportedSchema {
            path: paths.state_file.clone(),
            version: state.schema_version,
            supported: STATE_SCHEMA_VERSION,
        });
    }
    // Lazy migration: `materialization_dirs` was added after the initial schema
    // without a version bump. A state written before it exists has no block, which
    // would make `sync` treat every owned skill as orphaned and remove its symlink.
    // Reconstruct it from the targets when it is missing but targets are present.
    if state.materialization_dirs.is_empty() && !state.targets.is_empty() {
        state.rebuild_materialization_dirs();
    }

    Ok(state)
}

/// Read the initialized user config.
pub fn read_config(paths: &StorePaths) -> DaloResult<UserConfig> {
    if !paths.config_file.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.config_file)?;
    let config: UserConfig = parse_store_toml(&paths.config_file, &content)?;
    ensure_unique_source_ids(&paths.config_file, &config)?;
    Ok(config)
}

/// Reject a hand-edited config that declares the same source id twice.
///
/// The resolver keys sources by id in a map (last write wins), so a duplicate
/// would silently drop a source while `list`/`status` still show both. Fail
/// loudly instead with the offending id.
fn ensure_unique_source_ids(path: &Path, config: &UserConfig) -> DaloResult<()> {
    let mut seen = std::collections::BTreeSet::new();
    for source in &config.sources {
        if !seen.insert(source.id.as_str()) {
            return Err(DaloError::FileParse {
                path: path.to_path_buf(),
                reason: format!("duplicate source id `{}`", source.id),
            });
        }
    }

    Ok(())
}

/// Read the resolved user lock and validate its schema version.
pub fn read_user_lock(paths: &StorePaths) -> DaloResult<UserLock> {
    if !paths.lock_file.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.lock_file)?;
    let lock: UserLock = parse_store_toml(&paths.lock_file, &content)?;
    if lock.schema_version != USER_LOCK_SCHEMA_VERSION {
        return Err(DaloError::UnsupportedLockSchema {
            path: paths.lock_file.clone(),
            version: lock.schema_version,
            supported: USER_LOCK_SCHEMA_VERSION,
        });
    }

    Ok(lock)
}

/// Write the user config atomically.
pub fn write_config(paths: &StorePaths, config: &UserConfig) -> DaloResult<()> {
    if !paths.root.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    write_toml_atomic(&paths.config_file, config)
}

/// Write the resolved user lock atomically.
pub fn write_user_lock(paths: &StorePaths, lock: &UserLock) -> DaloResult<()> {
    if !paths.root.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    write_toml_atomic(&paths.lock_file, lock)
}

/// Read local approvals.
pub fn read_approvals(paths: &StorePaths) -> DaloResult<ApprovalsFile> {
    if !paths.approvals_file.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.approvals_file)?;
    let approvals: ApprovalsFile = parse_store_toml(&paths.approvals_file, &content)?;
    if approvals.schema_version != APPROVALS_SCHEMA_VERSION {
        return Err(DaloError::UnsupportedSchema {
            path: paths.approvals_file.clone(),
            version: approvals.schema_version,
            supported: APPROVALS_SCHEMA_VERSION,
        });
    }

    Ok(approvals)
}

/// Parse store TOML, attaching the file path to any parser error.
fn parse_store_toml<T: serde::de::DeserializeOwned>(path: &Path, content: &str) -> DaloResult<T> {
    toml::from_str(content).map_err(|error| DaloError::FileParse {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

/// Write local approvals atomically.
pub fn write_approvals(paths: &StorePaths, approvals: &ApprovalsFile) -> DaloResult<()> {
    if !paths.root.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    write_toml_atomic(&paths.approvals_file, approvals)
}

/// Write the store state file atomically.
pub fn write_state(paths: &StorePaths, state: &StateFile) -> DaloResult<()> {
    if !paths.root.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    write_toml_atomic(&paths.state_file, state)
}

/// Exclusive store lock guard for mutating commands.
#[derive(Debug)]
pub struct StoreLock {
    path: PathBuf,
}

impl StoreLock {
    /// Acquire the store lock with a short interactive retry window.
    pub fn acquire(paths: &StorePaths) -> DaloResult<Self> {
        let delays = [
            Duration::from_millis(0),
            Duration::from_millis(100),
            Duration::from_millis(250),
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ];

        Self::acquire_with_delays(paths, &delays)
    }

    fn acquire_with_delays(paths: &StorePaths, delays: &[Duration]) -> DaloResult<Self> {
        for delay in delays {
            if !delay.is_zero() {
                thread::sleep(*delay);
            }

            match try_create_lock(&paths.lock_guard_file) {
                Ok(()) => {
                    return Ok(Self {
                        path: paths.lock_guard_file.clone(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // The guard file exists. If its owner has died, the lock is
                    // stale: drop it and immediately retry so a crashed process
                    // never blocks the store permanently. A live owner (or an
                    // unreadable/owner-unknown lock) keeps blocking through the
                    // remaining retry window.
                    if reclaim_if_stale(&paths.lock_guard_file)?
                        && try_create_lock(&paths.lock_guard_file).is_ok()
                    {
                        return Ok(Self {
                            path: paths.lock_guard_file.clone(),
                        });
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }

        Err(DaloError::StoreLocked {
            path: paths.lock_guard_file.clone(),
        })
    }
}

/// Remove the guard file when its recorded owner is provably dead.
///
/// Returns `true` when a stale lock was removed (so the caller should retry to
/// claim it), `false` when the lock should be treated as live and kept. A lock
/// with no readable `pid=` line, or whose owner is still alive, is preserved.
fn reclaim_if_stale(path: &Path) -> DaloResult<bool> {
    let Some(pid) = read_lock_pid(path) else {
        // No readable owner: be conservative and keep blocking. This also covers
        // the race where another process just removed the file before we read it.
        return Ok(false);
    };

    if process_is_alive(pid) {
        return Ok(false);
    }

    // The owner is gone. Best-effort removal; a concurrent reclaimer winning the
    // race (NotFound) is fine and still lets us retry the create below.
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}

/// Read the `pid=<n>` owner recorded by [`try_create_lock`].
fn read_lock_pid(path: &Path) -> Option<u32> {
    let content = fs::read_to_string(path).ok()?;
    content
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid="))
        .and_then(|value| value.trim().parse::<u32>().ok())
}

/// Probe whether a process is still alive.
///
/// Platform assumption: Unix. dalo ships for Unix-like systems, so liveness is
/// probed with `kill -0 <pid>`, which delivers no signal but reports whether the
/// process exists: success means alive, a non-zero status means it is gone. We
/// shell out to `kill` rather than calling `libc::kill` so the crate keeps
/// `unsafe-code = forbid` and takes on no extra dependency. The store is
/// single-user, so the owner is always the current user and `kill -0` never hits
/// the cross-user permission case. If `kill` itself cannot be spawned we err on
/// the side of caution and treat the owner as alive, so a live lock is never
/// reclaimed by mistake.
fn process_is_alive(pid: u32) -> bool {
    use std::process::Command;

    match Command::new("kill").args(["-0", &pid.to_string()]).status() {
        Ok(status) => status.success(),
        Err(_) => true,
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn try_create_lock(path: &Path) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file, "pid={}", std::process::id())?;
    Ok(())
}

fn ensure_dir(path: &Path, dry_run: bool) -> DaloResult<InitOperation> {
    let status = if path.exists() {
        if !path.is_dir() {
            return Err(DaloError::InvalidStorePath {
                path: path.to_path_buf(),
                reason: "expected a directory".to_owned(),
            });
        }
        InitOperationStatus::Existing
    } else if dry_run {
        InitOperationStatus::Planned
    } else {
        fs::create_dir_all(path)?;
        InitOperationStatus::Created
    };

    Ok(InitOperation {
        action: InitOperationAction::CreateDir,
        path: path.to_path_buf(),
        status,
    })
}

fn ensure_toml_file<T>(path: &Path, value: &T, dry_run: bool) -> DaloResult<InitOperation>
where
    T: Serialize,
{
    let status = if path.exists() {
        if !path.is_file() {
            return Err(DaloError::InvalidStorePath {
                path: path.to_path_buf(),
                reason: "expected a file".to_owned(),
            });
        }
        InitOperationStatus::Existing
    } else if dry_run {
        InitOperationStatus::Planned
    } else {
        write_toml_atomic(path, value)?;
        InitOperationStatus::Created
    };

    Ok(InitOperation {
        action: InitOperationAction::WriteFile,
        path: path.to_path_buf(),
        status,
    })
}

fn ensure_git_repo(path: &Path, dry_run: bool) -> DaloResult<InitOperation> {
    let git_dir = path.join(".git");
    let status = if git_dir.exists() {
        InitOperationStatus::Existing
    } else if dry_run {
        InitOperationStatus::Planned
    } else {
        git::init_repo(path)?;
        InitOperationStatus::Created
    };

    Ok(InitOperation {
        action: InitOperationAction::GitInit,
        path: path.to_path_buf(),
        status,
    })
}

pub(crate) fn write_toml_atomic<T>(path: &Path, value: &T) -> DaloResult<()>
where
    T: Serialize,
{
    let Some(parent) = path.parent() else {
        return Err(DaloError::InvalidStorePath {
            path: path.to_path_buf(),
            reason: "file has no parent directory".to_owned(),
        });
    };

    let content = toml::to_string_pretty(value)?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;
    temp_file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn validate_store_path(path: &Path) -> DaloResult<()> {
    if path.as_os_str().is_empty() {
        return Err(DaloError::InvalidStorePath {
            path: path.to_path_buf(),
            reason: "path is empty".to_owned(),
        });
    }

    Ok(())
}

/// Expand a leading `~` in a user-provided path.
pub fn expand_user_path(path: &Path) -> DaloResult<PathBuf> {
    let path_string = path.to_string_lossy();

    if path_string == "~" {
        return home_dir();
    }

    if let Some(rest) = path_string.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }

    Ok(path.to_path_buf())
}

fn home_dir() -> DaloResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| DaloError::StorePath {
            reason: "HOME is not set and no explicit store path was provided".to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_store_should_create_expected_layout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");

        let report = init_store(store_root.clone(), false).expect("init should succeed");

        assert_eq!(report.store, store_root);
        assert!(
            report
                .operations
                .iter()
                .all(|operation| { matches!(operation.status, InitOperationStatus::Created) })
        );
        assert!(store_root.join("config.toml").is_file());
        assert!(store_root.join("lock.toml").is_file());
        assert!(store_root.join("state.toml").is_file());
        assert!(store_root.join("approvals.toml").is_file());
        assert!(store_root.join("local/.git").is_dir());
        assert!(store_root.join("local/skills").is_dir());
        assert!(store_root.join("local/instructions").is_dir());
        assert!(store_root.join("sources").is_dir());
    }

    #[test]
    fn init_store_should_not_write_during_dry_run() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");

        let report = init_store(store_root.clone(), true).expect("dry-run should succeed");

        assert!(report.dry_run);
        assert!(
            report
                .operations
                .iter()
                .all(|operation| { matches!(operation.status, InitOperationStatus::Planned) })
        );
        assert!(!store_root.exists());
    }

    #[test]
    fn init_store_should_be_idempotent() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");

        init_store(store_root.clone(), false).expect("first init should succeed");
        let report = init_store(store_root, false).expect("second init should succeed");

        assert!(
            report
                .operations
                .iter()
                .all(|operation| { matches!(operation.status, InitOperationStatus::Existing) })
        );
    }

    #[test]
    fn read_state_should_rebuild_materialization_dirs_when_missing() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let mut state = read_state(&paths).expect("state should be readable");
        // Simulate a state written before `materialization_dirs` existed: a linked
        // target but no materialization dirs.
        state.targets = vec![TargetState {
            id: "generic".to_owned(),
            path: PathBuf::from("/target"),
            canonical_path: PathBuf::from("/target"),
            enabled: true,
        }];
        state.materialization_dirs = Vec::new();
        write_state(&paths, &state).expect("state should be written");

        let loaded = read_state(&paths).expect("state should reload");

        assert_eq!(loaded.materialization_dirs.len(), 1);
    }

    #[test]
    fn read_state_should_fail_when_store_is_not_initialized() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let paths = StorePaths::new(temp_dir.path().join("missing-store"));

        let error = read_state(&paths).expect_err("read should fail on uninitialized store");

        assert!(matches!(error, DaloError::StoreNotInitialized { .. }));
    }

    #[test]
    fn write_state_should_fail_when_store_root_is_absent() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let paths = StorePaths::new(temp_dir.path().join("missing-store"));

        let error = write_state(&paths, &StateFile::empty())
            .expect_err("write should fail when the store root is absent");

        assert!(matches!(error, DaloError::StoreNotInitialized { .. }));
    }

    #[test]
    fn read_config_should_reject_duplicate_source_ids() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        fs::write(
            &paths.config_file,
            "version = 1\n\n[settings]\nautosync = false\n\n\
             [[sources]]\nid = \"company\"\nkind = \"team\"\npath = \"a\"\npriority = 10\nenabled = true\ntrusted = true\n\n\
             [[sources]]\nid = \"company\"\nkind = \"team\"\npath = \"b\"\npriority = 20\nenabled = true\ntrusted = true\n",
        )
        .expect("config should be overwritten");

        let error = read_config(&paths).expect_err("duplicate source ids should be rejected");

        assert!(matches!(error, DaloError::FileParse { .. }));
    }

    #[test]
    fn read_user_lock_should_reject_unsupported_schema_version() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        fs::write(&paths.lock_file, "schema_version = 999\n").expect("lock should be overwritten");

        let error = read_user_lock(&paths).expect_err("read should reject the unsupported schema");

        assert!(matches!(error, DaloError::UnsupportedLockSchema { .. }));
    }

    #[test]
    fn read_state_should_reject_unsupported_schema_version() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        // Write a structurally valid state document with a future schema version
        // so the schema check (not a parse error) is what rejects the file.
        let mut state = StateFile::empty();
        state.schema_version = 999;
        write_state(&paths, &state).expect("state should be overwritten");

        let error = read_state(&paths).expect_err("read should reject the unsupported schema");

        assert!(matches!(error, DaloError::UnsupportedSchema { .. }));
    }

    #[test]
    fn read_approvals_should_reject_unsupported_schema_version() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        // Write a structurally valid approvals document with a future schema
        // version so the schema check is what rejects the file, not parsing.
        let mut approvals = ApprovalsFile::empty();
        approvals.schema_version = 999;
        write_approvals(&paths, &approvals).expect("approvals should be overwritten");

        let error = read_approvals(&paths).expect_err("read should reject the unsupported schema");

        assert!(matches!(error, DaloError::UnsupportedSchema { .. }));
    }

    #[test]
    fn init_store_should_reject_empty_store_path() {
        let error =
            init_store(PathBuf::new(), false).expect_err("init should reject an empty path");

        assert!(matches!(error, DaloError::InvalidStorePath { .. }));
    }

    #[test]
    fn store_lock_should_fail_when_already_held() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let _lock = StoreLock::acquire(&paths).expect("first lock should be acquired");

        let error = StoreLock::acquire_with_delays(&paths, &[Duration::from_millis(0)])
            .expect_err("second lock should fail");

        assert!(matches!(error, DaloError::StoreLocked { .. }));
    }

    #[test]
    fn store_lock_should_release_on_drop() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let lock = StoreLock::acquire(&paths).expect("first lock should be acquired");
        drop(lock);

        let reacquired = StoreLock::acquire_with_delays(&paths, &[Duration::from_millis(0)]);

        assert!(reacquired.is_ok());
    }

    #[test]
    fn store_lock_should_reclaim_stale_lock_when_owner_is_dead() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        // Heuristic: 2147480000 is just under i32::MAX, far above any pid a live
        // system assigns (default Linux/macOS pid_max is 32768/99999), so
        // `kill -0` reliably reports "no such process". A crashed owner leaves a
        // guard file like this; the lock must be reclaimable rather than fatal.
        fs::write(&paths.lock_guard_file, "pid=2147480000\n")
            .expect("stale lock should be written");

        let lock = StoreLock::acquire_with_delays(&paths, &[Duration::from_millis(0)])
            .expect("stale lock with a dead owner should be reclaimed");

        assert_eq!(lock.path, paths.lock_guard_file);
    }

    #[test]
    fn store_lock_should_block_on_stale_lock_when_owner_is_alive() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        // The current test process is provably alive, so a guard file naming it
        // must keep blocking: live contention is never mistaken for a stale lock.
        fs::write(
            &paths.lock_guard_file,
            format!("pid={}\n", std::process::id()),
        )
        .expect("live lock should be written");

        let error = StoreLock::acquire_with_delays(&paths, &[Duration::from_millis(0)])
            .expect_err("a live owner should still block acquisition");

        assert!(matches!(error, DaloError::StoreLocked { .. }));
    }

    #[test]
    fn store_lock_should_grant_to_exactly_one_thread_under_contention() {
        use std::sync::Arc;
        use std::sync::Barrier;

        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = Arc::new(StorePaths::new(store_root));
        // A single zero delay means neither thread retries: each makes exactly one
        // attempt, so exactly one must win and the other must observe `StoreLocked`.
        let delays = Arc::new([Duration::from_millis(0)]);
        let barrier = Arc::new(Barrier::new(2));

        // Both threads must be spawned before either is joined so they actually
        // contend; an explicit Vec keeps the spawn-all-then-join-all ordering and
        // holds every acquired guard alive until the loser makes its single attempt.
        let mut handles = Vec::new();
        for _ in 0..2 {
            let paths = Arc::clone(&paths);
            let delays = Arc::clone(&delays);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                StoreLock::acquire_with_delays(&paths, delays.as_ref())
            }));
        }
        let mut outcomes = Vec::new();
        for handle in handles {
            outcomes.push(handle.join().expect("thread should not panic"));
        }
        let successes = outcomes.iter().filter(|outcome| outcome.is_ok()).count();

        assert_eq!(successes, 1);
    }
}
