//! Agent target registry and detection.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::store::{self, StateFile, StorePaths, TargetState};

/// Target support level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSupport {
    /// Supported V1 target: its skill directory and symlink discovery are
    /// verified against a named agent release in `docs/agents.md`.
    Supported,
    /// Known but unverified target. No built-in target uses this level in V1;
    /// it stays available for targets registered before verification.
    Experimental,
}

impl TargetSupport {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Experimental => "experimental",
        }
    }
}

/// Static target registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetRegistryEntry {
    /// Target ID.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Default skill path.
    pub default_path: Option<&'static str>,
    /// Support level.
    pub support: TargetSupport,
}

/// One detected target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetDetection {
    /// Target ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Support level.
    pub support: TargetSupport,
    /// Linked path when configured, otherwise the expanded default path when known.
    pub path: Option<PathBuf>,
    /// Whether the path currently exists.
    pub exists: bool,
    /// Whether the target is linked in state.
    pub linked: bool,
}

/// Target detection report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetDetectReport {
    /// Detected targets.
    pub targets: Vec<TargetDetection>,
}

/// One verified native instruction-file destination shared by logical targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionFileDestination {
    /// Logical target IDs that resolve to this physical file.
    pub logical_targets: Vec<String>,
    /// Effective physical instruction-file path.
    pub path: PathBuf,
}

/// Target link status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetLinkStatus {
    /// Link would be created in dry-run mode.
    Planned,
    /// Target was linked.
    Linked,
    /// Existing target was updated.
    Updated,
    /// Target was already linked with the same path.
    Existing,
}

impl TargetLinkStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Linked => "linked",
            Self::Updated => "updated",
            Self::Existing => "existing",
        }
    }
}

/// Target link report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetLinkReport {
    /// Target ID.
    pub target_id: String,
    /// Expanded path.
    pub path: PathBuf,
    /// Canonical path.
    pub canonical_path: PathBuf,
    /// Link status.
    pub status: TargetLinkStatus,
    /// Whether the directory was created.
    pub created_dir: bool,
}

/// Target unlink status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetUnlinkStatus {
    /// Unlink would run in dry-run mode.
    Planned,
    /// Target was unlinked.
    Unlinked,
    /// Target was not linked.
    Missing,
}

impl TargetUnlinkStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Unlinked => "unlinked",
            Self::Missing => "missing",
        }
    }
}

/// Target unlink report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetUnlinkReport {
    /// Target ID.
    pub target_id: String,
    /// Unlink status.
    pub status: TargetUnlinkStatus,
}

/// Built-in target IDs as the `target link` hints spell them.
///
/// [`registry`] is the source of truth; `link_hint_should_name_every_builtin`
/// keeps this list and the CLI long help in step with it, so a newly supported
/// target cannot be left out of the command Dalo tells people to run.
pub const LINK_HINT_TARGETS: &str = "codex|claude|cursor|openclaw|hermes|opencode|generic";

/// Return the built-in target registry.
#[must_use]
pub fn registry() -> &'static [TargetRegistryEntry] {
    &[
        TargetRegistryEntry {
            id: "codex",
            name: "Codex",
            default_path: Some("~/.agents/skills"),
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "claude",
            name: "Claude Code",
            default_path: Some("~/.claude/skills"),
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "cursor",
            name: "Cursor",
            default_path: Some("~/.cursor/skills"),
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "openclaw",
            name: "OpenClaw",
            default_path: Some("~/.agents/skills"),
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "hermes",
            name: "Hermes",
            default_path: Some("~/.hermes/skills"),
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "opencode",
            name: "OpenCode",
            default_path: Some("~/.config/opencode/skills"),
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "generic",
            name: "Generic folder",
            default_path: None,
            support: TargetSupport::Supported,
        },
    ]
}

/// Resolve logical agent targets to verified native user-level instruction files.
///
/// Physical destinations are de-duplicated after resolving existing symlinks.
/// Targets without a verified native mapping fail closed instead of guessing.
pub fn resolve_instruction_files(
    target_ids: &[String],
) -> DaloResult<Vec<InstructionFileDestination>> {
    let default_config_dir = |name: &str| {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(name))
            .ok_or_else(|| DaloError::StateError {
                reason: "HOME is required to resolve native instruction files".to_owned(),
            })
    };
    let mut destinations = BTreeMap::<PathBuf, Vec<String>>::new();

    for target_id in target_ids {
        registry_entry(target_id)?;
        let path = match target_id.as_str() {
            "codex" => match env::var_os("CODEX_HOME") {
                Some(path) => PathBuf::from(path),
                None => default_config_dir(".codex")?,
            }
            .join("AGENTS.md"),
            "claude" => match env::var_os("CLAUDE_CONFIG_DIR") {
                Some(path) => PathBuf::from(path),
                None => default_config_dir(".claude")?,
            }
            .join("CLAUDE.md"),
            _ => {
                return Err(DaloError::InvalidArgument {
                    reason: format!(
                        "target `{target_id}` has no verified native instruction-file mapping; use an explicit file instead"
                    ),
                });
            }
        };
        let path = store::comparable_path(&store::absolute_path(&path)?);
        destinations
            .entry(path)
            .or_default()
            .push(target_id.clone());
    }

    Ok(destinations
        .into_iter()
        .map(|(path, mut logical_targets)| {
            logical_targets.sort();
            logical_targets.dedup();
            InstructionFileDestination {
                logical_targets,
                path,
            }
        })
        .collect())
}

/// Detect target paths and current link state.
#[must_use = "the target detection report should be rendered or inspected"]
pub fn detect_targets(store_root: &Path) -> DaloResult<TargetDetectReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let state = read_state_if_initialized(&paths)?;

    let mut targets = registry()
        .iter()
        .map(|entry| detect_entry(entry, state.as_ref()))
        .collect::<DaloResult<Vec<_>>>()?;

    if let Some(state) = &state {
        for target in state
            .targets
            .iter()
            .filter(|target| target.enabled && target.provider_id() != target.id)
        {
            let entry = registry_entry(target.provider_id())?;
            targets.push(TargetDetection {
                id: target.id.clone(),
                name: entry.name.to_owned(),
                support: entry.support,
                path: Some(target.path.clone()),
                exists: target.path.exists(),
                linked: true,
            });
        }
    }

    Ok(TargetDetectReport { targets })
}

/// Link a target.
pub fn link_target(
    store_root: &Path,
    target_id: &str,
    path_override: Option<&Path>,
    dry_run: bool,
) -> DaloResult<TargetLinkReport> {
    link_named_target(store_root, target_id, path_override, None, dry_run)
}

/// Link a provider under an optional distinct logical name.
pub fn link_named_target(
    store_root: &Path,
    target_id: &str,
    path_override: Option<&Path>,
    name: Option<&str>,
    dry_run: bool,
) -> DaloResult<TargetLinkReport> {
    let entry = registry_entry(target_id)?;
    let id = name.unwrap_or(entry.id);
    if let Some(name) = name {
        if !valid_instance_name(name) {
            return Err(DaloError::InvalidArgument {
                reason: format!("invalid or reserved target name `{name}`"),
            });
        }
        if path_override.is_none() {
            return Err(DaloError::InvalidArgument {
                reason: "named targets require an explicit path".to_owned(),
            });
        }
        if matches!(entry.id, "claude" | "codex")
            && path_override.and_then(Path::file_name) != Some(std::ffi::OsStr::new("skills"))
        {
            return Err(DaloError::InvalidArgument {
                reason: "named Claude and Codex target paths must end in `skills`".to_owned(),
            });
        }
    }
    let path = target_path(entry, path_override)?;
    let path = if name.is_some() {
        store::absolute_path(&path)?
    } else {
        path
    };
    let existed_before = fs::symlink_metadata(&path).is_ok();

    let paths = StorePaths::new(store_root.to_path_buf());
    let mut state = if dry_run && name.is_none() && !paths.state_file.exists() {
        StateFile::default()
    } else {
        store::read_state(&paths)?
    };

    let canonical_path = canonicalize_target_path(&path);
    if let Some(existing) = state.targets.iter().find(|target| target.id == id)
        && existing.provider_id() != entry.id
    {
        return Err(DaloError::InvalidArgument {
            reason: format!(
                "target name `{id}` already belongs to provider `{}`",
                existing.provider_id()
            ),
        });
    }
    let updated = upsert_target_state(&mut state, id, &path, &canonical_path)?;
    if name.is_some() {
        let target = state
            .targets
            .iter_mut()
            .find(|target| target.id == id)
            .expect("target just inserted");
        target.extra.insert(
            "instance_provider".to_owned(),
            toml::Value::String(entry.id.to_owned()),
        );
    }
    if matches!(entry.id, "claude" | "codex")
        && !crate::project::is_project_store(&paths.root)
        && state.targets.iter().any(|target| {
            target.enabled && target.provider_id() == entry.id && target.id != entry.id
        })
        && let Some(default) = state
            .targets
            .iter_mut()
            .find(|target| target.enabled && target.id == entry.id)
        && default.pinned_hook_root().is_none()
    {
        let root = default_hook_root(entry.id).ok_or_else(|| DaloError::StateError {
            reason: format!(
                "HOME is required to pin the {} hook configuration root",
                entry.id
            ),
        })?;
        let root = store::absolute_path(&root)?;
        let root = root.to_str().ok_or_else(|| DaloError::StateError {
            reason: "native hook configuration root is not valid UTF-8".to_owned(),
        })?;
        default.extra.insert(
            "instance_config_root".to_owned(),
            toml::Value::String(root.to_owned()),
        );
    }
    validate_target_state(&state, !crate::project::is_project_store(&paths.root))
        .map_err(|reason| DaloError::InvalidArgument { reason })?;
    let status = if dry_run {
        TargetLinkStatus::Planned
    } else {
        let created_dirs = if existed_before {
            if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    format!("target path `{}` is not a directory", path.display()),
                )
                .into());
            }
            Vec::new()
        } else {
            create_target_directories(&path)?
        };
        if let Err(error) = store::write_state(&paths, &state) {
            remove_created_target_directories(&created_dirs);
            return Err(error);
        }
        updated
    };

    Ok(TargetLinkReport {
        target_id: id.to_owned(),
        path,
        canonical_path,
        status,
        created_dir: !existed_before && !dry_run,
    })
}

pub(crate) fn valid_instance_name(name: &str) -> bool {
    name.len() <= 128
        && name
            .as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        && !registry().iter().any(|entry| entry.id == name)
}

pub(crate) fn validate_target_state(
    state: &StateFile,
    native_projections: bool,
) -> Result<(), String> {
    for (index, target) in state.targets.iter().enumerate() {
        if target.extra.contains_key("instance_config_root")
            && (!matches!(target.id.as_str(), "claude" | "codex")
                || target.extra.contains_key("instance_provider")
                || !target
                    .pinned_hook_root()
                    .is_some_and(|root| root.is_absolute() && !root.as_os_str().is_empty()))
        {
            return Err(format!(
                "invalid pinned hook root for target `{}`",
                target.id
            ));
        }
        let Some(value) = target.extra.get("instance_provider") else {
            continue;
        };
        let provider = value
            .as_str()
            .filter(|provider| registry().iter().any(|entry| entry.id == *provider))
            .ok_or_else(|| format!("invalid provider for target `{}`", target.id))?;
        if !valid_instance_name(&target.id) {
            return Err(format!("invalid named target ID `{}`", target.id));
        }
        if !target.path.is_absolute()
            || !target.canonical_path.is_absolute()
            || store::comparable_path(&target.path)
                != store::comparable_path(&target.canonical_path)
            || (matches!(provider, "codex" | "claude")
                && target.path.file_name() != Some(std::ffi::OsStr::new("skills")))
        {
            return Err(format!(
                "invalid or changed path for named target `{}`",
                target.id
            ));
        }
        for (other_index, other) in state.targets.iter().enumerate() {
            if index == other_index {
                continue;
            }
            if target.id == other.id {
                return Err(format!("duplicate target ID `{}`", target.id));
            }
            if !target.enabled || !other.enabled || provider != other.provider_id() {
                continue;
            }
            let target_path = store::comparable_path(&target.path);
            let other_path = store::comparable_path(&other.path);
            let same_skills = target_path == other_path;
            let same_codex_plugins =
                native_projections
                    && provider == "codex"
                    && target.path.parent().zip(other.path.parent()).is_some_and(
                        |(left, right)| {
                            store::comparable_path(left) == store::comparable_path(right)
                        },
                    );
            let same_hooks = native_projections
                && matches!(provider, "codex" | "claude")
                && target.path.parent().is_some_and(|root| {
                    let other_root = other
                        .configured_hook_root()
                        .map(Path::to_path_buf)
                        .or_else(|| default_hook_root(provider));
                    other_root.is_some_and(|other_root| {
                        store::comparable_path(root) == store::comparable_path(&other_root)
                    })
                });
            if same_skills || same_codex_plugins || same_hooks {
                return Err(format!(
                    "targets `{}` and `{}` share a {provider} destination",
                    target.id, other.id
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn default_hook_root(provider: &str) -> Option<PathBuf> {
    let (key, fallback) = match provider {
        "codex" => ("CODEX_HOME", ".codex"),
        "claude" => ("CLAUDE_CONFIG_DIR", ".claude"),
        _ => return None,
    };
    env::var_os(key).map(PathBuf::from).or_else(|| {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(fallback))
    })
}

fn missing_target_directories(path: &Path) -> Vec<PathBuf> {
    let mut missing = Vec::new();
    let mut current = Some(path);
    while let Some(candidate) = current.filter(|path| !path.as_os_str().is_empty()) {
        if fs::symlink_metadata(candidate).is_ok() {
            break;
        }
        missing.push(candidate.to_path_buf());
        current = candidate.parent();
    }
    missing.reverse();
    missing
}

fn create_target_directories(path: &Path) -> DaloResult<Vec<PathBuf>> {
    let mut created = Vec::new();
    for candidate in missing_target_directories(path) {
        match fs::create_dir(&candidate) {
            Ok(()) => created.push(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                remove_created_target_directories(&created);
                return Err(error.into());
            }
        }
    }

    if !fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        remove_created_target_directories(&created);
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("target path `{}` is not a directory", path.display()),
        )
        .into());
    }

    Ok(created)
}

fn remove_created_target_directories(created: &[PathBuf]) {
    for path in created.iter().rev() {
        // Removing only directories that this operation observed as missing
        // avoids touching pre-existing parents. Stop at the first non-empty
        // or otherwise unavailable directory so concurrent content is kept.
        if fs::remove_dir(path).is_err() {
            break;
        }
    }
}

/// Unlink a target from state without removing target files.
pub fn unlink_target(
    store_root: &Path,
    target_id: &str,
    dry_run: bool,
) -> DaloResult<TargetUnlinkReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let mut state = store::read_state_for_target_recovery(&paths)?;
    if !state.targets.iter().any(|target| target.id == target_id) {
        registry_entry(target_id)?;
    }
    let original_len = state.targets.len();
    let linked = state.targets.iter().any(|target| target.id == target_id);
    let status = if dry_run {
        if linked {
            TargetUnlinkStatus::Planned
        } else {
            TargetUnlinkStatus::Missing
        }
    } else {
        state.targets.retain(|target| target.id != target_id);
        validate_target_state(&state, !crate::project::is_project_store(&paths.root))
            .map_err(|reason| DaloError::StateError { reason })?;
        state.rebuild_materialization_dirs_for_removal()?;
        store::write_state(&paths, &state)?;

        if state.targets.len() == original_len {
            TargetUnlinkStatus::Missing
        } else {
            TargetUnlinkStatus::Unlinked
        }
    };

    Ok(TargetUnlinkReport {
        target_id: target_id.to_owned(),
        status,
    })
}

fn detect_entry(
    entry: &TargetRegistryEntry,
    state: Option<&StateFile>,
) -> DaloResult<TargetDetection> {
    let linked_target = state.and_then(|state| {
        state
            .targets
            .iter()
            .find(|target| target.id == entry.id && target.enabled)
    });
    let path = if let Some(target) = linked_target {
        Some(target.path.clone())
    } else {
        entry
            .default_path
            .map(PathBuf::from)
            .map(|path| store::expand_user_path(&path))
            .transpose()?
    };
    let exists = path.as_ref().is_some_and(|path| path.exists());
    let linked = linked_target.is_some();

    Ok(TargetDetection {
        id: entry.id.to_owned(),
        name: entry.name.to_owned(),
        support: entry.support,
        path,
        exists,
        linked,
    })
}

fn read_state_if_initialized(paths: &StorePaths) -> DaloResult<Option<StateFile>> {
    if paths.state_file.exists() {
        Ok(Some(store::read_state(paths)?))
    } else {
        Ok(None)
    }
}

fn registry_entry(target_id: &str) -> DaloResult<&'static TargetRegistryEntry> {
    registry()
        .iter()
        .find(|entry| entry.id == target_id)
        .ok_or_else(|| {
            DaloError::unknown_target(
                target_id,
                registry().iter().map(|entry| entry.id.to_owned()).collect(),
            )
        })
}

fn target_path(entry: &TargetRegistryEntry, path_override: Option<&Path>) -> DaloResult<PathBuf> {
    if let Some(path) = path_override {
        return store::expand_user_path(path);
    }

    let Some(default_path) = entry.default_path else {
        return Err(DaloError::TargetPathRequired {
            target: entry.id.to_owned(),
        });
    };

    store::expand_user_path(Path::new(default_path))
}

fn canonicalize_target_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    // The directory may not exist yet (notably during dry-run, which skips
    // `create_dir_all`). Canonicalize the parent and re-attach the final
    // component so a symlinked ancestor (e.g. `/var` -> `/private/var`) resolves
    // identically whether or not the leaf exists. This keeps the dry-run
    // "planned" canonical path equal to what the real run later persists.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(file_name)) => match parent.canonicalize() {
            Ok(canonical_parent) => canonical_parent.join(file_name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

fn upsert_target_state(
    state: &mut StateFile,
    target_id: &str,
    path: &Path,
    canonical_path: &Path,
) -> DaloResult<TargetLinkStatus> {
    let existing = state
        .targets
        .iter_mut()
        .find(|target| target.id == target_id);

    let status = if let Some(target) = existing {
        if target.path == path && target.canonical_path == canonical_path && target.enabled {
            TargetLinkStatus::Existing
        } else {
            target.path = path.to_path_buf();
            target.canonical_path = canonical_path.to_path_buf();
            target.enabled = true;
            TargetLinkStatus::Updated
        }
    } else {
        state.targets.push(TargetState {
            id: target_id.to_owned(),
            path: path.to_path_buf(),
            canonical_path: canonical_path.to_path_buf(),
            enabled: true,
            extra: Default::default(),
        });
        TargetLinkStatus::Linked
    };

    state.rebuild_materialization_dirs()?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_targets_preserve_provider_and_reject_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        let first = temp.path().join("one/skills");
        let second = temp.path().join("two/skills");
        store::init_store(root.clone(), false).unwrap();
        link_target(&root, "claude", Some(&first), false).unwrap();
        link_named_target(&root, "claude", Some(&second), Some("claude-eg"), false).unwrap();
        let state = store::read_state(&StorePaths::new(root.clone())).unwrap();
        assert_eq!(state.schema_version, 2);
        assert_eq!(state.targets[1].provider_id(), "claude");
        let pinned = state.targets[0].pinned_hook_root().unwrap().to_path_buf();
        assert!(
            detect_targets(&root)
                .unwrap()
                .targets
                .iter()
                .any(|t| t.id == "claude-eg" && t.linked)
        );
        assert!(
            link_named_target(&root, "codex", Some(&second), Some("claude-eg"), false).is_err()
        );
        assert!(link_named_target(&root, "claude", Some(&first), Some("another"), false).is_err());
        assert!(link_named_target(&root, "claude", Some(&second), Some("codex"), false).is_err());
        assert!(link_named_target(&root, "claude", Some(&second), Some("../bad"), false).is_err());
        assert!(link_named_target(&root, "claude", Some(temp.path()), Some("bad"), false).is_err());
        unlink_target(&root, "claude-eg", false).unwrap();
        let state = store::read_state(&StorePaths::new(root)).unwrap();
        assert_eq!(state.targets.len(), 1);
        assert_eq!(state.targets[0].id, "claude");
        assert_eq!(state.targets[0].pinned_hook_root(), Some(pinned.as_path()));
    }

    #[test]
    fn legacy_state_is_upgraded_when_named_target_is_written() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        store::init_store(root.clone(), false).unwrap();
        let paths = StorePaths::new(root.clone());
        let original = fs::read_to_string(&paths.state_file).unwrap();
        fs::write(
            &paths.state_file,
            original.replace("schema_version = 2", "schema_version = 1"),
        )
        .unwrap();
        link_named_target(
            &root,
            "claude",
            Some(&temp.path().join("alt/skills")),
            Some("alt"),
            false,
        )
        .unwrap();
        assert!(
            fs::read_to_string(paths.state_file)
                .unwrap()
                .contains("schema_version = 2")
        );
    }

    #[test]
    fn legacy_dry_run_does_not_require_initialized_store() {
        let temp = tempfile::tempdir().unwrap();
        let result = link_target(
            &temp.path().join("missing"),
            "generic",
            Some(&temp.path().join("skills")),
            true,
        )
        .unwrap();
        assert_eq!(result.status, TargetLinkStatus::Planned);
        assert!(!result.path.exists());
    }

    #[test]
    fn legacy_unknown_provider_field_stays_inert_after_migration() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        store::init_store(root.clone(), false).unwrap();
        link_target(&root, "generic", Some(&temp.path().join("first")), false).unwrap();
        let paths = StorePaths::new(root.clone());
        let original = fs::read_to_string(&paths.state_file).unwrap();
        let legacy = original
            .replace("schema_version = 2", "schema_version = 1")
            .replace(
                "id = \"generic\"",
                "id = \"generic\"\nprovider = \"future-value\"",
            );
        fs::write(&paths.state_file, legacy).unwrap();
        link_named_target(
            &root,
            "claude",
            Some(&temp.path().join("alt/skills")),
            Some("alt"),
            false,
        )
        .unwrap();
        let state = store::read_state(&paths).unwrap();
        assert_eq!(state.targets[0].provider_id(), "generic");
        assert_eq!(
            state.targets[0].extra["provider"].as_str(),
            Some("future-value")
        );
    }

    #[test]
    fn named_codex_rejects_shared_legacy_plugin_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        store::init_store(root.clone(), false).unwrap();
        link_target(
            &root,
            "codex",
            Some(&temp.path().join("config/custom")),
            false,
        )
        .unwrap();
        assert!(
            link_named_target(
                &root,
                "codex",
                Some(&temp.path().join("config/skills")),
                Some("other"),
                true
            )
            .is_err()
        );
    }

    #[test]
    fn registry_should_include_v1_targets() {
        let ids = registry().iter().map(|entry| entry.id).collect::<Vec<_>>();

        assert_eq!(
            ids,
            [
                "codex", "claude", "cursor", "openclaw", "hermes", "opencode", "generic",
            ]
        );
    }

    #[test]
    fn cursor_target_should_use_the_verified_user_skill_directory() {
        let entry = registry_entry("cursor").expect("Cursor should be a built-in target");

        assert_eq!(entry.name, "Cursor");
        assert_eq!(entry.default_path, Some("~/.cursor/skills"));
        assert_eq!(entry.support, TargetSupport::Supported);
    }

    #[test]
    fn link_hint_should_name_every_builtin() {
        // The hints told people to run
        // The target link hint once lagged behind the built-in registry, so the command Dalo prints
        // has to be derived from the registry, not maintained beside it.
        let ids = registry().iter().map(|entry| entry.id).collect::<Vec<_>>();

        assert_eq!(
            LINK_HINT_TARGETS,
            ids.join("|"),
            "the target link hint fell behind the registry"
        );
        assert!(
            crate::cli::AFTER_LONG_HELP
                .contains(&format!("dalo target link <{LINK_HINT_TARGETS}> [path]")),
            "the CLI long help fell behind the registry"
        );
    }

    #[test]
    fn registry_should_ship_only_verified_targets() {
        for entry in registry() {
            assert_eq!(
                entry.support,
                TargetSupport::Supported,
                "target `{}` ships an unverified support level",
                entry.id
            );
        }
    }

    #[test]
    fn registry_should_give_every_agent_target_a_default_path() {
        for entry in registry().iter().filter(|entry| entry.id != "generic") {
            assert!(
                entry.default_path.is_some(),
                "target `{}` ships without a default skill path",
                entry.id
            );
        }
    }

    #[test]
    fn link_target_should_dedupe_shared_physical_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let shared_target = temp_dir.path().join("skills");
        store::init_store(store_root.clone(), false).expect("init should succeed");

        link_target(&store_root, "codex", Some(&shared_target), false)
            .expect("codex link should succeed");
        link_target(&store_root, "openclaw", Some(&shared_target), false)
            .expect("openclaw link should succeed");

        let state =
            store::read_state(&StorePaths::new(store_root)).expect("state should be readable");

        assert_eq!(state.targets.len(), 2);
        assert_eq!(state.materialization_dirs.len(), 1);
        assert_eq!(
            state.materialization_dirs[0].logical_targets,
            ["codex".to_owned(), "openclaw".to_owned()]
        );
    }

    #[test]
    fn link_target_should_report_stable_canonical_path_between_dry_run_and_real_run() {
        use std::os::unix::fs as unix_fs;

        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        // A symlinked parent means the not-yet-created leaf path differs from its
        // canonical form. Dry-run (which skips dir creation) must still report the
        // same canonical path the real run later persists.
        let real_parent = temp_dir.path().join("real");
        std::fs::create_dir_all(&real_parent).expect("real parent should be created");
        let linked_parent = temp_dir.path().join("linked");
        unix_fs::symlink(&real_parent, &linked_parent).expect("symlink should be created");
        let target = linked_parent.join("skills");

        let planned = link_target(&store_root, "codex", Some(&target), true)
            .expect("dry-run link should succeed");
        let applied = link_target(&store_root, "codex", Some(&target), false)
            .expect("real link should succeed");

        assert_eq!(planned.canonical_path, applied.canonical_path);
    }

    #[test]
    fn link_target_should_read_store_before_creating_target_dir() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("missing-store");
        let target = temp_dir.path().join("skills");

        let error = link_target(&store_root, "codex", Some(&target), false)
            .expect_err("link should require an initialized store");

        assert!(matches!(error, DaloError::StoreNotInitialized { .. }));
        assert!(!target.exists());
    }

    #[test]
    fn link_target_should_not_create_directory_when_metadata_merge_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let shared_target = temp_dir.path().join("shared-skills");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root.clone());
        let state = conflicting_future_directory_state(temp_dir.path(), &shared_target);
        store::write_state(&paths, &state).expect("conflicting future state should be written");

        let error = link_target(&store_root, "codex", Some(&shared_target), false)
            .expect_err("lossy metadata merge should block link");

        assert!(matches!(error, DaloError::StateMetadataConflict { .. }));
        assert!(!shared_target.exists());
        assert_eq!(
            store::read_state(&paths).expect("state should remain readable"),
            state
        );
    }

    #[test]
    fn link_target_cleanup_should_remove_new_empty_ancestors_only() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let existing_parent = temp_dir.path().join("existing");
        let target = existing_parent.join("new/deep/skills");
        fs::create_dir_all(&existing_parent).expect("existing parent should be created");

        let created = missing_target_directories(&target);
        fs::create_dir_all(&target).expect("target hierarchy should be created");
        remove_created_target_directories(&created);

        assert!(existing_parent.is_dir());
        assert!(!target.exists());
        assert!(!existing_parent.join("new").exists());
    }

    #[test]
    fn link_target_should_reject_a_broken_symlink_without_recording_state() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("broken-skills");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        std::os::unix::fs::symlink(temp_dir.path().join("missing"), &target)
            .expect("broken target symlink should be created");

        let error = link_target(&store_root, "codex", Some(&target), false)
            .expect_err("broken target symlink should be rejected");

        assert!(error.to_string().contains("not a directory"));
        assert!(
            store::read_state(&StorePaths::new(store_root))
                .expect("state should remain readable")
                .targets
                .is_empty()
        );
    }

    #[test]
    fn unlink_target_should_drop_unrelated_conflicting_directory_metadata() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let shared_target = temp_dir.path().join("shared-skills");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root.clone());
        let state = conflicting_future_directory_state(temp_dir.path(), &shared_target);
        store::write_state(&paths, &state).expect("conflicting future state should be written");

        let report = unlink_target(&store_root, "codex", false)
            .expect("unrelated metadata must not wedge target removal");
        let updated = store::read_state(&paths).expect("updated state should be readable");

        assert_eq!(report.status, TargetUnlinkStatus::Unlinked);
        assert!(updated.targets.iter().all(|target| target.id != "codex"));
        assert_eq!(updated.materialization_dirs.len(), 1);
        assert_eq!(updated.materialization_dirs[0].path, shared_target);
        assert_eq!(
            updated.materialization_dirs[0].logical_targets,
            ["claude".to_owned(), "openclaw".to_owned()]
        );
        assert!(
            !updated.materialization_dirs[0]
                .extra
                .contains_key("future_directory_field")
        );
    }

    #[test]
    fn unlink_target_dry_run_should_report_missing_for_unlinked_target() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");

        let report =
            unlink_target(&store_root, "codex", true).expect("dry-run unlink should inspect state");

        assert_eq!(report.status, TargetUnlinkStatus::Missing);
    }

    #[test]
    fn link_generic_should_require_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");

        let error = link_target(&store_root, "generic", None, false)
            .expect_err("generic should require a path");

        assert_eq!(
            error.to_string(),
            "target `generic` requires an explicit path"
        );
    }

    fn conflicting_future_directory_state(root: &Path, shared_target: &Path) -> StateFile {
        let mut state = StateFile::empty();
        state.targets = vec![
            TargetState {
                id: "codex".to_owned(),
                path: root.join("old-codex"),
                canonical_path: root.join("old-codex"),
                enabled: true,
                extra: Default::default(),
            },
            TargetState {
                id: "claude".to_owned(),
                path: root.join("old-claude"),
                canonical_path: root.join("old-claude"),
                enabled: true,
                extra: Default::default(),
            },
            TargetState {
                id: "openclaw".to_owned(),
                path: root.join("old-openclaw"),
                canonical_path: root.join("old-openclaw"),
                enabled: true,
                extra: Default::default(),
            },
        ];
        state
            .rebuild_materialization_dirs()
            .expect("initial directories should rebuild");
        for (target_id, value) in [("claude", "claude value"), ("openclaw", "openclaw value")] {
            state
                .materialization_dirs
                .iter_mut()
                .find(|directory| directory.logical_targets == [target_id.to_owned()])
                .expect("target directory should exist")
                .extra
                .insert(
                    "future_directory_field".to_owned(),
                    toml::Value::String(value.to_owned()),
                );
        }
        for target in state
            .targets
            .iter_mut()
            .filter(|target| target.id != "codex")
        {
            target.path = shared_target.to_path_buf();
            target.canonical_path = shared_target.to_path_buf();
        }
        state
    }
}
