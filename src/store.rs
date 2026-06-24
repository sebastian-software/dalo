//! Store path resolution and managed state layout.

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
use crate::error::{SkillmgrError, SkillmgrResult};
use crate::git;
use crate::lockfile::{USER_LOCK_SCHEMA_VERSION, UserLock};

/// Environment variable used to override the default store path.
pub const STORE_ENV_VAR: &str = "SKILLMGR_STORE";

/// Default store directory name below the user's home directory.
pub const DEFAULT_STORE_DIR: &str = ".skillmgr";

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
    /// Log directory.
    pub logs_dir: PathBuf,
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
            logs_dir: root.join("logs"),
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

impl StateFile {
    /// Empty state for a new store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            targets: Vec::new(),
            materialization_dirs: Vec::new(),
            owned_skills: Vec::new(),
        }
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
pub fn resolve_store_path(explicit: Option<&Path>) -> SkillmgrResult<PathBuf> {
    let candidate = if let Some(path) = explicit {
        path.to_path_buf()
    } else if let Some(path) = env::var_os(STORE_ENV_VAR) {
        PathBuf::from(path)
    } else {
        home_dir()?.join(DEFAULT_STORE_DIR)
    };

    expand_user_path(&candidate)
}

/// Initialize the skillmgr store.
pub fn init_store(store_root: PathBuf, dry_run: bool) -> SkillmgrResult<InitReport> {
    validate_store_path(&store_root)?;

    let paths = StorePaths::new(store_root);
    let mut operations = Vec::new();

    for directory in [
        &paths.root,
        &paths.local_dir,
        &paths.local_skills_dir,
        &paths.local_instructions_dir,
        &paths.sources_dir,
        &paths.logs_dir,
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
pub fn read_state(paths: &StorePaths) -> SkillmgrResult<StateFile> {
    if !paths.state_file.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.state_file)?;
    Ok(toml::from_str(&content)?)
}

/// Read the initialized user config.
pub fn read_config(paths: &StorePaths) -> SkillmgrResult<UserConfig> {
    if !paths.config_file.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.config_file)?;
    Ok(toml::from_str(&content)?)
}

/// Read the resolved user lock and validate its schema version.
pub fn read_user_lock(paths: &StorePaths) -> SkillmgrResult<UserLock> {
    if !paths.lock_file.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.lock_file)?;
    let lock: UserLock = toml::from_str(&content)?;
    if lock.schema_version != USER_LOCK_SCHEMA_VERSION {
        return Err(SkillmgrError::UnsupportedLockSchema {
            path: paths.lock_file.clone(),
            version: lock.schema_version,
            supported: USER_LOCK_SCHEMA_VERSION,
        });
    }

    Ok(lock)
}

/// Write the user config atomically.
pub fn write_config(paths: &StorePaths, config: &UserConfig) -> SkillmgrResult<()> {
    if !paths.root.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    write_toml_atomic(&paths.config_file, config)
}

/// Write the resolved user lock atomically.
pub fn write_user_lock(paths: &StorePaths, lock: &UserLock) -> SkillmgrResult<()> {
    if !paths.root.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    write_toml_atomic(&paths.lock_file, lock)
}

/// Read local approvals.
pub fn read_approvals(paths: &StorePaths) -> SkillmgrResult<ApprovalsFile> {
    if !paths.approvals_file.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.approvals_file)?;
    Ok(toml::from_str(&content)?)
}

/// Write the store state file atomically.
pub fn write_state(paths: &StorePaths, state: &StateFile) -> SkillmgrResult<()> {
    if !paths.root.exists() {
        return Err(SkillmgrError::StoreNotInitialized {
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
    pub fn acquire(paths: &StorePaths) -> SkillmgrResult<Self> {
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

    fn acquire_with_delays(paths: &StorePaths, delays: &[Duration]) -> SkillmgrResult<Self> {
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
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }

        Err(SkillmgrError::StoreLocked {
            path: paths.lock_guard_file.clone(),
        })
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

fn ensure_dir(path: &Path, dry_run: bool) -> SkillmgrResult<InitOperation> {
    let status = if path.exists() {
        if !path.is_dir() {
            return Err(SkillmgrError::InvalidStorePath {
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

fn ensure_toml_file<T>(path: &Path, value: &T, dry_run: bool) -> SkillmgrResult<InitOperation>
where
    T: Serialize,
{
    let status = if path.exists() {
        if !path.is_file() {
            return Err(SkillmgrError::InvalidStorePath {
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

fn ensure_git_repo(path: &Path, dry_run: bool) -> SkillmgrResult<InitOperation> {
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

fn write_toml_atomic<T>(path: &Path, value: &T) -> SkillmgrResult<()>
where
    T: Serialize,
{
    let Some(parent) = path.parent() else {
        return Err(SkillmgrError::InvalidStorePath {
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

fn validate_store_path(path: &Path) -> SkillmgrResult<()> {
    if path.as_os_str().is_empty() {
        return Err(SkillmgrError::InvalidStorePath {
            path: path.to_path_buf(),
            reason: "path is empty".to_owned(),
        });
    }

    Ok(())
}

/// Expand a leading `~` in a user-provided path.
pub fn expand_user_path(path: &Path) -> SkillmgrResult<PathBuf> {
    let path_string = path.to_string_lossy();

    if path_string == "~" {
        return home_dir();
    }

    if let Some(rest) = path_string.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }

    Ok(path.to_path_buf())
}

fn home_dir() -> SkillmgrResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| SkillmgrError::StorePath {
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
        assert!(store_root.join("logs").is_dir());
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
    fn store_lock_should_fail_when_already_held() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let _lock = StoreLock::acquire(&paths).expect("first lock should be acquired");

        let error = StoreLock::acquire_with_delays(&paths, &[Duration::from_millis(0)])
            .expect_err("second lock should fail");

        assert!(matches!(error, SkillmgrError::StoreLocked { .. }));
    }
}
