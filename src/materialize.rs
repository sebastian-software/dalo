//! Materialization planning and symlink reconciliation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::DaloResult;
use crate::resolver::Resolution;
use crate::store::{self, OwnedSkillState, StateFile, StorePaths};

/// Sync and materialization report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    /// Store root.
    pub store: PathBuf,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
    /// Planned and applied operations.
    pub operations: Vec<MaterializeOperation>,
}

/// One materialization operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaterializeOperation {
    /// Operation kind.
    pub kind: MaterializeOperationKind,
    /// Target link path.
    pub link_path: PathBuf,
    /// Desired store path when applicable.
    pub desired_path: Option<PathBuf>,
    /// Operation status.
    pub status: MaterializeOperationStatus,
    /// Optional reason.
    pub reason: Option<String>,
}

/// Materialization operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializeOperationKind {
    /// Create a new owned symlink.
    Create,
    /// Relink an owned symlink to the desired path.
    Relink,
    /// Remove an owned symlink because it is no longer desired.
    Remove,
    /// Drop a stale ownership record without touching the filesystem.
    DropRecord,
    /// Conflict that must not be applied.
    Conflict,
    /// No action needed.
    NoOp,
}

impl MaterializeOperationKind {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Relink => "relink",
            Self::Remove => "remove",
            Self::DropRecord => "drop_record",
            Self::Conflict => "conflict",
            Self::NoOp => "noop",
        }
    }
}

/// Materialization operation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializeOperationStatus {
    /// Planned in dry-run mode.
    Planned,
    /// Applied during this run.
    Applied,
    /// Existing state already matched the desired state.
    Existing,
    /// Blocked because applying would touch unmanaged or foreign content.
    Blocked,
}

impl MaterializeOperationStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Applied => "applied",
            Self::Existing => "existing",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesiredLink {
    target_id: String,
    slot_name: String,
    link_path: PathBuf,
    store_path: PathBuf,
}

/// Materialize resolved skills into configured target directories.
pub fn materialize(
    paths: &StorePaths,
    resolution: &Resolution,
    dry_run: bool,
) -> DaloResult<SyncReport> {
    let mut state = store::read_state(paths)?;
    let desired_links = desired_links(&state, resolution);
    let mut operations = build_plan(paths, &state, &desired_links)?;

    if dry_run {
        for operation in &mut operations {
            if operation.status != MaterializeOperationStatus::Blocked
                && operation.status != MaterializeOperationStatus::Existing
            {
                operation.status = MaterializeOperationStatus::Planned;
            }
        }
    } else {
        apply_plan(&mut state, &operations, &desired_links)?;
        store::write_state(paths, &state)?;
    }

    Ok(SyncReport {
        store: paths.root.clone(),
        dry_run,
        operations,
    })
}

fn desired_links(state: &StateFile, resolution: &Resolution) -> Vec<DesiredLink> {
    let active_targets = state
        .targets
        .iter()
        .filter(|target| target.enabled)
        .map(|target| (target.canonical_path.clone(), target.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut links = Vec::new();

    for (target_dir, target_id) in active_targets {
        for skill in &resolution.active_skills {
            links.push(DesiredLink {
                target_id: target_id.clone(),
                slot_name: skill.slot_name.clone(),
                link_path: target_dir.join(&skill.slot_name),
                store_path: skill.path.clone(),
            });
        }
    }

    links.sort_by(|left, right| left.link_path.cmp(&right.link_path));
    links
}

fn build_plan(
    paths: &StorePaths,
    state: &StateFile,
    desired_links: &[DesiredLink],
) -> DaloResult<Vec<MaterializeOperation>> {
    let desired_by_link = desired_links
        .iter()
        .map(|desired| (desired.link_path.clone(), desired))
        .collect::<BTreeMap<_, _>>();
    let recorded_by_link = state
        .owned_skills
        .iter()
        .map(|record| (record.link_path.clone(), record))
        .collect::<BTreeMap<_, _>>();
    let all_links = desired_by_link
        .keys()
        .chain(recorded_by_link.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut operations = Vec::new();

    for link_path in all_links {
        match (
            desired_by_link.get(&link_path),
            recorded_by_link.get(&link_path),
        ) {
            (Some(desired), Some(_)) => {
                operations.push(plan_desired_recorded(paths, desired)?);
            }
            (Some(desired), None) => {
                operations.push(plan_desired_unrecorded(desired)?);
            }
            (None, Some(recorded)) => {
                operations.push(plan_undesired_recorded(recorded));
            }
            (None, None) => {}
        }
    }

    Ok(operations)
}

fn plan_desired_recorded(
    paths: &StorePaths,
    desired: &DesiredLink,
) -> DaloResult<MaterializeOperation> {
    let actual = actual_link_state(&desired.link_path)?;
    let operation = match actual {
        ActualLinkState::Absent => MaterializeOperation {
            kind: MaterializeOperationKind::Create,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Applied,
            reason: Some("recorded symlink is missing".to_owned()),
        },
        ActualLinkState::Symlink(target) if target == desired.store_path => MaterializeOperation {
            kind: MaterializeOperationKind::NoOp,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Existing,
            reason: None,
        },
        ActualLinkState::Symlink(target) if target.starts_with(&paths.root) => {
            MaterializeOperation {
                kind: MaterializeOperationKind::Relink,
                link_path: desired.link_path.clone(),
                desired_path: Some(desired.store_path.clone()),
                status: MaterializeOperationStatus::Applied,
                reason: Some(format!("owned symlink points to `{}`", target.display())),
            }
        }
        ActualLinkState::Symlink(target) => MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some(format!("foreign symlink points to `{}`", target.display())),
        },
        ActualLinkState::RealEntry => MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some("real unmanaged entry exists at target slot".to_owned()),
        },
    };

    Ok(operation)
}

fn plan_desired_unrecorded(desired: &DesiredLink) -> DaloResult<MaterializeOperation> {
    let actual = actual_link_state(&desired.link_path)?;
    let operation = match actual {
        ActualLinkState::Absent => MaterializeOperation {
            kind: MaterializeOperationKind::Create,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Applied,
            reason: None,
        },
        ActualLinkState::Symlink(target) => MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some(format!("foreign symlink points to `{}`", target.display())),
        },
        ActualLinkState::RealEntry => MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some("real unmanaged entry exists at target slot".to_owned()),
        },
    };

    Ok(operation)
}

fn plan_undesired_recorded(recorded: &OwnedSkillState) -> MaterializeOperation {
    match actual_link_state(&recorded.link_path) {
        Ok(ActualLinkState::Symlink(_)) => MaterializeOperation {
            kind: MaterializeOperationKind::Remove,
            link_path: recorded.link_path.clone(),
            desired_path: None,
            status: MaterializeOperationStatus::Applied,
            reason: Some("recorded skill is no longer desired".to_owned()),
        },
        Ok(ActualLinkState::Absent) => MaterializeOperation {
            kind: MaterializeOperationKind::DropRecord,
            link_path: recorded.link_path.clone(),
            desired_path: None,
            status: MaterializeOperationStatus::Applied,
            reason: Some("recorded symlink is already absent".to_owned()),
        },
        Ok(ActualLinkState::RealEntry) | Err(_) => MaterializeOperation {
            kind: MaterializeOperationKind::DropRecord,
            link_path: recorded.link_path.clone(),
            desired_path: None,
            status: MaterializeOperationStatus::Applied,
            reason: Some("recorded slot is no longer owned".to_owned()),
        },
    }
}

fn apply_plan(
    state: &mut StateFile,
    operations: &[MaterializeOperation],
    desired_links: &[DesiredLink],
) -> DaloResult<()> {
    for operation in operations {
        match operation.kind {
            MaterializeOperationKind::Create | MaterializeOperationKind::Relink => {
                if operation.status == MaterializeOperationStatus::Blocked {
                    continue;
                }
                if operation.link_path.exists()
                    || fs::symlink_metadata(&operation.link_path)
                        .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    fs::remove_file(&operation.link_path)?;
                }
                if let Some(parent) = operation.link_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let Some(desired_path) = operation.desired_path.as_ref() else {
                    continue;
                };
                unix_fs::symlink(desired_path, &operation.link_path)?;
            }
            MaterializeOperationKind::Remove => {
                if fs::symlink_metadata(&operation.link_path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    fs::remove_file(&operation.link_path)?;
                }
            }
            MaterializeOperationKind::DropRecord
            | MaterializeOperationKind::Conflict
            | MaterializeOperationKind::NoOp => {}
        }
    }

    state.owned_skills = desired_links
        .iter()
        .filter(|desired| {
            !operations.iter().any(|operation| {
                operation.link_path == desired.link_path
                    && operation.kind == MaterializeOperationKind::Conflict
            })
        })
        .map(|desired| OwnedSkillState {
            target_id: desired.target_id.clone(),
            slot_name: desired.slot_name.clone(),
            link_path: desired.link_path.clone(),
            store_path: desired.store_path.clone(),
        })
        .collect();

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActualLinkState {
    Absent,
    Symlink(PathBuf),
    RealEntry,
}

fn actual_link_state(link_path: &Path) -> DaloResult<ActualLinkState> {
    match fs::symlink_metadata(link_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Ok(ActualLinkState::Symlink(fs::read_link(link_path)?))
        }
        Ok(_) => Ok(ActualLinkState::RealEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ActualLinkState::Absent),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::resolver::{Resolution, ResolvedSkill};
    use crate::source::SourceKind;
    use crate::store::{MaterializationDirState, TargetState};

    #[test]
    fn materialize_should_create_directory_symlink() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        write_state_with_target(&store_root, &target_dir);

        let report = materialize(
            &StorePaths::new(store_root),
            &resolution_with_skill("review", &skill_dir),
            false,
        )
        .expect("materialize should succeed");

        assert_eq!(report.operations[0].kind, MaterializeOperationKind::Create);
        assert!(
            fs::symlink_metadata(target_dir.join("review"))
                .expect("link should exist")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn materialize_should_block_unmanaged_real_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(target_dir.join("review")).expect("unmanaged dir should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        write_state_with_target(&store_root, &target_dir);

        let report = materialize(
            &StorePaths::new(store_root),
            &resolution_with_skill("review", &skill_dir),
            false,
        )
        .expect("materialize should succeed");

        assert_eq!(
            report.operations[0].kind,
            MaterializeOperationKind::Conflict
        );
    }

    #[test]
    fn materialize_should_be_idempotent() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        write_state_with_target(&store_root, &target_dir);
        let paths = StorePaths::new(store_root);
        let resolution = resolution_with_skill("review", &skill_dir);
        materialize(&paths, &resolution, false).expect("first materialize should succeed");

        let report =
            materialize(&paths, &resolution, false).expect("second materialize should succeed");

        assert_eq!(report.operations[0].kind, MaterializeOperationKind::NoOp);
    }

    fn write_state_with_target(store_root: &Path, target_dir: &Path) {
        let paths = StorePaths::new(store_root.to_path_buf());
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.targets = vec![TargetState {
            id: "generic".to_owned(),
            path: target_dir.to_path_buf(),
            canonical_path: target_dir.to_path_buf(),
            enabled: true,
        }];
        state.materialization_dirs = vec![MaterializationDirState {
            path: target_dir.to_path_buf(),
            logical_targets: vec!["generic".to_owned()],
        }];
        store::write_state(&paths, &state).expect("state should be written");
    }

    fn resolution_with_skill(slot_name: &str, path: &Path) -> Resolution {
        Resolution {
            active_skills: vec![ResolvedSkill {
                source_ref: format!("local:{slot_name}"),
                slot_name: slot_name.to_owned(),
                id: None,
                source_id: "local".to_owned(),
                source_kind: SourceKind::Local,
                source_priority: 0,
                path: path.to_path_buf(),
                local_override: false,
            }],
            pending_approval_skills: Vec::new(),
            unlinked_skills: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}
