//! Store path resolution and managed state layout.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::config::{CONFIG_VERSION, UserConfig};
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
///
/// Unknown fields are intentionally tolerated throughout this state model so
/// older binaries can read state written by newer binaries after additive
/// changes. Breaking changes still require a schema-version bump.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Additive fields written by a newer binary, preserved on rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// Configured target state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetState {
    /// Logical target ID.
    pub id: String,
    /// Target directory.
    pub path: PathBuf,
    /// Canonical target directory used for de-duplication.
    pub canonical_path: PathBuf,
    /// Whether the target is enabled.
    pub enabled: bool,
    /// Additive fields written by a newer binary, preserved on rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// One physical materialization directory shared by one or more logical targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterializationDirState {
    /// Canonical physical directory.
    pub path: PathBuf,
    /// Logical target IDs that use this directory.
    pub logical_targets: Vec<String>,
    /// Additive fields written by a newer binary, preserved on rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// Recorded owned skill symlink.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedSkillState {
    /// Target ID.
    pub target_id: String,
    /// Skill slot name.
    pub slot_name: String,
    /// Symlink path.
    pub link_path: PathBuf,
    /// Store path the symlink should point to.
    pub store_path: PathBuf,
    /// Additive fields written by a newer binary, preserved on rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// Explicitly protected unmanaged skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtectedSkillState {
    /// Logical target ID whose slot is protected.
    #[serde(default)]
    pub target_id: String,
    /// Skill slot name.
    pub slot_name: String,
    /// Legacy absolute path, retained only when it cannot be migrated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Additive fields written by a newer binary, preserved on rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
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
            extra: BTreeMap::new(),
        }
    }

    /// Recompute the canonical materialization directories from the enabled
    /// logical targets. `logical_targets` is sorted so a directory shared by
    /// several targets gets a deterministic representative.
    pub fn rebuild_materialization_dirs(&mut self) {
        let previous_extra = std::mem::take(&mut self.materialization_dirs)
            .into_iter()
            .map(|dir| (dir.path, dir.extra))
            .collect::<BTreeMap<_, _>>();
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
                    extra: previous_extra.get(&path).cloned().unwrap_or_default(),
                    path,
                    logical_targets,
                }
            })
            .collect();
    }
}

/// Local approval state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalsFile {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Approval records.
    pub approvals: Vec<ApprovalRecord>,
}

/// One local approval record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Existing state was corrupt and was regenerated.
    Repaired,
}

impl InitOperationStatus {
    /// Status label for text output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Created => "created",
            Self::Existing => "existing",
            Self::Repaired => "repaired",
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
    /// Existing store files that still need manual attention.
    pub validation_warnings: Vec<InitValidationWarning>,
}

/// A persisted store file that remained invalid after initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitValidationWarning {
    /// Invalid store file.
    pub path: PathBuf,
    /// Validation error with parse or schema location details when available.
    pub message: String,
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

    absolute_path(&expand_user_path(&candidate)?)
}

/// Resolve a possibly relative path to an absolute path without requiring the
/// final path to exist.
pub fn absolute_path(path: &Path) -> DaloResult<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(env::current_dir()?.join(path))
}

/// Resolve a symlink target using the same rules the OS uses for links.
#[must_use]
pub fn resolve_link_target(link_path: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        return target.to_path_buf();
    }
    link_path
        .parent()
        .map_or_else(|| target.to_path_buf(), |parent| parent.join(target))
}

/// Normalize a path for identity/prefix comparisons, canonicalizing the longest
/// existing prefix and preserving any missing tail.
#[must_use]
pub fn comparable_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    let mut candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut tail = Vec::<OsString>::new();
    loop {
        if let Ok(canonical) = candidate.canonicalize() {
            let mut comparable = canonical;
            for component in tail.iter().rev() {
                comparable.push(component);
            }
            return comparable;
        }
        let Some(name) = candidate.file_name().map(|name| name.to_os_string()) else {
            return candidate;
        };
        tail.push(name);
        if !candidate.pop() {
            return path.to_path_buf();
        }
    }
}

/// Whether `path` is equal to or below `root`, after comparable normalization.
#[must_use]
pub fn path_is_same_or_descendant(path: &Path, root: &Path) -> bool {
    let path = comparable_path(path);
    let root = comparable_path(root);
    path == root || path.starts_with(root)
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
    operations.push(ensure_state_file(&paths, dry_run)?);
    operations.push(ensure_toml_file(
        &paths.approvals_file,
        &ApprovalsFile::empty(),
        dry_run,
    )?);
    operations.push(ensure_git_repo(&paths.local_dir, dry_run)?);

    let validation_warnings = validate_initialized_store(&paths);

    Ok(InitReport {
        store: paths.root,
        dry_run,
        operations,
        validation_warnings,
    })
}

fn validate_initialized_store(paths: &StorePaths) -> Vec<InitValidationWarning> {
    let checks = [
        (&paths.config_file, read_config(paths).map(|_| ())),
        (&paths.lock_file, read_user_lock(paths).map(|_| ())),
        (&paths.approvals_file, read_approvals(paths).map(|_| ())),
        (&paths.state_file, read_state(paths).map(|_| ())),
    ];

    checks
        .into_iter()
        .filter(|(path, _)| path.exists())
        .filter_map(|(path, result)| {
            result.err().map(|error| InitValidationWarning {
                path: path.clone(),
                message: error.to_string(),
            })
        })
        .collect()
}

/// Read the initialized store state file.
pub fn read_state(paths: &StorePaths) -> DaloResult<StateFile> {
    if !paths.state_file.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }

    let content = fs::read_to_string(&paths.state_file)?;
    let mut state: StateFile =
        toml::from_str(&content).map_err(|error| DaloError::CorruptState {
            path: paths.state_file.clone(),
            reason: error.to_string(),
        })?;
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
    migrate_protected_skills(&mut state);

    Ok(state)
}

fn migrate_protected_skills(state: &mut StateFile) {
    let mut migrated = Vec::new();
    for protected in std::mem::take(&mut state.protected_skills) {
        if !protected.target_id.is_empty() {
            migrated.push(protected);
            continue;
        }
        let Some(path) = protected.path.as_ref() else {
            migrated.push(protected);
            continue;
        };
        let Some(dir) = state
            .materialization_dirs
            .iter()
            .find(|dir| dir.path.join(&protected.slot_name) == *path)
        else {
            migrated.push(protected);
            continue;
        };
        for target_id in &dir.logical_targets {
            migrated.push(ProtectedSkillState {
                target_id: target_id.clone(),
                slot_name: protected.slot_name.clone(),
                path: None,
                extra: protected.extra.clone(),
            });
        }
    }
    migrated.sort_by(|left, right| {
        left.target_id
            .cmp(&right.target_id)
            .then_with(|| left.slot_name.cmp(&right.slot_name))
            .then_with(|| left.path.cmp(&right.path))
    });
    migrated.dedup();
    state.protected_skills = migrated;
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
    if config.version != CONFIG_VERSION {
        return Err(DaloError::UnsupportedSchema {
            path: paths.config_file.clone(),
            version: config.version,
            supported: CONFIG_VERSION,
        });
    }
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
        return Err(DaloError::UnsupportedSchema {
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
    _file: fs::File,
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

            // The file is persistent; the kernel advisory lock on this handle is
            // the ownership signal. The pid text is diagnostic metadata only, so
            // stale pids or missing `kill` binaries cannot block acquisition.
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&paths.lock_guard_file)?;
            match file.try_lock() {
                Ok(()) => {
                    file.set_len(0)?;
                    file.seek(SeekFrom::Start(0))?;
                    writeln!(file, "pid={}", std::process::id())?;
                    file.flush()?;
                    return Ok(Self { _file: file });
                }
                Err(fs::TryLockError::WouldBlock) => {}
                Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }

        Err(DaloError::StoreLocked {
            path: paths.lock_guard_file.clone(),
        })
    }
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

fn ensure_state_file(paths: &StorePaths, dry_run: bool) -> DaloResult<InitOperation> {
    let status = if paths.state_file.exists() {
        if !paths.state_file.is_file() {
            return Err(DaloError::InvalidStorePath {
                path: paths.state_file.clone(),
                reason: "expected a file".to_owned(),
            });
        }
        match read_state(paths) {
            Ok(_) => InitOperationStatus::Existing,
            Err(DaloError::CorruptState { .. }) if dry_run => InitOperationStatus::Planned,
            Err(DaloError::CorruptState { .. }) => {
                backup_corrupt_file(&paths.state_file)?;
                write_toml_atomic(&paths.state_file, &StateFile::empty())?;
                InitOperationStatus::Repaired
            }
            Err(error) => return Err(error),
        }
    } else if dry_run {
        InitOperationStatus::Planned
    } else {
        write_toml_atomic(&paths.state_file, &StateFile::empty())?;
        InitOperationStatus::Created
    };

    Ok(InitOperation {
        action: InitOperationAction::WriteFile,
        path: paths.state_file.clone(),
        status,
    })
}

fn backup_corrupt_file(path: &Path) -> DaloResult<PathBuf> {
    let Some(parent) = path.parent() else {
        return Err(DaloError::InvalidStorePath {
            path: path.to_path_buf(),
            reason: "file has no parent directory".to_owned(),
        });
    };
    let Some(file_name) = path.file_name() else {
        return Err(DaloError::InvalidStorePath {
            path: path.to_path_buf(),
            reason: "file has no file name".to_owned(),
        });
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut backup_name = file_name.to_os_string();
    backup_name.push(format!(
        ".corrupt-{}-{}",
        stamp.as_secs(),
        stamp.subsec_nanos()
    ));
    let backup_path = parent.join(backup_name);
    fs::rename(path, &backup_path)?;
    sync_directory(parent)?;
    Ok(backup_path)
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
    temp_file.as_file().sync_all()?;
    temp_file.persist(path).map_err(|error| error.error)?;
    sync_directory(parent)?;
    Ok(())
}

fn sync_directory(path: &Path) -> DaloResult<()> {
    fs::File::open(path)?.sync_all()?;
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
    if path == Path::new("~") {
        return home_dir();
    }

    if let Ok(rest) = path.strip_prefix("~") {
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
            extra: Default::default(),
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
    fn read_state_should_report_actionable_corrupt_state() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        fs::write(&paths.state_file, "schema_version = ").expect("state should be corrupted");

        let error = read_state(&paths).expect_err("corrupt state should fail");

        assert!(matches!(error, DaloError::CorruptState { .. }));
        assert!(error.to_string().contains("run `dalo init`"));
    }

    #[test]
    fn read_state_should_tolerate_unknown_top_level_fields() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root.clone());
        let mut content =
            fs::read_to_string(&paths.state_file).expect("state should be readable as text");
        content.push_str("\nfuture_additive_field = \"preserved by newer dalo\"\n");
        fs::write(&paths.state_file, &content).expect("future state should be written");

        let state = read_state(&paths).expect("unknown additive field should be tolerated");
        write_state(&paths, &state).expect("unknown additive field should survive a rewrite");
        let rewritten =
            fs::read_to_string(&paths.state_file).expect("rewritten state should be readable");
        let report = init_store(store_root, false).expect("init should not repair future state");

        assert_eq!(state.schema_version, STATE_SCHEMA_VERSION);
        assert!(report.operations.iter().any(|operation| {
            operation.path == paths.state_file && operation.status == InitOperationStatus::Existing
        }));
        assert!(rewritten.contains("future_additive_field = \"preserved by newer dalo\""));
    }

    #[test]
    fn read_state_should_tolerate_unknown_nested_fields() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let mut state = StateFile::empty();
        state.targets.push(TargetState {
            id: "generic".to_owned(),
            path: target.clone(),
            canonical_path: target,
            enabled: true,
            extra: Default::default(),
        });
        state.rebuild_materialization_dirs();
        write_state(&paths, &state).expect("state should be written");
        let content = fs::read_to_string(&paths.state_file)
            .expect("state should be readable")
            .replace(
                "enabled = true",
                "enabled = true\nfuture_target_field = \"newer dalo metadata\"",
            );
        fs::write(&paths.state_file, content).expect("future nested field should be written");

        let parsed = read_state(&paths).expect("unknown nested field should be tolerated");
        write_state(&paths, &parsed).expect("unknown nested field should survive a rewrite");
        let rewritten =
            fs::read_to_string(&paths.state_file).expect("rewritten state should be readable");

        assert_eq!(parsed.targets.len(), 1);
        assert_eq!(parsed.targets[0].id, "generic");
        assert!(rewritten.contains("future_target_field = \"newer dalo metadata\""));
    }

    #[test]
    fn read_state_should_migrate_legacy_protected_path_to_target_slot() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        let mut state = StateFile::empty();
        state.targets.push(TargetState {
            id: "generic".to_owned(),
            path: target.clone(),
            canonical_path: target.clone(),
            enabled: true,
            extra: Default::default(),
        });
        state.rebuild_materialization_dirs();
        state.protected_skills.push(ProtectedSkillState {
            target_id: String::new(),
            slot_name: "review".to_owned(),
            path: Some(target.join("review")),
            extra: Default::default(),
        });
        write_state(&paths, &state).expect("legacy state should be written");

        let migrated = read_state(&paths).expect("legacy protection should migrate");

        assert_eq!(migrated.protected_skills.len(), 1);
        assert_eq!(migrated.protected_skills[0].target_id, "generic");
        assert_eq!(migrated.protected_skills[0].slot_name, "review");
        assert!(migrated.protected_skills[0].path.is_none());
    }

    #[test]
    fn init_store_should_repair_corrupt_state_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root.clone());
        fs::write(&paths.state_file, "schema_version = ").expect("state should be corrupted");

        let report = init_store(store_root.clone(), false).expect("init should repair state");

        assert!(report.operations.iter().any(|operation| {
            operation.path == paths.state_file && operation.status == InitOperationStatus::Repaired
        }));
        assert_eq!(
            read_state(&paths).expect("repaired state should parse"),
            StateFile::empty()
        );
        let backups = fs::read_dir(&store_root)
            .expect("store dir should be readable")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("state.toml.corrupt-")
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(backups[0].path()).expect("backup should be readable"),
            "schema_version = "
        );
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
    fn read_config_should_reject_unknown_fields() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        fs::write(
            &paths.config_file,
            "version = 1\n\n[settings]\nautosync = false\n\n\
             [[sources]]\nid = \"company\"\nkind = \"team\"\npath = \"a\"\npriority = 10\nenabled = true\ntrusted = true\nseleciton = []\n",
        )
        .expect("config should be overwritten");

        let error = read_config(&paths).expect_err("unknown fields should be rejected");

        assert!(matches!(error, DaloError::FileParse { .. }));
        assert!(error.to_string().contains("unknown field"));
        assert!(error.to_string().contains("seleciton"));
    }

    #[test]
    fn read_store_files_should_reject_truncated_toml() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);

        fs::write(&paths.config_file, "version = ").expect("config should be truncated");
        let config_error = read_config(&paths).expect_err("truncated config should fail");
        assert!(matches!(config_error, DaloError::FileParse { .. }));

        fs::write(&paths.lock_file, "schema_version = ").expect("lock should be truncated");
        let lock_error = read_user_lock(&paths).expect_err("truncated lock should fail");
        assert!(matches!(lock_error, DaloError::FileParse { .. }));

        fs::write(&paths.approvals_file, "schema_version = ")
            .expect("approvals should be truncated");
        let approvals_error = read_approvals(&paths).expect_err("truncated approvals should fail");
        assert!(matches!(approvals_error, DaloError::FileParse { .. }));
    }

    #[test]
    fn init_store_should_not_clobber_corrupt_config_lock_or_approvals() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root.clone());
        fs::write(&paths.config_file, "version = ").expect("config should be truncated");
        fs::write(&paths.lock_file, "schema_version = ").expect("lock should be truncated");
        fs::write(&paths.approvals_file, "schema_version = ")
            .expect("approvals should be truncated");

        let report =
            init_store(store_root, false).expect("init should leave non-state TOML files alone");

        assert_eq!(report.validation_warnings.len(), 3);
        assert!(
            report
                .validation_warnings
                .iter()
                .any(|warning| warning.path == paths.config_file)
        );
        assert!(
            report
                .validation_warnings
                .iter()
                .any(|warning| warning.path == paths.lock_file)
        );
        assert!(
            report
                .validation_warnings
                .iter()
                .any(|warning| warning.path == paths.approvals_file)
        );

        assert_eq!(
            fs::read_to_string(&paths.config_file).expect("config should be readable"),
            "version = "
        );
        assert_eq!(
            fs::read_to_string(&paths.lock_file).expect("lock should be readable"),
            "schema_version = "
        );
        assert_eq!(
            fs::read_to_string(&paths.approvals_file).expect("approvals should be readable"),
            "schema_version = "
        );
    }

    #[test]
    fn read_user_lock_should_reject_unsupported_schema_version() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        fs::write(&paths.lock_file, "schema_version = 999\n").expect("lock should be overwritten");

        let error = read_user_lock(&paths).expect_err("read should reject the unsupported schema");

        assert!(matches!(error, DaloError::UnsupportedSchema { .. }));
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
    fn read_config_should_reject_unsupported_schema_version() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let root = temp_dir.path().join("store");
        init_store(root.clone(), false).expect("store should initialize");
        let paths = StorePaths::new(root);
        let mut config = read_config(&paths).expect("config should be readable");
        config.version = 999;
        write_config(&paths, &config).expect("config should be writable");

        assert!(matches!(
            read_config(&paths),
            Err(DaloError::UnsupportedSchema { version: 999, .. })
        ));
    }

    #[test]
    fn init_store_should_reject_empty_store_path() {
        let error =
            init_store(PathBuf::new(), false).expect_err("init should reject an empty path");

        assert!(matches!(error, DaloError::InvalidStorePath { .. }));
    }

    #[test]
    fn expand_user_path_should_not_lossily_rewrite_non_utf8_paths() {
        use std::os::unix::ffi::OsStringExt;

        let non_utf8 = PathBuf::from(OsString::from_vec(vec![0xff, b's', b't', b'o', b'r', b'e']));

        assert_eq!(
            expand_user_path(&non_utf8).expect("path should pass through"),
            non_utf8
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
        assert!(paths.lock_guard_file.is_file());
    }

    #[test]
    fn store_lock_should_ignore_stale_pid_metadata_when_file_is_unlocked() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root);
        fs::write(
            &paths.lock_guard_file,
            format!("pid={}\n", std::process::id()),
        )
        .expect("stale lock metadata should be written");

        let _lock = StoreLock::acquire_with_delays(&paths, &[Duration::from_millis(0)])
            .expect("unlocked stale metadata should not block acquisition");

        assert!(paths.lock_guard_file.is_file());
    }

    #[test]
    fn store_lock_should_grant_to_exactly_one_thread_for_unlocked_stale_file() {
        use std::sync::Arc;
        use std::sync::Barrier;

        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        init_store(store_root.clone(), false).expect("init should succeed");
        let paths = Arc::new(StorePaths::new(store_root));
        fs::write(&paths.lock_guard_file, "pid=2147480000\n")
            .expect("stale lock metadata should be written");
        // A single zero delay means neither thread retries: each makes exactly one
        // attempt against an unlocked stale file, so exactly one must win and the
        // other must observe `StoreLocked`.
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
