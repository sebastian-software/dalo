//! Agent target registry and detection.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::store::{self, StateFile, StorePaths, TargetState};

/// Target support level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSupport {
    /// Supported V1 target.
    Supported,
    /// Known but unverified target.
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
    /// Expanded default path when known.
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
            id: "generic",
            name: "Generic folder",
            default_path: None,
            support: TargetSupport::Supported,
        },
        TargetRegistryEntry {
            id: "cursor",
            name: "Cursor",
            default_path: None,
            support: TargetSupport::Experimental,
        },
        TargetRegistryEntry {
            id: "opencode",
            name: "OpenCode",
            default_path: None,
            support: TargetSupport::Experimental,
        },
    ]
}

/// Detect target paths and current link state.
#[must_use = "the target detection report should be rendered or inspected"]
pub fn detect_targets(store_root: &Path) -> DaloResult<TargetDetectReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let state = read_state_if_initialized(&paths)?;

    let targets = registry()
        .iter()
        .map(|entry| detect_entry(entry, state.as_ref()))
        .collect::<DaloResult<Vec<_>>>()?;

    Ok(TargetDetectReport { targets })
}

/// Link a target.
pub fn link_target(
    store_root: &Path,
    target_id: &str,
    path_override: Option<&Path>,
    dry_run: bool,
) -> DaloResult<TargetLinkReport> {
    let entry = registry_entry(target_id)?;
    let path = target_path(entry, path_override)?;
    let existed_before = path.exists();

    if !existed_before && !dry_run {
        fs::create_dir_all(&path)?;
    }

    let canonical_path = canonicalize_target_path(&path);
    let status = if dry_run {
        TargetLinkStatus::Planned
    } else {
        let paths = StorePaths::new(store_root.to_path_buf());
        let mut state = store::read_state(&paths)?;
        let status = upsert_target_state(&mut state, entry.id, &path, &canonical_path);
        store::write_state(&paths, &state)?;
        status
    };

    Ok(TargetLinkReport {
        target_id: entry.id.to_owned(),
        path,
        canonical_path,
        status,
        created_dir: !existed_before && !dry_run,
    })
}

/// Unlink a target from state without removing target files.
pub fn unlink_target(
    store_root: &Path,
    target_id: &str,
    dry_run: bool,
) -> DaloResult<TargetUnlinkReport> {
    registry_entry(target_id)?;

    let status = if dry_run {
        TargetUnlinkStatus::Planned
    } else {
        let paths = StorePaths::new(store_root.to_path_buf());
        let mut state = store::read_state(&paths)?;
        let original_len = state.targets.len();
        state.targets.retain(|target| target.id != target_id);
        state.rebuild_materialization_dirs();
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
    let path = entry
        .default_path
        .map(PathBuf::from)
        .map(|path| store::expand_user_path(&path))
        .transpose()?;
    let exists = path.as_ref().is_some_and(|path| path.exists());
    let linked = state.is_some_and(|state| {
        state
            .targets
            .iter()
            .any(|target| target.id == entry.id && target.enabled)
    });

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
        .ok_or_else(|| DaloError::UnknownTarget {
            target: target_id.to_owned(),
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
) -> TargetLinkStatus {
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
        });
        TargetLinkStatus::Linked
    };

    state.rebuild_materialization_dirs();
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_should_include_v1_targets() {
        let ids = registry().iter().map(|entry| entry.id).collect::<Vec<_>>();

        assert_eq!(
            ids,
            [
                "codex", "claude", "openclaw", "hermes", "generic", "cursor", "opencode",
            ]
        );
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
}
