//! Materialization planning and symlink reconciliation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::DaloResult;
use crate::resolver::Resolution;
use crate::store::{self, OwnedSkillState, StateFile, StorePaths};

/// Enabled source whose live scan was degraded during sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradedSource {
    /// Source ID.
    pub id: String,
    /// Source root path.
    pub path: PathBuf,
}

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
    materialize_with_degraded_sources(paths, resolution, dry_run, &[])
}

/// Materialize resolved skills while preserving records from degraded sources.
pub fn materialize_with_degraded_sources(
    paths: &StorePaths,
    resolution: &Resolution,
    dry_run: bool,
    degraded_sources: &[DegradedSource],
) -> DaloResult<SyncReport> {
    let mut state = store::read_state(paths)?;
    let desired_links = desired_links(&state, resolution);
    let mut operations = build_plan(paths, &state, &desired_links, degraded_sources)?;

    if dry_run {
        for operation in &mut operations {
            if operation.status != MaterializeOperationStatus::Blocked
                && operation.status != MaterializeOperationStatus::Existing
            {
                operation.status = MaterializeOperationStatus::Planned;
            }
        }
    } else {
        apply_plan(paths, &mut state, &mut operations, &desired_links)?;
        store::write_state(paths, &state)?;
    }

    Ok(SyncReport {
        store: paths.root.clone(),
        dry_run,
        operations,
    })
}

fn desired_links(state: &StateFile, resolution: &Resolution) -> Vec<DesiredLink> {
    let mut links = Vec::new();

    // Drive links from the canonical materialization directories, not from the raw
    // logical targets: several logical targets can share one physical directory
    // (e.g. `codex` and `openclaw` both at `~/.agents/skills`). `logical_targets`
    // is kept sorted, so its first element is a deterministic representative for
    // the recorded `target_id` instead of an arbitrary last-writer-wins value.
    for dir in &state.materialization_dirs {
        let Some(target_id) = dir.logical_targets.first() else {
            continue;
        };
        for skill in &resolution.active_skills {
            links.push(DesiredLink {
                target_id: target_id.clone(),
                slot_name: skill.slot_name.clone(),
                link_path: dir.path.join(&skill.slot_name),
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
    degraded_sources: &[DegradedSource],
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
                operations.push(plan_desired_unrecorded(paths, desired)?);
            }
            (None, Some(recorded)) => {
                operations.push(plan_undesired_recorded(paths, recorded, degraded_sources));
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
        ActualLinkState::Symlink(target)
            if link_target_matches(&desired.link_path, &target, &desired.store_path) =>
        {
            MaterializeOperation {
                kind: MaterializeOperationKind::NoOp,
                link_path: desired.link_path.clone(),
                desired_path: Some(desired.store_path.clone()),
                status: MaterializeOperationStatus::Existing,
                reason: None,
            }
        }
        ActualLinkState::Symlink(target)
            if is_owned_link_target(paths, &desired.link_path, &desired.store_path, &target) =>
        {
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

fn plan_desired_unrecorded(
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
            reason: None,
        },
        ActualLinkState::Symlink(target)
            if is_owned_link_target(paths, &desired.link_path, &desired.store_path, &target) =>
        {
            MaterializeOperation {
                kind: MaterializeOperationKind::Relink,
                link_path: desired.link_path.clone(),
                desired_path: Some(desired.store_path.clone()),
                status: MaterializeOperationStatus::Applied,
                reason: Some(format!(
                    "unrecorded owned symlink points to `{}`",
                    target.display()
                )),
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

fn plan_undesired_recorded(
    paths: &StorePaths,
    recorded: &OwnedSkillState,
    degraded_sources: &[DegradedSource],
) -> MaterializeOperation {
    if let Some(source) = degraded_sources
        .iter()
        .find(|source| recorded.store_path.starts_with(&source.path))
    {
        return MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: recorded.link_path.clone(),
            desired_path: Some(recorded.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some(format!(
                "source `{}` scan degraded; preserving recorded owned link",
                source.id
            )),
        };
    }

    match actual_link_state(&recorded.link_path) {
        Ok(ActualLinkState::Symlink(target))
            if is_owned_link_target(paths, &recorded.link_path, &recorded.store_path, &target) =>
        {
            MaterializeOperation {
                kind: MaterializeOperationKind::Remove,
                link_path: recorded.link_path.clone(),
                desired_path: Some(recorded.store_path.clone()),
                status: MaterializeOperationStatus::Applied,
                reason: Some("recorded skill is no longer desired".to_owned()),
            }
        }
        Ok(ActualLinkState::Symlink(target)) => MaterializeOperation {
            kind: MaterializeOperationKind::DropRecord,
            link_path: recorded.link_path.clone(),
            desired_path: Some(recorded.store_path.clone()),
            status: MaterializeOperationStatus::Applied,
            reason: Some(format!(
                "recorded slot now contains foreign symlink to `{}`",
                target.display()
            )),
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
    paths: &StorePaths,
    state: &mut StateFile,
    operations: &mut [MaterializeOperation],
    desired_links: &[DesiredLink],
) -> DaloResult<()> {
    let previous_owned_skills = state.owned_skills.clone();

    for operation in operations.iter_mut() {
        match operation.kind {
            MaterializeOperationKind::Create | MaterializeOperationKind::Relink => {
                if operation.status == MaterializeOperationStatus::Blocked {
                    continue;
                }
                match fs::symlink_metadata(&operation.link_path) {
                    // Only ever unlink a symlink we are about to replace.
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        fs::remove_file(&operation.link_path)?;
                    }
                    // A real file/dir appeared at the slot after planning (TOCTOU).
                    // Refuse to delete unmanaged content; skip rather than overwrite.
                    Ok(_) => {
                        operation.kind = MaterializeOperationKind::Conflict;
                        operation.status = MaterializeOperationStatus::Blocked;
                        operation.reason =
                            Some("real unmanaged entry appeared at target slot".to_owned());
                        continue;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
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
                if let Some(recorded_store_path) = operation.desired_path.as_ref()
                    && let ActualLinkState::Symlink(target) =
                        actual_link_state(&operation.link_path)?
                    && is_owned_link_target(
                        paths,
                        &operation.link_path,
                        recorded_store_path,
                        &target,
                    )
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
    for operation in operations
        .iter()
        .filter(|operation| is_degraded_source_preserve(operation))
    {
        let Some(preserved) = previous_owned_skills
            .iter()
            .find(|record| record.link_path == operation.link_path)
        else {
            continue;
        };
        if !state
            .owned_skills
            .iter()
            .any(|record| record.link_path == preserved.link_path)
        {
            state.owned_skills.push(preserved.clone());
        }
    }

    Ok(())
}

fn link_target_matches(link_path: &Path, target: &Path, expected: &Path) -> bool {
    store::comparable_path(&store::resolve_link_target(link_path, target))
        == store::comparable_path(expected)
}

fn is_owned_link_target(
    paths: &StorePaths,
    link_path: &Path,
    recorded_store_path: &Path,
    target: &Path,
) -> bool {
    link_target_matches(link_path, target, recorded_store_path)
        || store::path_is_same_or_descendant(
            &store::resolve_link_target(link_path, target),
            &paths.root,
        )
}

fn is_degraded_source_preserve(operation: &MaterializeOperation) -> bool {
    operation.kind == MaterializeOperationKind::Conflict
        && operation.status == MaterializeOperationStatus::Blocked
        && operation
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("scan degraded"))
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
    fn materialize_should_block_foreign_symlink() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        let foreign_dir = temp_dir.path().join("foreign");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        fs::create_dir_all(&foreign_dir).expect("foreign dir should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        unix_fs::symlink(&foreign_dir, target_dir.join("review"))
            .expect("foreign symlink should be created");
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
    fn materialize_should_block_dangling_foreign_symlink() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        let missing_foreign_dir = temp_dir.path().join("missing-foreign");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        unix_fs::symlink(&missing_foreign_dir, target_dir.join("review"))
            .expect("dangling foreign symlink should be created");
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
        assert!(
            fs::symlink_metadata(target_dir.join("review"))
                .expect("dangling symlink should survive")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn materialize_should_relink_owned_symlink_pointing_elsewhere() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        let stale_store_dir = store_root.join("local/skills/review-old");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        fs::create_dir_all(&stale_store_dir).expect("stale skill should be created");
        let link_path = target_dir.join("review");
        // An owned symlink that already exists but points at a different in-store path
        // must be relinked, not treated as a foreign conflict.
        unix_fs::symlink(&stale_store_dir, &link_path).expect("owned symlink should be created");
        write_state_with_owned_skill(&store_root, &target_dir, &link_path, &stale_store_dir);

        let report = materialize(
            &StorePaths::new(store_root),
            &resolution_with_skill("review", &skill_dir),
            false,
        )
        .expect("materialize should succeed");

        assert_eq!(report.operations[0].kind, MaterializeOperationKind::Relink);
    }

    #[test]
    fn materialize_should_relink_unrecorded_store_symlink() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        let link_path = target_dir.join("review");
        unix_fs::symlink(&skill_dir, &link_path).expect("unrecorded store symlink should exist");
        write_state_with_target(&store_root, &target_dir);
        let paths = StorePaths::new(store_root);

        let report = materialize(&paths, &resolution_with_skill("review", &skill_dir), false)
            .expect("materialize should succeed");

        assert_eq!(report.operations[0].kind, MaterializeOperationKind::Relink);
        let state = store::read_state(&paths).expect("state should be readable");
        assert_eq!(state.owned_skills.len(), 1);
        assert_eq!(state.owned_skills[0].link_path, link_path);
        assert_eq!(
            fs::read_link(&state.owned_skills[0].link_path).expect("link should be readable"),
            skill_dir
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

    #[test]
    fn materialize_should_accept_owned_symlink_to_store_equivalent_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let store_alias = temp_dir.path().join("store-alias");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        unix_fs::symlink(&store_root, &store_alias).expect("store alias should be created");
        let link_path = target_dir.join("review");
        unix_fs::symlink(store_alias.join("local/skills/review"), &link_path)
            .expect("owned symlink should be created");
        write_state_with_owned_skill(&store_root, &target_dir, &link_path, &skill_dir);

        let report = materialize(
            &StorePaths::new(store_root),
            &resolution_with_skill("review", &skill_dir),
            false,
        )
        .expect("materialize should succeed");

        assert_eq!(report.operations[0].kind, MaterializeOperationKind::NoOp);
    }

    #[test]
    fn apply_plan_should_not_replace_real_entry_at_create_slot() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let link_path = temp_dir.path().join("review");
        fs::write(&link_path, "unmanaged user file").expect("real file should be written");
        let store_path = temp_dir.path().join("store/review");
        let operation = MaterializeOperation {
            kind: MaterializeOperationKind::Create,
            link_path: link_path.clone(),
            desired_path: Some(store_path.clone()),
            status: MaterializeOperationStatus::Applied,
            reason: None,
        };
        let desired = DesiredLink {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link_path.clone(),
            store_path,
        };

        let paths = StorePaths::new(temp_dir.path().join("store"));
        let mut state = StateFile::empty();
        let mut operations = vec![operation];

        apply_plan(&paths, &mut state, &mut operations, &[desired]).expect("apply should succeed");

        assert_eq!(operations[0].kind, MaterializeOperationKind::Conflict);
        assert_eq!(operations[0].status, MaterializeOperationStatus::Blocked);
        assert_eq!(
            operations[0].reason.as_deref(),
            Some("real unmanaged entry appeared at target slot")
        );
        assert!(state.owned_skills.is_empty());
        assert!(
            !fs::symlink_metadata(&link_path)
                .expect("file should still exist")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn materialize_should_drop_record_without_removing_foreign_symlink_at_recorded_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        let foreign_dir = temp_dir.path().join("foreign");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        fs::create_dir_all(&foreign_dir).expect("foreign dir should be created");
        let recorded_store_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&recorded_store_dir).expect("recorded store dir should be created");
        let link_path = target_dir.join("review");
        unix_fs::symlink(&foreign_dir, &link_path).expect("foreign symlink should be created");
        write_state_with_owned_skill(&store_root, &target_dir, &link_path, &recorded_store_dir);

        let report = materialize(
            &StorePaths::new(store_root.clone()),
            &empty_resolution(),
            false,
        )
        .expect("materialize should succeed");

        assert_eq!(
            report.operations[0].kind,
            MaterializeOperationKind::DropRecord
        );
        assert_eq!(
            fs::read_link(&link_path).expect("foreign symlink should survive"),
            foreign_dir
        );
        let state =
            store::read_state(&StorePaths::new(store_root)).expect("state should be readable");
        assert!(state.owned_skills.is_empty());
    }

    #[test]
    fn apply_plan_should_recheck_remove_target_before_unlinking() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        let foreign_dir = temp_dir.path().join("foreign");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        fs::create_dir_all(&foreign_dir).expect("foreign dir should be created");
        let recorded_store_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&recorded_store_dir).expect("recorded store dir should be created");
        let link_path = target_dir.join("review");
        unix_fs::symlink(&foreign_dir, &link_path).expect("foreign symlink should be created");
        let mut state = StateFile::empty();
        state.owned_skills = vec![OwnedSkillState {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link_path.clone(),
            store_path: recorded_store_dir.clone(),
        }];
        let operation = MaterializeOperation {
            kind: MaterializeOperationKind::Remove,
            link_path: link_path.clone(),
            desired_path: Some(recorded_store_dir),
            status: MaterializeOperationStatus::Applied,
            reason: Some("recorded skill is no longer desired".to_owned()),
        };
        let mut operations = vec![operation];

        apply_plan(
            &StorePaths::new(store_root),
            &mut state,
            &mut operations,
            &[],
        )
        .expect("apply should succeed");

        assert_eq!(
            fs::read_link(&link_path).expect("foreign symlink should survive"),
            foreign_dir
        );
        assert!(state.owned_skills.is_empty());
    }

    #[test]
    fn desired_links_should_pick_deterministic_target_id_for_shared_dir() {
        let shared = PathBuf::from("/agents/skills");
        let state = StateFile {
            schema_version: store::STATE_SCHEMA_VERSION,
            targets: vec![
                TargetState {
                    id: "openclaw".to_owned(),
                    path: shared.clone(),
                    canonical_path: shared.clone(),
                    enabled: true,
                },
                TargetState {
                    id: "codex".to_owned(),
                    path: shared.clone(),
                    canonical_path: shared.clone(),
                    enabled: true,
                },
            ],
            materialization_dirs: vec![MaterializationDirState {
                path: shared,
                logical_targets: vec!["codex".to_owned(), "openclaw".to_owned()],
            }],
            owned_skills: Vec::new(),
            protected_skills: Vec::new(),
        };
        let resolution = resolution_with_skill("review", Path::new("/store/review"));

        let links = desired_links(&state, &resolution);

        assert_eq!(links[0].target_id, "codex");
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

    fn write_state_with_owned_skill(
        store_root: &Path,
        target_dir: &Path,
        link_path: &Path,
        store_path: &Path,
    ) {
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
        state.owned_skills = vec![OwnedSkillState {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link_path.to_path_buf(),
            store_path: store_path.to_path_buf(),
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
            blocked_skills: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn empty_resolution() -> Resolution {
        Resolution {
            active_skills: Vec::new(),
            pending_approval_skills: Vec::new(),
            unlinked_skills: Vec::new(),
            blocked_skills: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}
