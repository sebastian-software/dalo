//! Materialization planning and symlink reconciliation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::resolver::{
    BlockedSkill, ClosureBlockReason, Resolution, ResolutionDiagnostic, ResolutionDiagnosticCode,
    ResolvedSkill, closure_block_reason_name,
};
use crate::store::{self, OwnedSkillState, StateFile, StorePaths};

/// Enabled source whose live scan was degraded during sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DegradedSource {
    /// Source ID.
    pub id: String,
    /// Source root path.
    pub path: PathBuf,
    /// Why this source could not be treated as a complete inventory.
    pub reason: String,
}

/// Sync and materialization report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    /// Store root.
    pub store: PathBuf,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
    /// Number of enabled logical targets available for materialization.
    pub linked_targets: usize,
    /// Planned and applied operations.
    pub operations: Vec<MaterializeOperation>,
    /// Resolution used to build this plan, including pending and blocked skills.
    pub resolution: Resolution,
    /// Sources whose scan was incomplete, so stale links were preserved.
    pub degraded_sources: Vec<DegradedSource>,
}

/// In-memory rollback data for a materialization pass that has not yet committed
/// its companion user lock.
#[derive(Debug)]
pub struct MaterializationRollback {
    state: StateFile,
    links: Vec<LinkSnapshot>,
}

#[derive(Debug)]
struct LinkSnapshot {
    path: PathBuf,
    target: Option<PathBuf>,
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
    /// Protected unmanaged entry intentionally kept at the target slot.
    Keep,
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
            Self::Keep => "keep",
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
    let (report, _) =
        materialize_with_degraded_sources_rollback(paths, resolution, dry_run, degraded_sources)?;
    Ok(report)
}

/// Materialize while retaining rollback data until the caller commits related
/// durable artifacts such as the user lock.
pub fn materialize_with_degraded_sources_rollback(
    paths: &StorePaths,
    resolution: &Resolution,
    dry_run: bool,
    degraded_sources: &[DegradedSource],
) -> DaloResult<(SyncReport, Option<MaterializationRollback>)> {
    let mut state = store::read_state(paths)?;
    let mut resolution = resolution.clone();
    let all_skills = resolution.active_skills.clone();
    let all_links = desired_links(&state, &resolution);
    let mut suppressed_links = BTreeSet::new();
    let mut protected_suppressed_links = BTreeSet::new();
    let mut links = all_links.clone();
    let mut operations = build_plan(paths, &state, &links, degraded_sources)?;
    while block_link_time_dependents(
        &mut resolution,
        &all_skills,
        &all_links,
        &operations,
        &mut suppressed_links,
        &mut protected_suppressed_links,
    ) {
        links = desired_links(&state, &resolution)
            .into_iter()
            .filter(|link| !suppressed_links.contains(&link.link_path))
            .collect();
        operations = build_plan(paths, &state, &links, degraded_sources)?;
    }
    let rollback = if dry_run {
        None
    } else {
        Some(MaterializationRollback {
            state: state.clone(),
            links: snapshot_links(&operations)?,
        })
    };

    if dry_run {
        for operation in &mut operations {
            if operation.status != MaterializeOperationStatus::Blocked
                && operation.status != MaterializeOperationStatus::Existing
            {
                operation.status = MaterializeOperationStatus::Planned;
            }
        }
    } else {
        loop {
            if let Err(error) = apply_plan(paths, &mut state, &mut operations, &links) {
                return Err(rollback_materialization_failure(paths, rollback, error));
            }

            if !block_link_time_dependents(
                &mut resolution,
                &all_skills,
                &all_links,
                &operations,
                &mut suppressed_links,
                &mut protected_suppressed_links,
            ) {
                break;
            }
            links = desired_links(&state, &resolution)
                .into_iter()
                .filter(|link| !suppressed_links.contains(&link.link_path))
                .collect();
            operations = match build_plan(paths, &state, &links, degraded_sources) {
                Ok(operations) => operations,
                Err(error) => {
                    if let Some(rollback) = rollback
                        && let Err(rollback_error) = rollback.restore(paths)
                    {
                        return Err(std::io::Error::other(format!(
                            "{error}; additionally failed to roll back sync: {rollback_error}"
                        ))
                        .into());
                    }
                    return Err(error);
                }
            };
        }
        if let Err(error) = store::write_state(paths, &state) {
            if let Some(rollback) = rollback
                && let Err(rollback_error) = rollback.restore(paths)
            {
                return Err(std::io::Error::other(format!(
                    "{error}; additionally failed to roll back sync: {rollback_error}"
                ))
                .into());
            }
            return Err(error);
        }
    }

    append_protected_closure_operations(&mut operations, &all_links, &protected_suppressed_links);

    Ok((
        SyncReport {
            store: paths.root.clone(),
            dry_run,
            linked_targets: state.targets.iter().filter(|target| target.enabled).count(),
            operations,
            resolution,
            degraded_sources: degraded_sources.to_vec(),
        },
        rollback,
    ))
}

impl MaterializationRollback {
    /// Restore owned links and state after a later command-level commit fails.
    pub fn restore(self, paths: &StorePaths) -> DaloResult<()> {
        for snapshot in self.links {
            match fs::symlink_metadata(&snapshot.path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    fs::remove_file(&snapshot.path)?
                }
                Ok(_) => {
                    return Err(std::io::Error::other(format!(
                        "cannot roll back {}; it was replaced with a non-symlink entry",
                        snapshot.path.display()
                    ))
                    .into());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            if let Some(target) = snapshot.target {
                if let Some(parent) = snapshot.path.parent() {
                    fs::create_dir_all(parent)?;
                }
                unix_fs::symlink(target, snapshot.path)?;
            }
        }
        store::write_state(paths, &self.state)
    }
}

fn snapshot_links(operations: &[MaterializeOperation]) -> DaloResult<Vec<LinkSnapshot>> {
    let mut snapshots = Vec::new();
    let mut seen = BTreeSet::new();
    for operation in operations.iter().filter(|operation| {
        matches!(
            operation.kind,
            MaterializeOperationKind::Create
                | MaterializeOperationKind::Relink
                | MaterializeOperationKind::Remove
                | MaterializeOperationKind::NoOp
        ) && operation.status != MaterializeOperationStatus::Blocked
    }) {
        if !seen.insert(operation.link_path.clone()) {
            continue;
        }
        let target = match fs::symlink_metadata(&operation.link_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Some(fs::read_link(&operation.link_path)?)
            }
            Ok(_) => {
                return Err(std::io::Error::other(format!(
                    "cannot snapshot non-symlink materialization path {}",
                    operation.link_path.display()
                ))
                .into());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        snapshots.push(LinkSnapshot {
            path: operation.link_path.clone(),
            target,
        });
    }
    Ok(snapshots)
}

fn block_link_time_dependents(
    resolution: &mut Resolution,
    all_skills: &[ResolvedSkill],
    all_links: &[DesiredLink],
    operations: &[MaterializeOperation],
    suppressed_links: &mut BTreeSet<PathBuf>,
    protected_suppressed_links: &mut BTreeSet<PathBuf>,
) -> bool {
    let direct_blocked = operations
        .iter()
        .filter(|operation| operation.status == MaterializeOperationStatus::Blocked)
        .filter_map(|operation| {
            all_links
                .iter()
                .find(|desired| desired.link_path == operation.link_path)
        })
        .filter_map(|desired| {
            all_skills
                .iter()
                .find(|skill| {
                    skill.slot_name == desired.slot_name && skill.path == desired.store_path
                })
                .map(|skill| (link_scope(desired), skill.source_ref.clone()))
        })
        .collect::<BTreeSet<_>>();
    let direct_protected = operations
        .iter()
        .filter(|operation| operation.kind == MaterializeOperationKind::Keep)
        .filter_map(|operation| {
            all_links
                .iter()
                .find(|desired| desired.link_path == operation.link_path)
        })
        .filter_map(|desired| {
            all_skills
                .iter()
                .find(|skill| {
                    skill.slot_name == desired.slot_name && skill.path == desired.store_path
                })
                .map(|skill| (link_scope(desired), skill.source_ref.clone()))
        })
        .collect::<BTreeSet<_>>();
    let direct_unavailable = direct_blocked
        .union(&direct_protected)
        .cloned()
        .collect::<BTreeSet<_>>();

    if direct_unavailable.is_empty() {
        return false;
    }

    let mut unavailable = direct_unavailable;
    let mut protected_unavailable = direct_protected;
    for desired in all_links
        .iter()
        .filter(|desired| suppressed_links.contains(&desired.link_path))
    {
        if let Some(skill) = all_skills
            .iter()
            .find(|skill| skill.slot_name == desired.slot_name && skill.path == desired.store_path)
        {
            let key = (link_scope(desired), skill.source_ref.clone());
            unavailable.insert(key.clone());
            if protected_suppressed_links.contains(&desired.link_path) {
                protected_unavailable.insert(key);
            }
        }
    }

    let mut block_causes = BTreeMap::new();
    let mut links_changed = false;
    loop {
        let mut pass_changed = false;
        for desired in all_links {
            if suppressed_links.contains(&desired.link_path) {
                continue;
            }
            let Some(dependent) = all_skills.iter().find(|skill| {
                skill.slot_name == desired.slot_name && skill.path == desired.store_path
            }) else {
                continue;
            };
            let scope = link_scope(desired);
            let Some((requirement, required_ref, reason, protected_chain)) =
                dependent.requires.iter().find_map(|requirement| {
                    let required = all_skills.iter().find(|candidate| {
                        candidate.source_id == dependent.source_id
                            && requirement_matches_skill(requirement, candidate)
                    })?;
                    let required_key = (scope.clone(), required.source_ref.clone());
                    if dependent.source_ref == required.source_ref
                        || !unavailable.contains(&required_key)
                    {
                        return None;
                    }
                    let reason = if direct_blocked.contains(&required_key) {
                        ClosureBlockReason::SameNameBlocked
                    } else {
                        ClosureBlockReason::Unlinked
                    };
                    let protected_chain = protected_unavailable.contains(&required_key);
                    Some((
                        requirement.clone(),
                        required.source_ref.clone(),
                        reason,
                        protected_chain,
                    ))
                })
            else {
                continue;
            };

            suppressed_links.insert(desired.link_path.clone());
            let dependent_key = (scope.clone(), dependent.source_ref.clone());
            unavailable.insert(dependent_key.clone());
            if protected_chain {
                protected_unavailable.insert(dependent_key);
                protected_suppressed_links.insert(desired.link_path.clone());
            }
            block_causes.insert(
                (scope, dependent.source_ref.clone()),
                (requirement, required_ref, reason, protected_chain),
            );
            pass_changed = true;
            links_changed = true;
        }
        if !pass_changed {
            break;
        }
    }

    let mut new_blocks = Vec::new();
    let mut index = 0;
    while index < resolution.active_skills.len() {
        let skill = &resolution.active_skills[index];
        let skill_links = all_links
            .iter()
            .filter(|desired| {
                desired.slot_name == skill.slot_name && desired.store_path == skill.path
            })
            .collect::<Vec<_>>();
        if skill_links.is_empty()
            || !skill_links
                .iter()
                .all(|desired| suppressed_links.contains(&desired.link_path))
        {
            index += 1;
            continue;
        }
        let cause = skill_links
            .iter()
            .filter_map(|desired| {
                block_causes.get(&(link_scope(desired), skill.source_ref.clone()))
            })
            .find(|(_, _, _, protected_chain)| !protected_chain);
        let Some((requirement, _, reason, _)) = cause.cloned() else {
            index += 1;
            continue;
        };
        let skill = resolution.active_skills.remove(index);
        new_blocks.push(BlockedSkill {
            skill,
            requirement,
            reason,
        });
    }

    if new_blocks.is_empty() {
        return links_changed;
    }

    for block in new_blocks {
        resolution.diagnostics.push(ResolutionDiagnostic {
            code: ResolutionDiagnosticCode::RequiredBlocked,
            message: format!(
                "skill `{}` is blocked: requirement `{}` is {}",
                block.skill.source_ref,
                block.requirement,
                closure_block_reason_name(block.reason)
            ),
            source_ref: Some(block.skill.source_ref.clone()),
        });
        resolution.blocked_skills.push(block);
    }
    resolution
        .blocked_skills
        .sort_by(|left, right| left.skill.source_ref.cmp(&right.skill.source_ref));
    resolution.diagnostics.sort_by(|left, right| {
        left.source_ref
            .cmp(&right.source_ref)
            .then_with(|| left.message.cmp(&right.message))
    });
    true
}

fn append_protected_closure_operations(
    operations: &mut Vec<MaterializeOperation>,
    all_links: &[DesiredLink],
    protected_suppressed_links: &BTreeSet<PathBuf>,
) {
    let existing_paths = operations
        .iter()
        .map(|operation| operation.link_path.clone())
        .collect::<BTreeSet<_>>();
    for desired in all_links.iter().filter(|desired| {
        protected_suppressed_links.contains(&desired.link_path)
            && !existing_paths.contains(&desired.link_path)
    }) {
        operations.push(MaterializeOperation {
            kind: MaterializeOperationKind::Keep,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Existing,
            reason: Some("required closure kept because a required slot is protected".to_owned()),
        });
    }
    operations.sort_by(|left, right| left.link_path.cmp(&right.link_path));
}

fn link_scope(desired: &DesiredLink) -> PathBuf {
    desired
        .link_path
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

fn requirement_matches_skill(requirement: &str, skill: &ResolvedSkill) -> bool {
    skill.slot_name == requirement
        || skill.source_ref == requirement
        || skill.id.as_deref() == Some(requirement)
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
                operations.push(plan_desired_recorded(
                    paths,
                    desired,
                    desired_is_protected(state, desired),
                )?);
            }
            (Some(desired), None) => {
                operations.push(plan_desired_unrecorded(
                    paths,
                    desired,
                    desired_is_protected(state, desired),
                )?);
            }
            (None, Some(recorded)) => {
                operations.push(plan_undesired_recorded(paths, recorded, degraded_sources)?);
            }
            (None, None) => {}
        }
    }

    Ok(operations)
}

fn plan_desired_recorded(
    paths: &StorePaths,
    desired: &DesiredLink,
    protected: bool,
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
        ActualLinkState::RealEntry if protected => MaterializeOperation {
            kind: MaterializeOperationKind::Keep,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Existing,
            reason: Some("protected unmanaged entry kept".to_owned()),
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
    protected: bool,
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
        ActualLinkState::RealEntry if protected => MaterializeOperation {
            kind: MaterializeOperationKind::Keep,
            link_path: desired.link_path.clone(),
            desired_path: Some(desired.store_path.clone()),
            status: MaterializeOperationStatus::Existing,
            reason: Some("protected unmanaged entry kept".to_owned()),
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
) -> DaloResult<MaterializeOperation> {
    if let Some(source) = degraded_sources
        .iter()
        .find(|source| recorded.store_path.starts_with(&source.path))
    {
        return Ok(MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: recorded.link_path.clone(),
            desired_path: Some(recorded.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some(format!(
                "source `{}` scan degraded; preserving recorded owned link",
                source.id
            )),
        });
    }

    let operation = match actual_link_state(&recorded.link_path) {
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
        Ok(ActualLinkState::RealEntry) => MaterializeOperation {
            kind: MaterializeOperationKind::DropRecord,
            link_path: recorded.link_path.clone(),
            desired_path: None,
            status: MaterializeOperationStatus::Applied,
            reason: Some("recorded slot is no longer owned".to_owned()),
        },
        Err(error) => MaterializeOperation {
            kind: MaterializeOperationKind::Conflict,
            link_path: recorded.link_path.clone(),
            desired_path: Some(recorded.store_path.clone()),
            status: MaterializeOperationStatus::Blocked,
            reason: Some(format!(
                "could not inspect recorded slot `{}`: {error}",
                recorded.link_path.display()
            )),
        },
    };
    Ok(operation)
}

fn apply_plan(
    paths: &StorePaths,
    state: &mut StateFile,
    operations: &mut [MaterializeOperation],
    desired_links: &[DesiredLink],
) -> DaloResult<()> {
    let previous_owned_skills = state.owned_skills.clone();

    for operation in operations.iter_mut() {
        let kind = operation.kind;
        if let Err(error) = apply_operation(paths, operation) {
            return Err(std::io::Error::other(format!(
                "failed to apply {} operation at `{}`: {error}",
                kind.as_str(),
                operation.link_path.display()
            ))
            .into());
        }
    }

    state.owned_skills = desired_links
        .iter()
        .filter(|desired| {
            !operations.iter().any(|operation| {
                operation.link_path == desired.link_path
                    && matches!(
                        operation.kind,
                        MaterializeOperationKind::Conflict | MaterializeOperationKind::Keep
                    )
            })
        })
        .map(|desired| OwnedSkillState {
            target_id: desired.target_id.clone(),
            slot_name: desired.slot_name.clone(),
            link_path: desired.link_path.clone(),
            store_path: desired.store_path.clone(),
            extra: previous_owned_skills
                .iter()
                .find(|record| record.link_path == desired.link_path)
                .map(|record| record.extra.clone())
                .unwrap_or_default(),
        })
        .collect();
    for operation in operations
        .iter()
        .filter(|operation| should_preserve_recorded_operation(operation))
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

fn apply_operation(paths: &StorePaths, operation: &mut MaterializeOperation) -> DaloResult<()> {
    match operation.kind {
        MaterializeOperationKind::Create | MaterializeOperationKind::Relink => {
            if operation.status == MaterializeOperationStatus::Blocked {
                return Ok(());
            }
            match fs::symlink_metadata(&operation.link_path) {
                // Only ever unlink a symlink we are about to replace.
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let Some(desired_path) = operation.desired_path.as_ref() else {
                        return Ok(());
                    };
                    let target = fs::read_link(&operation.link_path)?;
                    if !is_owned_link_target(paths, &operation.link_path, desired_path, &target) {
                        operation.kind = MaterializeOperationKind::Conflict;
                        operation.status = MaterializeOperationStatus::Blocked;
                        operation.reason =
                            Some("foreign symlink appeared at target slot".to_owned());
                        return Ok(());
                    }
                    fs::remove_file(&operation.link_path)?;
                }
                // A real file/dir appeared at the slot after planning (TOCTOU).
                // Refuse to delete unmanaged content; skip rather than overwrite.
                Ok(_) => {
                    operation.kind = MaterializeOperationKind::Conflict;
                    operation.status = MaterializeOperationStatus::Blocked;
                    operation.reason =
                        Some("real unmanaged entry appeared at target slot".to_owned());
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            if let Some(parent) = operation.link_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let Some(desired_path) = operation.desired_path.as_ref() else {
                return Ok(());
            };
            unix_fs::symlink(desired_path, &operation.link_path)?;
        }
        MaterializeOperationKind::Remove => {
            if let Some(recorded_store_path) = operation.desired_path.as_ref()
                && let ActualLinkState::Symlink(target) = actual_link_state(&operation.link_path)?
                && is_owned_link_target(paths, &operation.link_path, recorded_store_path, &target)
            {
                fs::remove_file(&operation.link_path)?;
            }
        }
        MaterializeOperationKind::DropRecord
        | MaterializeOperationKind::Conflict
        | MaterializeOperationKind::Keep
        | MaterializeOperationKind::NoOp => {}
    }
    Ok(())
}

fn should_preserve_recorded_operation(operation: &MaterializeOperation) -> bool {
    is_degraded_source_preserve(operation)
        || operation.reason.as_deref().is_some_and(|reason| {
            operation.kind == MaterializeOperationKind::Conflict
                && operation.status == MaterializeOperationStatus::Blocked
                && reason.starts_with("could not inspect recorded slot")
        })
}

fn desired_is_protected(state: &StateFile, desired: &DesiredLink) -> bool {
    let Some(scope) = desired.link_path.parent() else {
        return false;
    };
    let Some(dir) = state
        .materialization_dirs
        .iter()
        .find(|dir| dir.path == scope)
    else {
        return false;
    };
    state.protected_skills.iter().any(|protected| {
        protected.slot_name == desired.slot_name
            && (dir.logical_targets.contains(&protected.target_id)
                || protected.path.as_ref() == Some(&desired.link_path))
    })
}

fn rollback_materialization_failure(
    paths: &StorePaths,
    rollback: Option<MaterializationRollback>,
    error: DaloError,
) -> DaloError {
    let Some(rollback) = rollback else {
        return error;
    };
    match rollback.restore(paths) {
        Ok(()) => std::io::Error::other(format!("{error}; changes were rolled back")).into(),
        Err(rollback_error) => std::io::Error::other(format!(
            "{error}; additionally failed to roll back sync: {rollback_error}"
        ))
        .into(),
    }
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
    use std::os::unix::fs::PermissionsExt;

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
    fn materialize_should_block_dependent_when_requirement_has_target_conflict() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(target_dir.join("beta")).expect("unmanaged dir should be created");
        let alpha_dir = store_root.join("team/skills/alpha");
        let beta_dir = store_root.join("team/skills/beta");
        fs::create_dir_all(&alpha_dir).expect("alpha should be created");
        fs::create_dir_all(&beta_dir).expect("beta should be created");
        write_state_with_target(&store_root, &target_dir);

        let report = materialize(
            &StorePaths::new(store_root),
            &resolution_with_required_pair(&alpha_dir, &beta_dir),
            false,
        )
        .expect("materialize should succeed");

        assert!(!target_dir.join("alpha").exists());
        assert!(target_dir.join("beta").is_dir());
        assert_eq!(report.resolution.active_skills.len(), 1);
        assert_eq!(
            report.resolution.active_skills[0].source_ref,
            "company:beta"
        );
        assert_eq!(report.resolution.blocked_skills.len(), 1);
        assert_eq!(
            report.resolution.blocked_skills[0].skill.source_ref,
            "company:alpha"
        );
        assert_eq!(
            report.resolution.blocked_skills[0].reason,
            ClosureBlockReason::SameNameBlocked
        );
    }

    #[test]
    fn materialize_should_remove_existing_dependent_when_requirement_becomes_blocked() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let alpha_dir = store_root.join("team/skills/alpha");
        let beta_dir = store_root.join("team/skills/beta");
        fs::create_dir_all(&alpha_dir).expect("alpha should be created");
        fs::create_dir_all(&beta_dir).expect("beta should be created");
        write_state_with_target(&store_root, &target_dir);
        let paths = StorePaths::new(store_root);
        let resolution = resolution_with_required_pair(&alpha_dir, &beta_dir);
        materialize(&paths, &resolution, false).expect("first materialize should succeed");
        fs::remove_file(target_dir.join("beta")).expect("beta link should be removed");
        fs::create_dir(target_dir.join("beta")).expect("unmanaged beta should be created");

        let report =
            materialize(&paths, &resolution, false).expect("second materialize should succeed");

        assert!(fs::symlink_metadata(target_dir.join("alpha")).is_err());
        assert!(report.operations.iter().any(|operation| {
            operation.link_path == target_dir.join("alpha")
                && operation.kind == MaterializeOperationKind::Remove
        }));
        assert_eq!(report.resolution.blocked_skills.len(), 1);
        let state = store::read_state(&paths).expect("state should be readable");
        assert!(state.owned_skills.is_empty());
    }

    #[test]
    fn materialize_should_keep_dependent_in_unaffected_target() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let blocked_target = temp_dir.path().join("blocked-target");
        let clean_target = temp_dir.path().join("clean-target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(blocked_target.join("beta")).expect("unmanaged dir should be created");
        fs::create_dir_all(&clean_target).expect("clean target should be created");
        let alpha_dir = store_root.join("team/skills/alpha");
        let beta_dir = store_root.join("team/skills/beta");
        fs::create_dir_all(&alpha_dir).expect("alpha should be created");
        fs::create_dir_all(&beta_dir).expect("beta should be created");
        let paths = StorePaths::new(store_root);
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.targets = vec![
            TargetState {
                id: "blocked".to_owned(),
                path: blocked_target.clone(),
                canonical_path: blocked_target.clone(),
                enabled: true,
                extra: Default::default(),
            },
            TargetState {
                id: "clean".to_owned(),
                path: clean_target.clone(),
                canonical_path: clean_target.clone(),
                enabled: true,
                extra: Default::default(),
            },
        ];
        state.materialization_dirs = vec![
            MaterializationDirState {
                path: blocked_target.clone(),
                logical_targets: vec!["blocked".to_owned()],
                extra: Default::default(),
            },
            MaterializationDirState {
                path: clean_target.clone(),
                logical_targets: vec!["clean".to_owned()],
                extra: Default::default(),
            },
        ];
        store::write_state(&paths, &state).expect("state should be written");

        let report = materialize(
            &paths,
            &resolution_with_required_pair(&alpha_dir, &beta_dir),
            false,
        )
        .expect("materialize should succeed");

        assert!(!blocked_target.join("alpha").exists());
        assert!(blocked_target.join("beta").is_dir());
        assert!(
            fs::symlink_metadata(clean_target.join("alpha"))
                .expect("clean alpha link should exist")
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(clean_target.join("beta"))
                .expect("clean beta link should exist")
                .file_type()
                .is_symlink()
        );
        assert_eq!(report.resolution.active_skills.len(), 2);
        assert!(report.resolution.blocked_skills.is_empty());
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
    fn materialize_rollback_should_restore_links_and_state() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        write_state_with_target(&store_root, &target_dir);
        let paths = StorePaths::new(store_root);

        let (_, rollback) = materialize_with_degraded_sources_rollback(
            &paths,
            &resolution_with_skill("review", &skill_dir),
            false,
            &[],
        )
        .expect("materialize should succeed");
        rollback
            .expect("non-dry-run materialization should be reversible")
            .restore(&paths)
            .expect("rollback should succeed");

        assert!(!target_dir.join("review").exists());
        assert!(
            store::read_state(&paths)
                .expect("state should be readable")
                .owned_skills
                .is_empty()
        );
    }

    #[test]
    fn materialize_should_roll_back_links_when_apply_fails_midway() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let first_target = temp_dir.path().join("a-target");
        let failing_target = temp_dir.path().join("z-target");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&first_target).expect("first target should be created");
        fs::create_dir_all(&failing_target).expect("failing target should be created");
        fs::set_permissions(&failing_target, fs::Permissions::from_mode(0o555))
            .expect("failing target should be read-only");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill should be created");
        let paths = StorePaths::new(store_root);
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.targets = vec![
            TargetState {
                id: "first".to_owned(),
                path: first_target.clone(),
                canonical_path: first_target.clone(),
                enabled: true,
                extra: Default::default(),
            },
            TargetState {
                id: "failing".to_owned(),
                path: failing_target.clone(),
                canonical_path: failing_target.clone(),
                enabled: true,
                extra: Default::default(),
            },
        ];
        state.materialization_dirs = vec![
            MaterializationDirState {
                path: first_target.clone(),
                logical_targets: vec!["first".to_owned()],
                extra: Default::default(),
            },
            MaterializationDirState {
                path: failing_target.clone(),
                logical_targets: vec!["failing".to_owned()],
                extra: Default::default(),
            },
        ];
        store::write_state(&paths, &state).expect("state should be written");

        let error = materialize(&paths, &resolution_with_skill("review", &skill_dir), false)
            .expect_err("second target should fail during apply");

        assert!(
            error
                .to_string()
                .contains(&failing_target.join("review").display().to_string()),
            "{error}"
        );
        assert!(
            error.to_string().contains("changes were rolled back"),
            "{error}"
        );
        fs::set_permissions(&failing_target, fs::Permissions::from_mode(0o755))
            .expect("failing target permissions should be restored");
        assert!(!first_target.join("review").exists());
        assert!(
            store::read_state(&paths)
                .expect("state should be readable")
                .owned_skills
                .is_empty()
        );
    }

    #[test]
    fn snapshot_links_should_fail_before_apply_when_a_path_cannot_be_inspected() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let parent = temp_dir.path().join("not-a-directory");
        fs::write(&parent, "file").expect("parent file should be written");
        let operations = vec![MaterializeOperation {
            kind: MaterializeOperationKind::Create,
            link_path: parent.join("review"),
            desired_path: Some(PathBuf::from("/store/review")),
            status: MaterializeOperationStatus::Applied,
            reason: None,
        }];

        let error = snapshot_links(&operations)
            .expect_err("an unreadable rollback path must prevent apply");

        assert!(!error.to_string().is_empty());
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
    fn materialize_should_preserve_record_on_metadata_errors() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_file = temp_dir.path().join("target-file");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::write(&target_file, "not a directory").expect("target file should be written");
        let link_path = target_file.join("review");
        let recorded_store_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&recorded_store_dir).expect("recorded store dir should be created");
        write_state_with_owned_skill(&store_root, &target_file, &link_path, &recorded_store_dir);

        let report = materialize(
            &StorePaths::new(store_root.clone()),
            &empty_resolution(),
            false,
        )
        .expect("materialize should succeed");

        assert_eq!(
            report.operations[0].kind,
            MaterializeOperationKind::Conflict
        );
        assert_eq!(
            report.operations[0].status,
            MaterializeOperationStatus::Blocked
        );
        assert!(
            report.operations[0]
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("could not inspect recorded slot"))
        );
        let state =
            store::read_state(&StorePaths::new(store_root)).expect("state should be readable");
        assert_eq!(state.owned_skills.len(), 1);
        assert_eq!(state.owned_skills[0].link_path, link_path);
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
            extra: Default::default(),
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
    fn apply_plan_should_recheck_relink_target_before_unlinking() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target_dir = temp_dir.path().join("target");
        let foreign_dir = temp_dir.path().join("foreign");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target_dir).expect("target should be created");
        fs::create_dir_all(&foreign_dir).expect("foreign dir should be created");
        let desired_store_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&desired_store_dir).expect("desired store dir should be created");
        let link_path = target_dir.join("review");
        unix_fs::symlink(&foreign_dir, &link_path).expect("foreign symlink should be created");
        let mut state = StateFile::empty();
        let operation = MaterializeOperation {
            kind: MaterializeOperationKind::Relink,
            link_path: link_path.clone(),
            desired_path: Some(desired_store_dir),
            status: MaterializeOperationStatus::Applied,
            reason: Some("owned symlink points elsewhere".to_owned()),
        };
        let mut operations = vec![operation];

        apply_plan(
            &StorePaths::new(store_root),
            &mut state,
            &mut operations,
            &[],
        )
        .expect("apply should succeed");

        assert_eq!(operations[0].kind, MaterializeOperationKind::Conflict);
        assert_eq!(operations[0].status, MaterializeOperationStatus::Blocked);
        assert_eq!(
            fs::read_link(&link_path).expect("foreign symlink should survive"),
            foreign_dir
        );
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
                    extra: Default::default(),
                },
                TargetState {
                    id: "codex".to_owned(),
                    path: shared.clone(),
                    canonical_path: shared.clone(),
                    enabled: true,
                    extra: Default::default(),
                },
            ],
            materialization_dirs: vec![MaterializationDirState {
                path: shared,
                logical_targets: vec!["codex".to_owned(), "openclaw".to_owned()],
                extra: Default::default(),
            }],
            owned_skills: Vec::new(),
            protected_skills: Vec::new(),
            extra: Default::default(),
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
            extra: Default::default(),
        }];
        state.materialization_dirs = vec![MaterializationDirState {
            path: target_dir.to_path_buf(),
            logical_targets: vec!["generic".to_owned()],
            extra: Default::default(),
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
            extra: Default::default(),
        }];
        state.materialization_dirs = vec![MaterializationDirState {
            path: target_dir.to_path_buf(),
            logical_targets: vec!["generic".to_owned()],
            extra: Default::default(),
        }];
        state.owned_skills = vec![OwnedSkillState {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link_path.to_path_buf(),
            store_path: store_path.to_path_buf(),
            extra: Default::default(),
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
                requires: Vec::new(),
            }],
            pending_approval_skills: Vec::new(),
            unlinked_skills: Vec::new(),
            blocked_skills: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn resolution_with_required_pair(alpha_path: &Path, beta_path: &Path) -> Resolution {
        Resolution {
            active_skills: vec![
                ResolvedSkill {
                    source_ref: "company:alpha".to_owned(),
                    slot_name: "alpha".to_owned(),
                    id: None,
                    source_id: "company".to_owned(),
                    source_kind: SourceKind::Team,
                    source_priority: 10,
                    path: alpha_path.to_path_buf(),
                    local_override: false,
                    requires: vec!["beta".to_owned()],
                },
                ResolvedSkill {
                    source_ref: "company:beta".to_owned(),
                    slot_name: "beta".to_owned(),
                    id: None,
                    source_id: "company".to_owned(),
                    source_kind: SourceKind::Team,
                    source_priority: 10,
                    path: beta_path.to_path_buf(),
                    local_override: false,
                    requires: Vec::new(),
                },
            ],
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
