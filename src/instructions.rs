//! Instruction packs rendered into managed blocks of agent instruction files.
//!
//! A managed block is delimited by paired markers
//! `<!-- dalo:start <pack-id> -->` and `<!-- dalo:end <pack-id> -->`. Only the
//! bytes between a pack's markers are ever rewritten; everything outside any
//! managed block is preserved.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Component;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(test)]
use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};
use crate::git;
use crate::lockfile::LockedInstructionPack;
use crate::source::{SourceConfig, SourceKind};
use crate::store::{self, ApprovalRecord, StorePaths};

const START_MARKER_PREFIX: &str = "<!-- dalo:start ";
const END_MARKER_PREFIX: &str = "<!-- dalo:end ";

/// A versioned instruction pack: standing agent-facing conventions as Markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionPack {
    /// Pack ID.
    pub id: String,
    /// Declared version, when present in frontmatter.
    pub version: Option<String>,
    /// Rendered Markdown body.
    pub body: String,
}

/// Report from enabling or disabling an instruction pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionPackReport {
    /// Pack ID.
    pub pack_id: String,
    /// Source that owns the pack.
    pub source_id: String,
    /// Instruction-file target affected.
    pub target: PathBuf,
    /// What happened: `enabled`, `disabled`, or `unchanged`.
    pub action: String,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
    /// Non-fatal recovery detail, such as a malformed block left untouched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Report from enabling or disabling one pack across logical agent targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionPackBatchReport {
    /// Pack ID.
    pub pack_id: String,
    /// Source that owns the pack.
    pub source_id: String,
    /// De-duplicated physical destination operations.
    pub operations: Vec<InstructionTargetOperationReport>,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// One de-duplicated instruction-file operation in a batch report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionTargetOperationReport {
    /// Logical target IDs mapped to this physical destination.
    pub logical_targets: Vec<String>,
    /// Effective physical instruction-file path.
    pub target: PathBuf,
    /// What happened: `enabled`, `disabled`, or `unchanged`.
    pub action: String,
    /// Non-fatal recovery detail, such as a malformed block left untouched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// One source-backed instruction update performed as part of `dalo sync`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionSyncOperation {
    /// Source that owns the already-active pack.
    pub source_id: String,
    /// Pack ID.
    pub pack_id: String,
    /// Effective physical instruction-file path.
    pub target: PathBuf,
    /// What happened: `refreshed` or `unchanged`.
    pub action: String,
    /// Previously rendered immutable source commit.
    pub previous_commit: String,
    /// Newly rendered immutable source commit.
    pub commit: String,
}

/// Result of reconciling already-active source-backed packs during sync.
#[derive(Debug)]
pub struct InstructionSyncResult {
    /// Updated lock entries, preserving local packs unchanged.
    pub active_instruction_packs: Vec<LockedInstructionPack>,
    /// Source revisions considered by this sync.
    pub operations: Vec<InstructionSyncOperation>,
    /// Target snapshots retained until the companion user lock is committed.
    pub rollback: Option<InstructionRefreshRollback>,
}

/// One active instruction pack removed with its owning source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionRemovalOperation {
    /// Source that owned the pack.
    pub source_id: String,
    /// Pack ID.
    pub pack_id: String,
    /// Effective physical instruction-file path.
    pub target: PathBuf,
    /// What happened: `removed` or `lock_removed`.
    pub action: String,
    /// Recovery detail when the managed block could not be removed safely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Result of removing active packs whose owning sources are being removed.
#[derive(Debug)]
pub struct InstructionSourceRemovalResult {
    /// Lock entries that remain active after removal.
    pub active_instruction_packs: Vec<LockedInstructionPack>,
    /// Pack removals for human and JSON reporting.
    pub operations: Vec<InstructionRemovalOperation>,
    /// Target snapshots retained until source-removal metadata commits.
    pub rollback: Option<InstructionRefreshRollback>,
}

/// In-memory rollback for instruction target writes awaiting lock commit.
#[derive(Debug)]
pub struct InstructionRefreshRollback {
    prepared: Vec<PreparedInstructionMutation>,
    written: Vec<(usize, TargetIdentity)>,
    _target_locks: Vec<TargetLock>,
}

impl InstructionRefreshRollback {
    /// Restore every instruction target written by the pending sync.
    pub fn restore(self) -> DaloResult<()> {
        let mut failures = Vec::new();
        for (index, identity) in self.written.iter().rev() {
            let mutation = &self.prepared[*index];
            let snapshot = TargetSnapshot {
                target: mutation.snapshot.target.clone(),
                content: mutation.snapshot.content.clone(),
                identity: mutation.snapshot.identity,
            };
            if let Err(error) = restore_target(
                snapshot,
                mutation
                    .rendered
                    .as_deref()
                    .expect("only rendered mutations are recorded as written"),
                *identity,
            ) {
                failures.push(error.to_string());
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(DaloError::Io(std::io::Error::other(failures.join("; "))))
        }
    }
}

/// Enable one pack across de-duplicated logical target destinations.
pub fn enable_pack_for_targets(
    paths: &StorePaths,
    selector: &str,
    destinations: &[crate::target::InstructionFileDestination],
    dry_run: bool,
) -> DaloResult<InstructionPackBatchReport> {
    if destinations.is_empty() {
        return Err(DaloError::InvalidArgument {
            reason: "at least one instruction target is required".to_owned(),
        });
    }
    let resolved = resolve_pack(paths, selector)?;
    let marker_id = marker_id(&resolved.source_id, &resolved.pack.id);
    let mut prepared = Vec::with_capacity(destinations.len());
    let mut target_locks = Vec::with_capacity(destinations.len());
    for destination in destinations {
        let target = normalize_target_path(&destination.path)?;
        target_locks.push(acquire_target_lock(paths, &target)?);
        let snapshot = target_snapshot(&target)?;
        let existing = snapshot.content.clone().unwrap_or_default();
        let rendered = render_block(&existing, &marker_id, &resolved.pack.body)?;
        prepared.push(PreparedInstructionMutation {
            logical_targets: destination.logical_targets.clone(),
            target,
            snapshot,
            rendered: Some(rendered),
            action: "enabled".to_owned(),
            warning: None,
        });
    }
    let mut lock = store::read_user_lock(paths)?;
    for mutation in &prepared {
        lock.active_instruction_packs.retain(|entry| {
            !(entry.source_id == resolved.source_id
                && entry.pack_id == resolved.pack.id
                && targets_match(entry, &mutation.target))
        });
        lock.active_instruction_packs.push(LockedInstructionPack {
            pack_id: resolved.pack.id.clone(),
            target: mutation.target.clone(),
            logical_targets: mutation.logical_targets.clone(),
            source_id: resolved.source_id.clone(),
            commit: resolved.commit.clone(),
            version: resolved.pack.version.clone(),
        });
    }
    sort_instruction_lock_entries(&mut lock.active_instruction_packs);
    if !dry_run {
        apply_prepared_instruction_mutations(paths, prepared.as_mut_slice(), &lock)?;
    }
    let operations = prepared
        .into_iter()
        .map(PreparedInstructionMutation::into_report)
        .collect();
    drop(target_locks);
    Ok(InstructionPackBatchReport {
        pack_id: resolved.pack.id,
        source_id: resolved.source_id,
        operations,
        dry_run,
    })
}

/// Disable one pack across de-duplicated logical target destinations.
pub fn disable_pack_for_targets(
    paths: &StorePaths,
    selector: &str,
    destinations: &[crate::target::InstructionFileDestination],
    dry_run: bool,
) -> DaloResult<InstructionPackBatchReport> {
    if destinations.is_empty() {
        return Err(DaloError::InvalidArgument {
            reason: "at least one instruction target is required".to_owned(),
        });
    }
    let (source_id, pack_id) = parse_pack_selector(selector)?;
    let marker_id = marker_id(&source_id, &pack_id);
    let mut lock = store::read_user_lock(paths)?;
    let mut prepared = Vec::with_capacity(destinations.len());
    let mut target_locks = Vec::with_capacity(destinations.len());
    for destination in destinations {
        let target = normalize_target_path(&destination.path)?;
        target_locks.push(acquire_target_lock(paths, &target)?);
        let snapshot = target_snapshot(&target)?;
        let existing = snapshot.content.clone().unwrap_or_default();
        let (block, malformed_error) = match find_block(&existing, &marker_id) {
            Ok(block) => (block, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let has_block = block.is_some();
        let has_lock_entry = lock.active_instruction_packs.iter().any(|entry| {
            entry.source_id == source_id
                && entry.pack_id == pack_id
                && targets_match(entry, &target)
        });
        let warning = malformed_error.map(|error| {
            let lock_action = if has_lock_entry {
                if dry_run {
                    "the lock entry would be removed"
                } else {
                    "the lock entry was removed"
                }
            } else {
                "no matching lock entry was found"
            };
            format!("{error}; target left untouched and {lock_action}")
        });
        let rendered = if warning.is_none() && has_block {
            Some(remove_block(&existing, &marker_id)?)
        } else {
            None
        };
        prepared.push(PreparedInstructionMutation {
            logical_targets: destination.logical_targets.clone(),
            target,
            snapshot,
            rendered,
            action: if has_block || has_lock_entry {
                "disabled".to_owned()
            } else {
                "unchanged".to_owned()
            },
            warning,
        });
    }
    let lock_changed = lock.active_instruction_packs.iter().any(|entry| {
        entry.source_id == source_id
            && entry.pack_id == pack_id
            && prepared
                .iter()
                .any(|mutation| targets_match(entry, &mutation.target))
    });
    lock.active_instruction_packs.retain(|entry| {
        !(entry.source_id == source_id
            && entry.pack_id == pack_id
            && prepared
                .iter()
                .any(|mutation| targets_match(entry, &mutation.target)))
    });
    if !dry_run {
        if lock_changed {
            apply_prepared_instruction_mutations(paths, prepared.as_mut_slice(), &lock)?;
        } else {
            apply_prepared_target_writes(prepared.as_mut_slice())?;
        }
    }
    let operations = prepared
        .into_iter()
        .map(PreparedInstructionMutation::into_report)
        .collect();
    drop(target_locks);
    Ok(InstructionPackBatchReport {
        pack_id,
        source_id,
        operations,
        dry_run,
    })
}

#[derive(Debug)]
struct PendingPackRefresh {
    entry_index: usize,
    previous_commit: String,
    commit: String,
    old_body: String,
    pack: InstructionPack,
}

/// Refresh only source-backed packs that are already active in the user lock.
///
/// Source approval is checked again on every sync. The previously rendered
/// managed block must still match its immutable source revision before Dalo
/// will replace it, so revoked trust or external block edits fail closed.
pub fn refresh_active_packs(
    paths: &StorePaths,
    sources: &[SourceConfig],
    approvals: &[ApprovalRecord],
    active: &[LockedInstructionPack],
    dry_run: bool,
) -> DaloResult<InstructionSyncResult> {
    let mut updated = active.to_vec();
    let mut grouped = BTreeMap::<PathBuf, Vec<PendingPackRefresh>>::new();

    for (entry_index, entry) in active.iter().enumerate() {
        if entry.source_id == "local" {
            continue;
        }
        let source = sources
            .iter()
            .find(|source| source.id == entry.source_id)
            .ok_or_else(|| {
                DaloError::StateError {
                    reason: format!(
                        "active instruction pack `{}` references missing source `{}`; restore the source or disable the pack",
                        pack_ref(&entry.source_id, &entry.pack_id),
                        entry.source_id
                    ),
                }
            })?;
        if !source.enabled {
            return Err(DaloError::StateError {
                reason: format!(
                    "active instruction pack `{}` references disabled source `{}`; enable the source or disable the pack",
                    pack_ref(&entry.source_id, &entry.pack_id),
                    entry.source_id
                ),
            });
        }
        ensure_instruction_source_approved(source, approvals, Some(&entry.pack_id))?;

        let previous_commit = entry.commit.clone().ok_or_else(|| DaloError::StateError {
            reason: format!(
                "active instruction pack `{}` has no immutable source provenance; review and re-enable the pack",
                pack_ref(&entry.source_id, &entry.pack_id)
            ),
        })?;
        let commit = validate_source_pack(paths, source, &entry.pack_id, None)?;
        let pack = read_pack_from_dir(&source.path.join("instructions"), &entry.pack_id)?;
        let relative_path = PathBuf::from("instructions").join(format!("{}.md", entry.pack_id));
        let old_body = if previous_commit == commit {
            pack.body.clone()
        } else {
            git::read_file_at_commit(&source.path, &previous_commit, &relative_path).map_err(
                |error| DaloError::StateError {
                    reason: format!(
                        "could not verify the previously rendered revision of `{}`: {error}; restore the source history or review and re-enable the pack",
                        pack_ref(&entry.source_id, &entry.pack_id)
                    ),
                },
            )?
        };
        let target = lock_entry_target_path(paths, &entry.target);
        grouped.entry(target).or_default().push(PendingPackRefresh {
            entry_index,
            previous_commit,
            commit,
            old_body,
            pack,
        });
    }

    let mut operations = Vec::new();
    let mut prepared = Vec::with_capacity(grouped.len());
    let mut target_locks = Vec::with_capacity(grouped.len());
    for (target, refreshes) in grouped {
        target_locks.push(acquire_target_lock(paths, &target)?);
        let snapshot = target_snapshot(&target)?;
        let existing = snapshot.content.clone().unwrap_or_default();
        let mut rendered = existing.clone();
        let line_ending = line_ending_for(&existing);

        for refresh in refreshes {
            let entry = &active[refresh.entry_index];
            let marker = lock_marker_id(entry);
            let expected =
                render_managed_block_with_line_ending(&marker, &refresh.old_body, line_ending)?;
            let Some((start, end)) = find_block(&existing, &marker)? else {
                return Err(DaloError::StateError {
                    reason: format!(
                        "managed block for `{}` is missing from `{}`; run `dalo status` and review or re-enable the pack",
                        pack_ref(&entry.source_id, &entry.pack_id),
                        target.display()
                    ),
                });
            };
            if existing[start..end] != expected {
                return Err(DaloError::StateError {
                    reason: format!(
                        "managed block for `{}` in `{}` changed outside Dalo; run `dalo status` and review or re-enable the pack",
                        pack_ref(&entry.source_id, &entry.pack_id),
                        target.display()
                    ),
                });
            }

            if refresh.previous_commit != refresh.commit {
                let next_block = render_managed_block_with_line_ending(
                    &marker,
                    &refresh.pack.body,
                    line_ending,
                )?;
                rendered = render_block(&rendered, &marker, &refresh.pack.body)?;
                updated[refresh.entry_index].commit = Some(refresh.commit.clone());
                updated[refresh.entry_index].version = refresh.pack.version.clone();
                operations.push(InstructionSyncOperation {
                    source_id: entry.source_id.clone(),
                    pack_id: entry.pack_id.clone(),
                    target: target.clone(),
                    action: if next_block == expected {
                        "unchanged".to_owned()
                    } else {
                        "refreshed".to_owned()
                    },
                    previous_commit: refresh.previous_commit,
                    commit: refresh.commit,
                });
            }
        }

        prepared.push(PreparedInstructionMutation {
            logical_targets: Vec::new(),
            target,
            snapshot,
            rendered: (rendered != existing).then_some(rendered),
            action: "refreshed".to_owned(),
            warning: None,
        });
    }
    sort_instruction_lock_entries(&mut updated);
    operations.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.source_id.cmp(&right.source_id))
            .then_with(|| left.pack_id.cmp(&right.pack_id))
    });

    let rollback = if dry_run {
        None
    } else {
        let written = apply_prepared_target_writes(prepared.as_mut_slice())?;
        Some(InstructionRefreshRollback {
            prepared,
            written,
            _target_locks: target_locks,
        })
    };
    Ok(InstructionSyncResult {
        active_instruction_packs: updated,
        operations,
        rollback,
    })
}

/// Remove active managed blocks and lock entries for sources being deleted.
///
/// Missing blocks drop their stale lock entries. Malformed blocks abort before
/// any write so the source and lock retain the ownership provenance needed for
/// recovery. Valid blocks across the same physical target are removed in one
/// conditional write.
pub fn remove_active_packs_for_sources(
    paths: &StorePaths,
    active: &[LockedInstructionPack],
    removed_source_ids: &BTreeSet<String>,
    dry_run: bool,
) -> DaloResult<InstructionSourceRemovalResult> {
    let mut retained = active
        .iter()
        .filter(|entry| !removed_source_ids.contains(&entry.source_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut grouped = BTreeMap::<PathBuf, Vec<&LockedInstructionPack>>::new();
    for entry in active
        .iter()
        .filter(|entry| removed_source_ids.contains(&entry.source_id))
    {
        grouped
            .entry(lock_entry_target_path(paths, &entry.target))
            .or_default()
            .push(entry);
    }

    let mut operations = Vec::new();
    let mut prepared = Vec::with_capacity(grouped.len());
    let mut target_locks = Vec::with_capacity(grouped.len());
    for (target, entries) in grouped {
        target_locks.push(acquire_target_lock(paths, &target)?);
        let snapshot = target_snapshot(&target)?;
        let existing = snapshot.content.clone().unwrap_or_default();
        let mut rendered = existing.clone();
        for entry in entries {
            let marker = lock_marker_id(entry);
            let (action, warning) = match find_block(&rendered, &marker) {
                Ok(Some(_)) => {
                    rendered = remove_block(&rendered, &marker)?;
                    ("removed".to_owned(), None)
                }
                Ok(None) => (
                    "lock_removed".to_owned(),
                    Some("managed block was already missing; removed its lock entry".to_owned()),
                ),
                Err(error) => return Err(error),
            };
            operations.push(InstructionRemovalOperation {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: target.clone(),
                action,
                warning,
            });
        }
        prepared.push(PreparedInstructionMutation {
            logical_targets: Vec::new(),
            target,
            snapshot,
            rendered: (rendered != existing).then_some(rendered),
            action: "removed".to_owned(),
            warning: None,
        });
    }
    sort_instruction_lock_entries(&mut retained);
    operations.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.source_id.cmp(&right.source_id))
            .then_with(|| left.pack_id.cmp(&right.pack_id))
    });
    let rollback = if dry_run {
        None
    } else {
        let written = apply_prepared_target_writes(prepared.as_mut_slice())?;
        Some(InstructionRefreshRollback {
            prepared,
            written,
            _target_locks: target_locks,
        })
    };
    Ok(InstructionSourceRemovalResult {
        active_instruction_packs: retained,
        operations,
        rollback,
    })
}

#[derive(Debug)]
struct PreparedInstructionMutation {
    logical_targets: Vec<String>,
    target: PathBuf,
    snapshot: TargetSnapshot,
    rendered: Option<String>,
    action: String,
    warning: Option<String>,
}

impl PreparedInstructionMutation {
    fn into_report(self) -> InstructionTargetOperationReport {
        InstructionTargetOperationReport {
            logical_targets: self.logical_targets,
            target: self.target,
            action: self.action,
            warning: self.warning,
        }
    }
}

fn apply_prepared_instruction_mutations(
    paths: &StorePaths,
    prepared: &mut [PreparedInstructionMutation],
    lock: &crate::lockfile::UserLock,
) -> DaloResult<()> {
    let written = apply_prepared_target_writes(prepared)?;
    if let Err(error) = store::write_user_lock(paths, lock) {
        return Err(rollback_prepared_target_writes(prepared, &written, error));
    }
    Ok(())
}

fn apply_prepared_target_writes(
    prepared: &mut [PreparedInstructionMutation],
) -> DaloResult<Vec<(usize, TargetIdentity)>> {
    let mut written = Vec::new();
    for (index, mutation) in prepared.iter().enumerate() {
        let Some(rendered) = mutation.rendered.as_deref() else {
            continue;
        };
        match write_target_if_unchanged(&mutation.snapshot, rendered) {
            Ok(identity) => written.push((index, identity)),
            Err(error) => {
                return Err(rollback_prepared_target_writes(prepared, &written, error));
            }
        }
    }
    Ok(written)
}

fn rollback_prepared_target_writes(
    prepared: &[PreparedInstructionMutation],
    written: &[(usize, TargetIdentity)],
    mut error: DaloError,
) -> DaloError {
    for (index, identity) in written.iter().rev() {
        let mutation = &prepared[*index];
        let snapshot = TargetSnapshot {
            target: mutation.snapshot.target.clone(),
            content: mutation.snapshot.content.clone(),
            identity: mutation.snapshot.identity,
        };
        error = restore_target_after_error(
            snapshot,
            mutation
                .rendered
                .as_deref()
                .expect("only rendered mutations are recorded as written"),
            *identity,
            error,
        );
    }
    error
}

/// Active instruction packs returned by `instructions list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionPackListReport {
    /// Packs currently rendered into instruction files.
    pub active_instruction_packs: Vec<LockedInstructionPack>,
}

/// Drift detected for an active instruction pack's rendered block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionBlockDrift {
    /// Source that owns the pack.
    pub source_id: String,
    /// Pack ID.
    pub pack_id: String,
    /// Instruction-file target that should contain the block.
    pub target: PathBuf,
    /// Drift kind.
    pub kind: InstructionBlockDriftKind,
    /// Human-readable detail.
    pub message: String,
}

/// Instruction block drift classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionBlockDriftKind {
    /// The expected managed block is absent.
    Missing,
    /// Markers are malformed, duplicated, or unreadable.
    Malformed,
    /// The block exists but no longer matches the current pack body.
    Stale,
    /// The active lock entry points to a pack that cannot be read.
    SourceMissing,
}

fn start_marker(pack_id: &str) -> String {
    format!("{START_MARKER_PREFIX}{pack_id} -->")
}

fn end_marker(pack_id: &str) -> String {
    format!("{END_MARKER_PREFIX}{pack_id} -->")
}

fn pack_ref(source_id: &str, pack_id: &str) -> String {
    format!("{source_id}:{pack_id}")
}

fn marker_id(source_id: &str, pack_id: &str) -> String {
    if source_id == "local" {
        pack_id.to_owned()
    } else {
        pack_ref(source_id, pack_id)
    }
}

fn lock_marker_id(entry: &LockedInstructionPack) -> String {
    marker_id(&entry.source_id, &entry.pack_id)
}

/// Byte offsets `(start, end)` spanning a pack's managed block, markers included.
fn find_block(content: &str, pack_id: &str) -> DaloResult<Option<(usize, usize)>> {
    let start = start_marker(pack_id);
    let end = end_marker(pack_id);
    let starts = content.match_indices(&start).collect::<Vec<_>>();
    let ends = content.match_indices(&end).collect::<Vec<_>>();

    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start_idx, _)], [(end_idx, _)]) if start_idx < end_idx => {
            Ok(Some((*start_idx, end_idx + end.len())))
        }
        ([], _) => Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "end marker exists without a matching start marker".to_owned(),
        }),
        (_, []) => Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "start marker exists without a matching end marker".to_owned(),
        }),
        _ => Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "expected exactly one ordered start/end marker pair".to_owned(),
        }),
    }
}

/// Render `body` into `content` as `pack_id`'s managed block.
///
/// When the block exists, only the bytes between its markers change. When it does
/// not, the block is appended, separated from existing content by a blank line.
/// Rendering the same body twice is idempotent.
pub fn render_block(content: &str, pack_id: &str, body: &str) -> DaloResult<String> {
    let line_ending = line_ending_for(content);
    let block = render_managed_block_with_line_ending(pack_id, body, line_ending)?;
    Ok(match find_block(content, pack_id)? {
        Some((start_idx, end_idx)) => {
            format!("{}{}{}", &content[..start_idx], block, &content[end_idx..])
        }
        None => append_block(content, &block, line_ending),
    })
}

#[cfg(test)]
fn render_managed_block(pack_id: &str, body: &str) -> DaloResult<String> {
    render_managed_block_with_line_ending(pack_id, body, "\n")
}

fn render_managed_block_with_line_ending(
    pack_id: &str,
    body: &str,
    line_ending: &str,
) -> DaloResult<String> {
    validate_body_markers(pack_id, body)?;
    let body = normalize_line_endings(
        body.trim_matches(|character| character == '\n' || character == '\r'),
        line_ending,
    );
    Ok(format!(
        "{}{}{}{}{}",
        start_marker(pack_id),
        line_ending,
        body,
        line_ending,
        end_marker(pack_id)
    ))
}

fn validate_body_markers(pack_id: &str, body: &str) -> DaloResult<()> {
    if body.contains(START_MARKER_PREFIX) || body.contains(END_MARKER_PREFIX) {
        return Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "instruction pack body contains dalo managed-block marker text".to_owned(),
        });
    }

    Ok(())
}

/// Check active instruction-pack lock entries against their rendered target blocks.
#[must_use]
pub fn instruction_block_drifts(
    paths: &StorePaths,
    sources: &[SourceConfig],
    active: &[LockedInstructionPack],
) -> Vec<InstructionBlockDrift> {
    let mut drifts = Vec::new();
    for entry in active {
        if let Some(drift) = instruction_block_drift(paths, sources, entry) {
            drifts.push(drift);
        }
    }
    drifts.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.source_id.cmp(&right.source_id))
            .then_with(|| left.pack_id.cmp(&right.pack_id))
    });
    drifts
}

fn instruction_block_drift(
    paths: &StorePaths,
    sources: &[SourceConfig],
    entry: &LockedInstructionPack,
) -> Option<InstructionBlockDrift> {
    let target = lock_entry_target_path(paths, &entry.target);
    let pack = match read_pack_for_lock_entry(paths, sources, entry) {
        Ok(pack) => pack,
        Err(error) => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::SourceMissing,
                message: format!("active instruction pack source could not be read: {error}"),
            });
        }
    };
    let content = match fs::read_to_string(&target) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::Missing,
                message: "instruction target file is missing".to_owned(),
            });
        }
        Err(error) => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::Malformed,
                message: format!("instruction target file could not be read: {error}"),
            });
        }
    };
    let marker_id = lock_marker_id(entry);
    let expected = match render_managed_block_with_line_ending(
        &marker_id,
        &pack.body,
        line_ending_for(&content),
    ) {
        Ok(block) => block,
        Err(error) => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::SourceMissing,
                message: format!("active instruction pack body is invalid: {error}"),
            });
        }
    };
    match find_block(&content, &marker_id) {
        Ok(Some((start_idx, end_idx))) if content[start_idx..end_idx] == expected => None,
        Ok(Some(_)) => Some(InstructionBlockDrift {
            source_id: entry.source_id.clone(),
            pack_id: entry.pack_id.clone(),
            target: entry.target.clone(),
            kind: InstructionBlockDriftKind::Stale,
            message: "instruction block does not match current pack body".to_owned(),
        }),
        Ok(None) => Some(InstructionBlockDrift {
            source_id: entry.source_id.clone(),
            pack_id: entry.pack_id.clone(),
            target: entry.target.clone(),
            kind: InstructionBlockDriftKind::Missing,
            message: "instruction block is missing from target file".to_owned(),
        }),
        Err(error) => Some(InstructionBlockDrift {
            source_id: entry.source_id.clone(),
            pack_id: entry.pack_id.clone(),
            target: entry.target.clone(),
            kind: InstructionBlockDriftKind::Malformed,
            message: error.to_string(),
        }),
    }
}

fn lock_entry_target_path(paths: &StorePaths, target: &Path) -> PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        lexically_normalize(&paths.root.join(target))
    }
}

fn read_pack_for_lock_entry(
    paths: &StorePaths,
    sources: &[SourceConfig],
    entry: &LockedInstructionPack,
) -> DaloResult<InstructionPack> {
    if entry.source_id == "local" {
        return read_local_pack(paths, &entry.pack_id);
    }

    let Some(source) = sources.iter().find(|source| source.id == entry.source_id) else {
        return Err(DaloError::unknown_source(
            entry.source_id.clone(),
            sources.iter().map(|source| source.id.clone()).collect(),
        ));
    };
    if !source.enabled {
        return Err(DaloError::StateError {
            reason: format!("instruction pack source `{}` is disabled", source.id),
        });
    }
    validate_source_pack(paths, source, &entry.pack_id, entry.commit.as_deref())?;
    read_pack_from_dir(&source.path.join("instructions"), &entry.pack_id)
}

fn append_block(content: &str, block: &str, line_ending: &str) -> String {
    if content.is_empty() {
        return format!("{block}{line_ending}");
    }
    // Normalize the seam to exactly one blank line before the appended block.
    let double_line_ending = format!("{line_ending}{line_ending}");
    let separator = if content.ends_with(&double_line_ending) {
        ""
    } else if content.ends_with(line_ending) {
        line_ending
    } else {
        return format!("{content}{line_ending}{line_ending}{block}{line_ending}");
    };
    format!("{content}{separator}{block}{line_ending}")
}

/// Remove `pack_id`'s managed block, preserving content outside it. A single
/// separating newline on each side of the block is also dropped so removal leaves
/// no blank gap where the block used to be.
pub fn remove_block(content: &str, pack_id: &str) -> DaloResult<String> {
    let Some((start_idx, end_idx)) = find_block(content, pack_id)? else {
        return Ok(content.to_owned());
    };
    let line_ending = line_ending_for(content);
    let before_raw = &content[..start_idx];
    let (before, before_had_line_ending) = strip_line_ending_suffix(before_raw, line_ending);
    let after_raw = &content[end_idx..];
    let (after, _) = strip_line_ending_prefix(after_raw, line_ending);
    Ok(match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_owned(),
        (_, true) if before_had_line_ending => format!("{before}{line_ending}"),
        (_, true) => before.to_owned(),
        _ if before.ends_with(line_ending) || after.starts_with(line_ending) => {
            format!("{before}{after}")
        }
        _ => format!("{before}{line_ending}{after}"),
    })
}

fn line_ending_for(content: &str) -> &'static str {
    let crlf_count = content.match_indices("\r\n").count();
    let bytes = content.as_bytes();
    let lf_count = bytes
        .iter()
        .enumerate()
        .filter(|(index, byte)| {
            **byte == b'\n' && (*index == 0 || bytes[index.saturating_sub(1)] != b'\r')
        })
        .count();
    if crlf_count > lf_count { "\r\n" } else { "\n" }
}

fn normalize_line_endings(content: &str, line_ending: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', line_ending)
}

fn strip_line_ending_suffix<'a>(content: &'a str, line_ending: &str) -> (&'a str, bool) {
    if let Some(stripped) = content.strip_suffix(line_ending) {
        return (stripped, true);
    }
    if line_ending != "\r\n"
        && let Some(stripped) = content.strip_suffix("\r\n")
    {
        return (stripped, true);
    }
    if let Some(stripped) = content.strip_suffix('\n') {
        return (stripped, true);
    }
    (content, false)
}

fn strip_line_ending_prefix<'a>(content: &'a str, line_ending: &str) -> (&'a str, bool) {
    if let Some(stripped) = content.strip_prefix(line_ending) {
        return (stripped, true);
    }
    if line_ending != "\r\n"
        && let Some(stripped) = content.strip_prefix("\r\n")
    {
        return (stripped, true);
    }
    if let Some(stripped) = content.strip_prefix('\n') {
        return (stripped, true);
    }
    (content, false)
}

/// Whether `content` contains `pack_id`'s managed block.
#[must_use]
pub fn has_block(content: &str, pack_id: &str) -> bool {
    find_block(content, pack_id).is_ok_and(|block| block.is_some())
}

/// Validate a pack ID (same character rules as a source ID).
fn is_valid_pack_id(pack_id: &str) -> bool {
    !pack_id.is_empty()
        && pack_id != "."
        && pack_id != ".."
        && pack_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

/// Optional `version:` line in the pack's leading lines.
fn parse_version(body: &str) -> Option<String> {
    body.lines()
        .take(5)
        .find_map(|line| line.trim().strip_prefix("version:"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Read a user-authored pack from `local/instructions/<id>.md`.
pub fn read_local_pack(paths: &StorePaths, pack_id: &str) -> DaloResult<InstructionPack> {
    if !is_valid_pack_id(pack_id) {
        return Err(DaloError::InvalidSourceId {
            id: pack_id.to_owned(),
            reason: "instruction pack id must be `[A-Za-z0-9._-]` and not `.`/`..`".to_owned(),
        });
    }
    read_pack_from_dir(&paths.local_instructions_dir, pack_id)
}

#[derive(Debug)]
struct ResolvedInstructionPack {
    pack: InstructionPack,
    source_id: String,
    commit: Option<String>,
}

fn parse_pack_selector(selector: &str) -> DaloResult<(String, String)> {
    let (source_id, pack_id) = selector
        .split_once(':')
        .map_or(("local", selector), |(source_id, pack_id)| {
            (source_id, pack_id)
        });
    if source_id.is_empty() || !is_valid_pack_id(pack_id) {
        return Err(DaloError::InvalidArgument {
            reason: "instruction pack references must use `<source>:<pack>` or a local `<pack>`; pack IDs must match `[A-Za-z0-9._-]` and not be `.`/`..`"
                .to_owned(),
        });
    }
    Ok((source_id.to_owned(), pack_id.to_owned()))
}

fn resolve_pack(paths: &StorePaths, selector: &str) -> DaloResult<ResolvedInstructionPack> {
    let (source_id, pack_id) = parse_pack_selector(selector)?;
    if source_id == "local" {
        return Ok(ResolvedInstructionPack {
            pack: read_local_pack(paths, &pack_id)?,
            source_id,
            commit: None,
        });
    }

    let config = store::read_config(paths)?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            DaloError::unknown_source(
                source_id.clone(),
                config
                    .sources
                    .iter()
                    .map(|source| source.id.clone())
                    .collect(),
            )
        })?;
    if !source.enabled {
        return Err(DaloError::StateError {
            reason: format!(
                "instruction pack source `{source_id}` is disabled; enable the source before activating `{selector}`"
            ),
        });
    }
    let approvals = store::read_approvals(paths)?;
    ensure_instruction_source_approved(source, &approvals.approvals, Some(&pack_id))?;
    let commit = validate_source_pack(paths, source, &pack_id, None)?;
    Ok(ResolvedInstructionPack {
        pack: read_pack_from_dir(&source.path.join("instructions"), &pack_id)?,
        source_id,
        commit: Some(commit),
    })
}

fn ensure_instruction_source_approved(
    source: &SourceConfig,
    approvals: &[ApprovalRecord],
    pack_id: Option<&str>,
) -> DaloResult<()> {
    if source.trusted
        || approvals
            .iter()
            .any(|approval| approval.scope == "source" && approval.value == source.id)
    {
        return Ok(());
    }
    let subject = pack_id.map_or_else(
        || format!("source `{}`", source.id),
        |pack_id| format!("instruction pack `{}`", pack_ref(&source.id, pack_id)),
    );
    Err(DaloError::StateError {
        reason: format!(
            "approval for {subject} is missing or was revoked; review the source and run `dalo approve source {}`",
            source.id
        ),
    })
}

fn validate_source_pack(
    paths: &StorePaths,
    source: &SourceConfig,
    pack_id: &str,
    expected_commit: Option<&str>,
) -> DaloResult<String> {
    if git::is_dirty(&source.path)? {
        return Err(DaloError::DirtySource {
            source_id: source.id.clone(),
            path: source.path.clone(),
        });
    }
    let pack_path = source
        .path
        .join("instructions")
        .join(format!("{pack_id}.md"));
    let metadata =
        fs::symlink_metadata(&pack_path).map_err(|_| DaloError::InstructionPackNotFound {
            pack_id: pack_id.to_owned(),
            path: pack_path.clone(),
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(DaloError::StateError {
            reason: format!(
                "instruction pack `{}` must be a regular, non-symlink file",
                pack_ref(&source.id, pack_id)
            ),
        });
    }
    if !git::is_tracked_file(&source.path, &pack_path)? {
        return Err(DaloError::StateError {
            reason: format!(
                "instruction pack `{}` is not tracked by its source commit; commit it before enabling the pack",
                pack_ref(&source.id, pack_id)
            ),
        });
    }
    let commit = git::rev_parse_head(&source.path)?;
    if let Some(expected) = expected_commit
        && expected != commit
    {
        return Err(DaloError::StateError {
            reason: format!(
                "instruction pack `{}` was enabled from commit `{expected}`, but the source checkout is now `{commit}`; review and re-enable the pack",
                pack_ref(&source.id, pack_id)
            ),
        });
    }
    if source.kind == SourceKind::Catalog {
        let source_lock = crate::catalog::read_source_lock(paths)?;
        let locked_commit = source_lock
            .catalog(&source.id)
            .map(|locked| locked.commit.as_str())
            .ok_or_else(|| DaloError::StateError {
                reason: format!(
                    "catalog source `{}` has no pinned source-lock entry; run `dalo source refresh {}` before enabling instructions",
                    source.id, source.id
                ),
            })?;
        if locked_commit != commit {
            return Err(DaloError::StateError {
                reason: format!(
                    "catalog source `{}` checkout `{commit}` does not match its pinned commit `{locked_commit}`",
                    source.id
                ),
            });
        }
    }
    Ok(commit)
}

fn read_pack_from_dir(dir: &Path, pack_id: &str) -> DaloResult<InstructionPack> {
    let path = dir.join(format!("{pack_id}.md"));
    let body = fs::read_to_string(&path).map_err(|_| DaloError::InstructionPackNotFound {
        pack_id: pack_id.to_owned(),
        path: path.clone(),
    })?;
    Ok(InstructionPack {
        id: pack_id.to_owned(),
        version: parse_version(&body),
        body,
    })
}

/// Enable a local or source-qualified pack: render its managed block into `target` and record it in
/// the user lock. Idempotent: enabling an already-active pack re-renders the block
/// and updates the lock entry in place.
pub fn enable_pack(
    paths: &StorePaths,
    selector: &str,
    target: &Path,
    dry_run: bool,
) -> DaloResult<InstructionPackReport> {
    enable_pack_with_logical_targets(paths, selector, target, &[], dry_run)
}

fn enable_pack_with_logical_targets(
    paths: &StorePaths,
    selector: &str,
    target: &Path,
    logical_targets: &[String],
    dry_run: bool,
) -> DaloResult<InstructionPackReport> {
    enable_pack_with_lock_writer(
        paths,
        selector,
        target,
        logical_targets,
        dry_run,
        store::write_user_lock,
    )
}

fn enable_pack_with_lock_writer<F>(
    paths: &StorePaths,
    selector: &str,
    target: &Path,
    logical_targets: &[String],
    dry_run: bool,
    write_lock: F,
) -> DaloResult<InstructionPackReport>
where
    F: FnOnce(&StorePaths, &crate::lockfile::UserLock) -> DaloResult<()>,
{
    let target = normalize_target_path(target)?;
    let resolved = resolve_pack(paths, selector)?;
    let pack = resolved.pack;
    let marker_id = marker_id(&resolved.source_id, &pack.id);
    let mut lock = store::read_user_lock(paths)?;
    let _target_lock = acquire_target_lock(paths, &target)?;
    let snapshot = target_snapshot(&target)?;
    let existing = snapshot.content.clone().unwrap_or_default();
    let rendered = render_block(&existing, &marker_id, &pack.body)?;
    if !dry_run {
        lock.active_instruction_packs.retain(|entry| {
            !(entry.source_id == resolved.source_id
                && entry.pack_id == pack.id
                && targets_match(entry, &target))
        });
        lock.active_instruction_packs.push(LockedInstructionPack {
            pack_id: pack.id.clone(),
            target: target.clone(),
            logical_targets: logical_targets.to_vec(),
            source_id: resolved.source_id.clone(),
            commit: resolved.commit,
            version: pack.version,
        });
        sort_instruction_lock_entries(&mut lock.active_instruction_packs);
        let written_identity = write_target_if_unchanged(&snapshot, &rendered)?;
        if let Err(error) = write_lock(paths, &lock) {
            return Err(restore_target_after_error(
                snapshot,
                &rendered,
                written_identity,
                error,
            ));
        }
    }

    Ok(InstructionPackReport {
        pack_id: pack.id,
        source_id: resolved.source_id,
        target,
        action: "enabled".to_owned(),
        dry_run,
        warning: None,
    })
}

fn sort_instruction_lock_entries(entries: &mut [LockedInstructionPack]) {
    entries.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then_with(|| left.pack_id.cmp(&right.pack_id))
            .then(left.target.cmp(&right.target))
    });
}

/// Disable a pack: remove its managed block from `target` and drop its lock entry.
pub fn disable_pack(
    paths: &StorePaths,
    selector: &str,
    target: &Path,
    dry_run: bool,
) -> DaloResult<InstructionPackReport> {
    disable_pack_with_lock_writer(paths, selector, target, dry_run, store::write_user_lock)
}

fn disable_pack_with_lock_writer<F>(
    paths: &StorePaths,
    selector: &str,
    target: &Path,
    dry_run: bool,
    write_lock: F,
) -> DaloResult<InstructionPackReport>
where
    F: FnOnce(&StorePaths, &crate::lockfile::UserLock) -> DaloResult<()>,
{
    let (source_id, pack_id) = parse_pack_selector(selector)?;
    let marker_id = marker_id(&source_id, &pack_id);
    let target = normalize_target_path(target)?;
    let mut lock = store::read_user_lock(paths)?;
    let _target_lock = acquire_target_lock(paths, &target)?;
    let snapshot = target_snapshot(&target)?;
    let existing = snapshot.content.clone().unwrap_or_default();
    let (block, malformed_error) = match find_block(&existing, &marker_id) {
        Ok(block) => (block, None),
        Err(error) => (None, Some(error.to_string())),
    };
    let has_block = block.is_some();
    let before = lock.active_instruction_packs.len();
    let has_lock_entry = lock.active_instruction_packs.iter().any(|entry| {
        entry.source_id == source_id && entry.pack_id == pack_id && targets_match(entry, &target)
    });
    let warning = malformed_error.map(|error| {
        let lock_action = if has_lock_entry {
            if dry_run {
                "the lock entry would be removed"
            } else {
                "the lock entry was removed"
            }
        } else {
            "no matching lock entry was found"
        };
        format!("{error}; target left untouched and {lock_action}")
    });

    let updated = if warning.is_none() && has_block {
        Some(remove_block(&existing, &marker_id)?)
    } else {
        None
    };
    let action = if has_block || has_lock_entry {
        "disabled"
    } else {
        "unchanged"
    };

    lock.active_instruction_packs.retain(|entry| {
        !(entry.source_id == source_id && entry.pack_id == pack_id && targets_match(entry, &target))
    });
    if !dry_run {
        if let Some(updated) = updated {
            let written_identity = write_target_if_unchanged(&snapshot, &updated)?;
            if lock.active_instruction_packs.len() != before
                && let Err(error) = write_lock(paths, &lock)
            {
                return Err(restore_target_after_error(
                    snapshot,
                    &updated,
                    written_identity,
                    error,
                ));
            }
        } else if lock.active_instruction_packs.len() != before {
            write_lock(paths, &lock)?;
        }
    }

    Ok(InstructionPackReport {
        pack_id,
        source_id,
        target,
        action: action.to_owned(),
        dry_run,
        warning,
    })
}

fn normalize_target_path(target: &Path) -> DaloResult<PathBuf> {
    let absolute = store::absolute_path(target)?;
    Ok(lexically_normalize(&absolute))
}

fn targets_match(entry: &LockedInstructionPack, target: &Path) -> bool {
    entry.target == target
        || (entry.target.is_relative()
            && normalize_target_path(&entry.target).is_ok_and(|normalized| normalized == target))
        || (entry.target.is_absolute()
            && fs::canonicalize(&entry.target)
                .ok()
                .zip(fs::canonicalize(target).ok())
                .is_some_and(|(left, right)| left == right))
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
}

#[cfg(test)]
fn read_target(target: &Path) -> DaloResult<String> {
    match fs::read_to_string(target) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Debug)]
struct TargetSnapshot {
    target: PathBuf,
    content: Option<String>,
    identity: Option<TargetIdentity>,
}

/// Stable identity for a target inode. A path can be replaced by another file
/// while an operation is in flight; checking this identity prevents rollback
/// from touching the replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetIdentity {
    device: u64,
    inode: u64,
}

fn target_identity(metadata: &fs::Metadata) -> TargetIdentity {
    TargetIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

/// Advisory lock held while a target is read, rendered, replaced, or rolled back.
/// The lock is keyed by the normalized target path and intentionally lives in the
/// Dalo store so missing targets can be serialized before their first write.
#[derive(Debug)]
struct TargetLock {
    _file: fs::File,
}

fn acquire_target_lock(paths: &StorePaths, target: &Path) -> DaloResult<TargetLock> {
    let digest = Sha256::digest(target.to_string_lossy().as_bytes());
    let suffix = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = paths.root.join(format!(".target-{suffix}.lock"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(TargetLock { _file: file })
}

fn target_snapshot(target: &Path) -> DaloResult<TargetSnapshot> {
    let target = writable_target_path(target)?;
    match fs::read_to_string(&target) {
        Ok(content) => {
            let metadata = fs::metadata(&target)?;
            Ok(TargetSnapshot {
                target,
                content: Some(content),
                identity: Some(target_identity(&metadata)),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TargetSnapshot {
            target,
            content: None,
            identity: None,
        }),
        Err(error) => Err(error.into()),
    }
}

fn write_target_if_unchanged(
    snapshot: &TargetSnapshot,
    content: &str,
) -> DaloResult<TargetIdentity> {
    let mut file = match snapshot.identity {
        Some(_) => OpenOptions::new()
            .read(true)
            .write(true)
            .open(&snapshot.target)?,
        None => OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&snapshot.target)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    DaloError::InstructionTargetChanged {
                        path: snapshot.target.clone(),
                    }
                } else {
                    error.into()
                }
            })?,
    };
    // Keep the target inode locked while checking and replacing its contents.
    // Editors that use the same standard advisory lock cannot interleave an
    // update, and a rename-based replacement is harmless because this handle
    // continues to refer to the original inode.
    file.lock()?;
    let identity = target_identity(&file.metadata()?);
    if snapshot
        .identity
        .is_some_and(|expected| expected != identity)
    {
        return Err(DaloError::InstructionTargetChanged {
            path: snapshot.target.clone(),
        });
    }
    let current = read_locked_target(&mut file)?;
    if current != snapshot.content.as_deref().unwrap_or_default() {
        return Err(DaloError::InstructionTargetChanged {
            path: snapshot.target.clone(),
        });
    }
    write_locked_target(&mut file, content)?;
    if !target_has_identity(&snapshot.target, identity)?
        || read_locked_target(&mut file)? != content
    {
        return Err(DaloError::InstructionTargetChanged {
            path: snapshot.target.clone(),
        });
    }
    Ok(identity)
}

fn read_locked_target(file: &mut fs::File) -> DaloResult<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn write_locked_target(file: &mut fs::File, content: &str) -> DaloResult<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(content.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn target_has_identity(target: &Path, expected: TargetIdentity) -> DaloResult<bool> {
    match fs::metadata(target) {
        Ok(metadata) => Ok(target_identity(&metadata) == expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn restore_target_after_error(
    snapshot: TargetSnapshot,
    written_content: &str,
    written_identity: TargetIdentity,
    original_error: DaloError,
) -> DaloError {
    let target = snapshot.target.clone();
    match restore_target(snapshot, written_content, written_identity) {
        Ok(()) => original_error,
        Err(restore_error) => DaloError::Io(std::io::Error::other(format!(
            "{original_error}; also failed to restore instruction target `{}`: {restore_error}",
            target.display()
        ))),
    }
}

fn restore_target(
    snapshot: TargetSnapshot,
    written_content: &str,
    written_identity: TargetIdentity,
) -> DaloResult<()> {
    let target = snapshot.target.clone();
    match OpenOptions::new().read(true).write(true).open(&target) {
        Ok(mut file) => (|| -> DaloResult<()> {
            file.lock()?;
            if target_identity(&file.metadata()?) != written_identity
                || read_locked_target(&mut file)? != written_content
            {
                return Err(std::io::Error::other(
                    "instruction target changed during rollback; newer content was left untouched",
                )
                .into());
            }
            if let Some(content) = snapshot.content {
                write_locked_target(&mut file, &content)
            } else {
                drop(file);
                if target_has_identity(&target, written_identity)? {
                    fs::remove_file(&target)?;
                }
                Ok(())
            }
        })(),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound && snapshot.content.is_none() =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
fn write_target(target: &Path, content: &str) -> DaloResult<()> {
    let target = writable_target_path(target)?;
    let existing_permissions = fs::metadata(&target)
        .map(|metadata| metadata.permissions())
        .ok();
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;
    temp_file.persist(&target).map_err(|error| error.error)?;
    if let Some(permissions) = existing_permissions {
        fs::set_permissions(&target, permissions)?;
    }
    Ok(())
}

fn writable_target_path(target: &Path) -> DaloResult<PathBuf> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(fs::canonicalize(target)?),
        Ok(_) => Ok(target.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(target.to_path_buf()),
        Err(error) => Err(error.into()),
    }
}

/// A discovered instruction pack (read-only inventory entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredPack {
    /// Pack ID.
    pub id: String,
    /// Source the pack was discovered in.
    pub source_id: String,
    /// Declared version, when present.
    pub version: Option<String>,
    /// Declared topics/tags.
    pub topics: Vec<String>,
    /// Whether the pack is currently enabled (active in the user lock).
    pub enabled: bool,
}

impl DiscoveredPack {
    /// Source-qualified pack ref.
    #[must_use]
    pub fn pack_ref(&self) -> String {
        format!("{}:{}", self.source_id, self.id)
    }
}

/// A topic overlap between two active instruction packs (advisory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicOverlap {
    /// The two overlapping pack refs.
    pub packs: [String; 2],
    /// The topics they share.
    pub topics: Vec<String>,
}

/// Discover instruction packs across the local store and configured sources.
///
/// Read-only: never materializes a pack. A pack is `enabled` when an active lock
/// entry matches its source and ID.
#[must_use]
pub fn discover_packs(
    paths: &StorePaths,
    sources: &[SourceConfig],
    active: &[LockedInstructionPack],
) -> Vec<DiscoveredPack> {
    let enabled: BTreeSet<(&str, &str)> = active
        .iter()
        .map(|entry| (entry.source_id.as_str(), entry.pack_id.as_str()))
        .collect();
    let mut packs = Vec::new();
    scan_pack_dir(&paths.local_instructions_dir, "local", &enabled, &mut packs);
    for source in sources {
        // The local source's instructions dir is the one scanned above; skip it so
        // local packs are not counted twice.
        if source.kind == SourceKind::Local {
            continue;
        }
        scan_pack_dir(
            &source.path.join("instructions"),
            &source.id,
            &enabled,
            &mut packs,
        );
    }
    packs.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then_with(|| left.id.cmp(&right.id))
    });
    packs
}

fn scan_pack_dir(
    dir: &Path,
    source_id: &str,
    enabled: &BTreeSet<(&str, &str)>,
    out: &mut Vec<DiscoveredPack>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !is_valid_pack_id(id) {
            continue;
        }
        let body = fs::read_to_string(&path).unwrap_or_default();
        out.push(DiscoveredPack {
            id: id.to_owned(),
            source_id: source_id.to_owned(),
            version: parse_version(&body),
            topics: parse_topics(&body),
            enabled: enabled.contains(&(source_id, id)),
        });
    }
}

/// Optional `topics:`/`tags:` line in the pack's leading lines (comma-separated).
fn parse_topics(body: &str) -> Vec<String> {
    body.lines()
        .take(8)
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("topics:")
                .or_else(|| trimmed.strip_prefix("tags:"))
        })
        .map(|value| {
            value
                .split(',')
                .map(|topic| topic.trim().to_owned())
                .filter(|topic| !topic.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Detect declared-topic overlaps among active packs. Advisory only: overlaps
/// never block materialization.
#[must_use]
pub fn topic_overlaps(active: &[DiscoveredPack]) -> Vec<TopicOverlap> {
    let mut overlaps = Vec::new();
    for (index, left) in active.iter().enumerate() {
        for right in active.iter().skip(index + 1) {
            let mut shared: Vec<String> = left
                .topics
                .iter()
                .filter(|topic| right.topics.contains(topic))
                .cloned()
                .collect();
            if !shared.is_empty() {
                shared.sort();
                shared.dedup();
                overlaps.push(TopicOverlap {
                    packs: [left.pack_ref(), right.pack_ref()],
                    topics: shared,
                });
            }
        }
    }
    overlaps
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    use super::*;
    use proptest::prelude::*;

    const PACK: &str = "house-style";

    #[test]
    fn render_block_should_append_when_absent_and_be_idempotent() {
        let original = "# Project\n\nNotes.\n";
        let once = render_block(original, PACK, "Use tabs.").expect("render should succeed");
        assert!(has_block(&once, PACK));
        assert!(once.starts_with("# Project\n\nNotes.\n"));
        // A second render with the same body changes nothing.
        let twice = render_block(&once, PACK, "Use tabs.").expect("render should succeed");
        assert_eq!(once, twice);
    }

    #[test]
    fn render_block_should_preserve_crlf_line_endings() {
        let original = "# Project\r\n\r\nNotes.\r\n";
        let rendered =
            render_block(original, PACK, "Use tabs.\nSecond line.").expect("render should succeed");

        assert!(rendered.contains("<!-- dalo:start house-style -->\r\n"));
        assert!(rendered.contains("Use tabs.\r\nSecond line."));
        assert!(!rendered.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn render_block_should_only_touch_bytes_inside_markers() {
        let original = "TOP CONTENT\n\n<!-- dalo:start house-style -->\nold\n<!-- dalo:end house-style -->\n\nBOTTOM CONTENT\n";
        let updated = render_block(original, PACK, "new body").expect("render should succeed");
        // Everything outside the block is byte-identical.
        assert!(updated.starts_with("TOP CONTENT\n\n"));
        assert!(updated.ends_with("\n\nBOTTOM CONTENT\n"));
        assert!(updated.contains("new body"));
        assert!(!updated.contains("old"));
    }

    proptest! {
        #[test]
        fn render_block_should_preserve_surrounding_text_and_stay_idempotent(
            prefix in "[A-Za-z0-9 ._\\n-]{0,80}",
            body in "[A-Za-z0-9 ._\\n-]{0,80}",
            suffix in "[A-Za-z0-9 ._\\n-]{0,80}",
        ) {
            let original = format!(
                "{}{}\\nold body\\n{}{}",
                prefix,
                start_marker(PACK),
                end_marker(PACK),
                suffix
            );

            let rendered = render_block(&original, PACK, &body).expect("render should succeed");
            prop_assert!(rendered.starts_with(&prefix));
            prop_assert!(rendered.ends_with(&suffix));
            prop_assert_eq!(
                render_block(&rendered, PACK, &body).expect("rerender should succeed"),
                rendered
            );
        }
    }

    #[test]
    fn remove_block_should_preserve_surrounding_content() {
        let original = "# Header\n\nIntro.\n";
        let rendered = render_block(original, PACK, "Body.").expect("render should succeed");
        let removed = remove_block(&rendered, PACK).expect("remove should succeed");
        assert!(!has_block(&removed, PACK));
        // The user-owned content survives the round trip.
        assert!(removed.contains("# Header"));
        assert!(removed.contains("Intro."));
        assert!(!removed.contains("dalo:start"));
    }

    #[test]
    fn remove_block_should_keep_content_on_both_sides() {
        let original = "ABOVE\n\n<!-- dalo:start house-style -->\nbody\n<!-- dalo:end house-style -->\n\nBELOW\n";
        let removed = remove_block(original, PACK).expect("remove should succeed");
        assert!(removed.contains("ABOVE"));
        assert!(removed.contains("BELOW"));
        assert!(!removed.contains("dalo:"));
    }

    #[test]
    fn remove_block_should_preserve_crlf_seams() {
        let original = "ABOVE\r\n\r\n<!-- dalo:start house-style -->\r\nbody\r\n<!-- dalo:end house-style -->\r\n\r\nBELOW\r\n";
        let removed = remove_block(original, PACK).expect("remove should succeed");

        assert_eq!(removed, "ABOVE\r\n\r\nBELOW\r\n");
    }

    #[test]
    fn remove_block_should_noop_when_absent() {
        let original = "# Header\n\nNo blocks here.\n";
        assert_eq!(
            remove_block(original, PACK).expect("remove should succeed"),
            original
        );
    }

    #[test]
    fn render_block_should_reject_malformed_markers() {
        let malformed = "# Header\n\n<!-- dalo:start house-style -->\nmissing end\n";
        let error = render_block(malformed, PACK, "Body.").expect_err("render should fail");

        assert!(matches!(error, DaloError::MalformedInstructionBlock { .. }));
    }

    #[test]
    fn render_block_should_reject_duplicate_start_markers() {
        let malformed = format!(
            "# Header\n\n{}\nold\n{}\nsecond\n{}\n",
            start_marker(PACK),
            start_marker(PACK),
            end_marker(PACK)
        );

        let error = render_block(&malformed, PACK, "Body.").expect_err("render should fail");

        assert!(matches!(error, DaloError::MalformedInstructionBlock { .. }));
    }

    #[test]
    fn render_block_should_reject_end_before_start() {
        let malformed = format!(
            "# Header\n\n{}\nold\n{}\n",
            end_marker(PACK),
            start_marker(PACK)
        );

        let error = render_block(&malformed, PACK, "Body.").expect_err("render should fail");

        assert!(matches!(error, DaloError::MalformedInstructionBlock { .. }));
    }

    #[test]
    fn render_block_should_replace_user_edits_inside_managed_block() {
        let rendered = render_block("# Header\n", PACK, "Original body.")
            .expect("initial render should succeed");
        let edited = rendered.replace("Original body.", "User edit inside block.");

        let updated =
            render_block(&edited, PACK, "Original body.").expect("rerender should succeed");

        assert_eq!(updated, rendered);
        assert!(!updated.contains("User edit inside block."));
    }

    #[test]
    fn render_block_should_handle_existing_block_at_eof_without_trailing_newline() {
        let content = format!("{}\nOld body\n{}", start_marker(PACK), end_marker(PACK));

        let updated = render_block(&content, PACK, "New body").expect("render should succeed");

        assert_eq!(updated, render_managed_block(PACK, "New body").unwrap());
    }

    #[test]
    fn render_block_should_reject_same_id_marker_in_body() {
        let body = format!("Do not emit this marker:\n{}\n", end_marker(PACK));

        let error = render_block("# Header\n", PACK, &body).expect_err("render should fail");

        assert_body_marker_error(error, PACK);
    }

    #[test]
    fn render_block_should_reject_different_id_marker_in_body() {
        let body = format!(
            "Example for another pack:\n{}\n",
            start_marker("other-pack")
        );

        let error = render_block("# Header\n", PACK, &body).expect_err("render should fail");

        assert_body_marker_error(error, PACK);
    }

    #[test]
    fn enable_pack_should_reject_marker_body_without_rewriting_target() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp.path().join("store");
        let target = temp.path().join("AGENTS.md");
        store::init_store(store_root.clone(), false).expect("store should be initialized");
        let paths = StorePaths::new(store_root);
        fs::write(
            paths.local_instructions_dir.join(format!("{PACK}.md")),
            format!("Before\n{}\nAfter\n", start_marker("other-pack")),
        )
        .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");

        let error = enable_pack(&paths, PACK, &target, false).expect_err("enable should fail");

        assert_body_marker_error(error, PACK);
        assert_eq!(
            fs::read_to_string(&target).expect("target should be readable"),
            "user-owned content\n"
        );
        let lock = store::read_user_lock(&paths).expect("lock should be readable");
        assert!(lock.active_instruction_packs.is_empty());
    }

    #[test]
    fn enable_pack_should_restore_target_when_lock_write_fails() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp.path().join("store");
        let target = temp.path().join("AGENTS.md");
        store::init_store(store_root.clone(), false).expect("store should be initialized");
        let paths = StorePaths::new(store_root);
        fs::write(
            paths.local_instructions_dir.join(format!("{PACK}.md")),
            "Body\n",
        )
        .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");

        let error = enable_pack_with_lock_writer(&paths, PACK, &target, &[], false, |_, _| {
            Err(DaloError::Io(std::io::Error::other("lock write failed")))
        })
        .expect_err("lock write failure should fail enable");

        assert!(matches!(error, DaloError::Io(_)));
        assert_eq!(
            fs::read_to_string(&target).expect("target should be readable"),
            "user-owned content\n"
        );
        assert!(
            store::read_user_lock(&paths)
                .expect("lock should be readable")
                .active_instruction_packs
                .is_empty()
        );
    }

    #[test]
    fn disable_pack_should_restore_target_when_lock_write_fails() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp.path().join("store");
        let target = temp.path().join("AGENTS.md");
        store::init_store(store_root.clone(), false).expect("store should be initialized");
        let paths = StorePaths::new(store_root);
        fs::write(
            paths.local_instructions_dir.join(format!("{PACK}.md")),
            "Body\n",
        )
        .expect("pack should be written");
        enable_pack(&paths, PACK, &target, false).expect("pack should enable");
        let before = fs::read_to_string(&target).expect("target should be readable");

        let error = disable_pack_with_lock_writer(&paths, PACK, &target, false, |_, _| {
            Err(DaloError::Io(std::io::Error::other("lock write failed")))
        })
        .expect_err("lock write failure should fail disable");

        assert!(matches!(error, DaloError::Io(_)));
        assert_eq!(fs::read_to_string(&target).unwrap(), before);
        assert_eq!(
            store::read_user_lock(&paths)
                .expect("lock should be readable")
                .active_instruction_packs
                .len(),
            1
        );
    }

    #[test]
    fn disable_pack_should_remove_lock_and_leave_malformed_target_untouched() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp.path().join("store");
        let target = temp.path().join("AGENTS.md");
        store::init_store(store_root.clone(), false).expect("store should be initialized");
        let paths = StorePaths::new(store_root);
        fs::write(
            paths.local_instructions_dir.join(format!("{PACK}.md")),
            "Body\n",
        )
        .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");
        enable_pack(&paths, PACK, &target, false).expect("pack should enable");

        let malformed = format!(
            "user-owned content\n\n{}\nmissing end\n",
            start_marker(PACK)
        );
        fs::write(&target, &malformed).expect("malformed target should be written");

        let report = disable_pack(&paths, PACK, &target, false)
            .expect("disable should recover the malformed block lock");

        assert_eq!(report.action, "disabled");
        assert!(
            report
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("target left untouched"))
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), malformed);
        assert!(
            store::read_user_lock(&paths)
                .unwrap()
                .active_instruction_packs
                .is_empty()
        );
    }

    #[test]
    fn instruction_rollback_should_report_restore_failure() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let occupied_target = temp.path().join("AGENTS.md");
        fs::create_dir(&occupied_target).expect("target directory should be created");
        let snapshot = TargetSnapshot {
            target: occupied_target.clone(),
            content: Some("previous content\n".to_owned()),
            identity: Some(TargetIdentity {
                device: 0,
                inode: 0,
            }),
        };

        let error = restore_target_after_error(
            snapshot,
            "new content",
            TargetIdentity {
                device: 0,
                inode: 0,
            },
            DaloError::Io(std::io::Error::other("lock write failed")),
        );
        let message = error.to_string();

        assert!(message.contains("lock write failed"));
        assert!(message.contains("also failed to restore instruction target"));
        assert!(message.contains(&occupied_target.display().to_string()));
    }

    #[test]
    fn instruction_refresh_rollback_should_restore_written_targets() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        fs::write(&target, "before\n").expect("target should be seeded");
        let snapshot = target_snapshot(&target).expect("snapshot should be readable");
        let mut prepared = vec![PreparedInstructionMutation {
            logical_targets: Vec::new(),
            target: target.clone(),
            snapshot,
            rendered: Some("after\n".to_owned()),
            action: "refreshed".to_owned(),
            warning: None,
        }];
        let written = apply_prepared_target_writes(&mut prepared)
            .expect("instruction refresh should be written");
        let rollback = InstructionRefreshRollback {
            prepared,
            written,
            _target_locks: Vec::new(),
        };

        rollback.restore().expect("rollback should restore target");

        assert_eq!(fs::read_to_string(target).unwrap(), "before\n");
    }

    #[test]
    fn batch_write_should_roll_back_earlier_targets_after_a_later_concurrent_edit() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let first = temp.path().join("AGENTS.md");
        let second = temp.path().join("CLAUDE.md");
        fs::write(&first, "first before\n").expect("first target should be seeded");
        fs::write(&second, "second before\n").expect("second target should be seeded");
        let first_snapshot = target_snapshot(&first).expect("first snapshot should be readable");
        let second_snapshot = target_snapshot(&second).expect("second snapshot should be readable");
        fs::write(&second, "external edit\n").expect("second target should be edited");
        let mut prepared = vec![
            PreparedInstructionMutation {
                logical_targets: vec!["codex".to_owned()],
                target: first.clone(),
                snapshot: first_snapshot,
                rendered: Some("first after\n".to_owned()),
                action: "enabled".to_owned(),
                warning: None,
            },
            PreparedInstructionMutation {
                logical_targets: vec!["claude".to_owned()],
                target: second.clone(),
                snapshot: second_snapshot,
                rendered: Some("second after\n".to_owned()),
                action: "enabled".to_owned(),
                warning: None,
            },
        ];

        let error = apply_prepared_target_writes(&mut prepared)
            .expect_err("concurrent edit should fail the batch");

        assert!(matches!(error, DaloError::InstructionTargetChanged { .. }));
        assert_eq!(fs::read_to_string(first).unwrap(), "first before\n");
        assert_eq!(fs::read_to_string(second).unwrap(), "external edit\n");
    }

    #[test]
    fn write_target_if_unchanged_should_preserve_external_edit() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        fs::write(&target, "before\n").expect("target should be seeded");
        let snapshot = target_snapshot(&target).expect("snapshot should be readable");
        fs::write(&target, "external edit\n").expect("target should be edited");

        let error = write_target_if_unchanged(&snapshot, "dalo output\n")
            .expect_err("external edit should block replacement");
        assert!(matches!(error, DaloError::InstructionTargetChanged { .. }));
        assert_eq!(fs::read_to_string(&target).unwrap(), "external edit\n");
    }

    #[test]
    fn write_target_if_unchanged_should_reject_replaced_file() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        let replacement = temp.path().join("AGENTS.md.external");
        fs::write(&target, "before\n").expect("target should be seeded");
        let snapshot = target_snapshot(&target).expect("snapshot should be readable");
        fs::write(&replacement, "external edit\n").expect("replacement should be written");
        fs::rename(&replacement, &target).expect("replacement should be installed");

        let error = write_target_if_unchanged(&snapshot, "dalo output\n")
            .expect_err("replaced target should block replacement");
        assert!(matches!(error, DaloError::InstructionTargetChanged { .. }));
        assert_eq!(fs::read_to_string(&target).unwrap(), "external edit\n");
    }

    #[test]
    fn rollback_should_not_restore_over_newer_external_content() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        fs::write(&target, "before\n").expect("target should be seeded");
        let snapshot = target_snapshot(&target).expect("snapshot should be readable");
        write_target(&target, "dalo output\n").expect("dalo output should be written");
        let written_identity = target_snapshot(&target)
            .expect("written target should be readable")
            .identity
            .expect("written target should have an identity");
        fs::write(&target, "newer external edit\n").expect("target should be edited");

        let error = restore_target_after_error(
            snapshot,
            "dalo output\n",
            written_identity,
            DaloError::Io(std::io::Error::other("lock write failed")),
        );

        assert!(
            error
                .to_string()
                .contains("newer content was left untouched")
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "newer external edit\n"
        );
    }

    #[test]
    fn rollback_should_not_restore_over_replaced_file_with_same_content() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        let replacement = temp.path().join("AGENTS.md.external");
        fs::write(&target, "before\n").expect("target should be seeded");
        let snapshot = target_snapshot(&target).expect("snapshot should be readable");
        write_target(&target, "dalo output\n").expect("dalo output should be written");
        let written_identity = target_snapshot(&target)
            .expect("written target should be readable")
            .identity
            .expect("written target should have an identity");
        fs::write(&replacement, "dalo output\n").expect("replacement should be written");
        fs::rename(&replacement, &target).expect("replacement should be installed");

        let error = restore_target_after_error(
            snapshot,
            "dalo output\n",
            written_identity,
            DaloError::Io(std::io::Error::other("lock write failed")),
        );

        assert!(
            error
                .to_string()
                .contains("newer content was left untouched")
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "dalo output\n");
    }

    #[test]
    fn parse_version_should_read_leading_version_line() {
        assert_eq!(
            parse_version("version: 1.2.0\n\n# Body\n"),
            Some("1.2.0".to_owned())
        );
        assert_eq!(parse_version("# Body only\n"), None);
    }

    #[test]
    fn is_valid_pack_id_should_reject_traversal() {
        assert!(is_valid_pack_id("house-style"));
        assert!(!is_valid_pack_id(".."));
        assert!(!is_valid_pack_id("bad/slash"));
        assert!(!is_valid_pack_id(""));
    }

    #[test]
    fn parse_topics_should_split_comma_separated_tags() {
        assert_eq!(
            parse_topics("topics: review, formatting, git\n\n# Body\n"),
            vec!["review", "formatting", "git"]
        );
        assert_eq!(parse_topics("tags: a,b\n"), vec!["a", "b"]);
        assert!(parse_topics("# No topics\n").is_empty());
    }

    fn discovered(id: &str, source: &str, topics: &[&str], enabled: bool) -> DiscoveredPack {
        DiscoveredPack {
            id: id.to_owned(),
            source_id: source.to_owned(),
            version: None,
            topics: topics.iter().map(|topic| (*topic).to_owned()).collect(),
            enabled,
        }
    }

    #[test]
    fn topic_overlaps_should_name_both_packs_sharing_a_topic() {
        let active = vec![
            discovered("style", "local", &["formatting", "tone"], true),
            discovered("format", "team", &["formatting"], true),
        ];
        let overlaps = topic_overlaps(&active);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(
            overlaps[0].packs,
            ["local:style".to_owned(), "team:format".to_owned()]
        );
        assert_eq!(overlaps[0].topics, vec!["formatting".to_owned()]);
    }

    #[test]
    fn topic_overlaps_should_ignore_disjoint_topics() {
        let active = vec![
            discovered("a", "local", &["security"], true),
            discovered("b", "team", &["formatting"], true),
        ];
        assert!(topic_overlaps(&active).is_empty());
    }

    #[test]
    fn discover_packs_should_find_local_packs_and_mark_enabled() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let paths = StorePaths::new(temp.path().to_path_buf());
        fs::create_dir_all(&paths.local_instructions_dir).expect("dir should be created");
        fs::write(
            paths.local_instructions_dir.join("house.md"),
            "topics: x\n\nBody\n",
        )
        .expect("pack should be written");
        let active = vec![LockedInstructionPack {
            pack_id: "house".to_owned(),
            target: PathBuf::from("/tmp/AGENTS.md"),
            logical_targets: Vec::new(),
            source_id: "local".to_owned(),
            commit: None,
            version: None,
        }];

        let packs = discover_packs(&paths, &[], &active);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "house");
        assert!(packs[0].enabled);
        assert_eq!(packs[0].topics, vec!["x".to_owned()]);
    }

    #[test]
    fn read_target_should_treat_missing_file_as_empty_only() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let missing = temp.path().join("AGENTS.md");

        assert_eq!(
            read_target(&missing).expect("missing target should read as empty"),
            ""
        );
    }

    #[test]
    fn write_target_should_replace_file_via_temp_rename() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        fs::write(&target, "old\n").expect("target should be seeded");
        let before_inode = fs::metadata(&target)
            .expect("target metadata should be readable")
            .ino();

        write_target(&target, "new\n").expect("target should be written");

        assert_eq!(
            fs::read_to_string(&target).expect("target should be readable"),
            "new\n"
        );
        let after_inode = fs::metadata(&target)
            .expect("target metadata should be readable")
            .ino();
        assert_ne!(before_inode, after_inode);
        assert_eq!(
            fs::read_dir(temp.path())
                .expect("parent dir should be readable")
                .count(),
            1
        );
    }

    #[test]
    fn write_target_should_preserve_existing_permissions() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        fs::write(&target, "old\n").expect("target should be seeded");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644))
            .expect("permissions should be set");

        write_target(&target, "new\n").expect("target should be written");

        let mode = fs::metadata(&target)
            .expect("target metadata should be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn write_target_should_write_through_symlinked_target() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let canonical_target = temp.path().join("AGENTS.md");
        let symlink_target = temp.path().join("CLAUDE.md");
        fs::write(&canonical_target, "old\n").expect("target should be seeded");
        symlink(&canonical_target, &symlink_target).expect("symlink should be created");

        write_target(&symlink_target, "new\n").expect("target should be written");

        assert!(
            fs::symlink_metadata(&symlink_target)
                .expect("symlink metadata should be readable")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(&canonical_target).expect("canonical target should be readable"),
            "new\n"
        );
    }

    fn assert_body_marker_error(error: DaloError, pack_id: &str) {
        let DaloError::MalformedInstructionBlock {
            pack_id: actual,
            reason,
        } = error
        else {
            panic!("expected malformed instruction block error");
        };
        assert_eq!(actual, pack_id);
        assert!(reason.contains("body contains dalo managed-block marker"));
    }
}
