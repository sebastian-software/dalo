//! Source lock and resolved user lock schemas.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::materialize::SyncReport;
use crate::resolver::{Resolution, UnlinkedReason};
use crate::source::{SourceConfig, SourceKind};

/// Current persisted user-lock schema version.
pub const USER_LOCK_SCHEMA_VERSION: u32 = 1;

/// Resolved user lock.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserLock {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Source commit snapshots known to the lock.
    #[serde(default)]
    pub sources: Vec<LockedSource>,
    /// Active skill records.
    #[serde(default)]
    pub active_skills: Vec<LockedSkill>,
    /// Would-be active skills that require local approval.
    #[serde(default)]
    pub pending_approval_skills: Vec<LockedSkill>,
    /// Managed skills present in the store but not linked into targets.
    #[serde(default)]
    pub unlinked_skills: Vec<LockedSkill>,
    /// Target materialization summary from the last successful sync.
    #[serde(default)]
    pub target_materializations: Vec<LockedTargetMaterialization>,
    /// Active instruction packs rendered into instruction-file targets.
    #[serde(default)]
    pub active_instruction_packs: Vec<LockedInstructionPack>,
}

/// Source identity captured in the user lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSource {
    /// Source ID.
    pub id: String,
    /// Source kind.
    pub kind: SourceKind,
    /// Local checkout path.
    pub path: PathBuf,
    /// Optional commit ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// Skill identity captured in the user lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSkill {
    /// Source-qualified ref.
    pub source_ref: String,
    /// Slot name.
    pub slot_name: String,
    /// Optional stable skill ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Source ID.
    pub source_id: String,
    /// Source kind.
    pub source_kind: SourceKind,
    /// Optional reason when the skill is unlinked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One materialized target slot captured in the user lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedTargetMaterialization {
    /// Target slot path.
    pub link_path: PathBuf,
    /// Desired store path for this slot, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_path: Option<PathBuf>,
    /// Materialization operation kind.
    pub kind: String,
    /// Materialization operation status.
    pub status: String,
    /// Optional operation reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One active instruction pack captured in the user lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedInstructionPack {
    /// Pack ID.
    pub pack_id: String,
    /// Instruction-file target the block was rendered into.
    pub target: PathBuf,
    /// Source ID the pack came from.
    pub source_id: String,
    /// Source commit the pack was rendered from, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Pack version, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Drift between the previous lock and the current live resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LockDrift {
    /// Machine-readable drift code.
    pub code: LockDriftCode,
    /// Affected source, skill, or target subject.
    pub subject: String,
    /// Human-readable explanation.
    pub message: String,
}

/// User-lock drift code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockDriftCode {
    /// A source commit changed since the last lock write.
    SourceCommitChanged,
    /// A source disappeared from the live config.
    SourceRemoved,
    /// A source appeared in the live config.
    SourceAdded,
    /// A previously active skill is no longer active.
    ActiveRemoved,
    /// A skill is now active but was not active in the lock.
    ActiveAdded,
    /// A previously unlinked skill is no longer unlinked.
    UnlinkedRemoved,
    /// A skill is now unlinked but was not unlinked in the lock.
    UnlinkedAdded,
    /// A previously pending skill is no longer pending approval.
    PendingApprovalRemoved,
    /// A skill is now pending approval but was not pending in the lock.
    PendingApprovalAdded,
}

impl UserLock {
    /// Empty lock for a new store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: USER_LOCK_SCHEMA_VERSION,
            sources: Vec::new(),
            active_skills: Vec::new(),
            pending_approval_skills: Vec::new(),
            unlinked_skills: Vec::new(),
            target_materializations: Vec::new(),
            active_instruction_packs: Vec::new(),
        }
    }
}

/// Build a stable resolved user lock from the current source config and sync result.
#[must_use]
pub fn build_user_lock(
    sources: &[SourceConfig],
    resolution: &Resolution,
    sync_report: Option<&SyncReport>,
) -> UserLock {
    let mut lock = UserLock {
        schema_version: USER_LOCK_SCHEMA_VERSION,
        sources: locked_sources(sources),
        active_skills: resolution
            .active_skills
            .iter()
            .map(|skill| LockedSkill {
                source_ref: skill.source_ref.clone(),
                slot_name: skill.slot_name.clone(),
                id: skill.id.clone(),
                source_id: skill.source_id.clone(),
                source_kind: skill.source_kind,
                reason: None,
            })
            .collect(),
        pending_approval_skills: resolution
            .pending_approval_skills
            .iter()
            .map(|skill| LockedSkill {
                source_ref: skill.source_ref.clone(),
                slot_name: skill.slot_name.clone(),
                id: skill.id.clone(),
                source_id: skill.source_id.clone(),
                source_kind: skill.source_kind,
                reason: Some("pending_approval".to_owned()),
            })
            .collect(),
        unlinked_skills: resolution
            .unlinked_skills
            .iter()
            .map(|unlinked| LockedSkill {
                source_ref: unlinked.skill.source_ref.clone(),
                slot_name: unlinked.skill.slot_name.clone(),
                id: unlinked.skill.id.clone(),
                source_id: unlinked.skill.source_id.clone(),
                source_kind: unlinked.skill.source_kind,
                reason: Some(unlinked_reason_name(unlinked.reason).to_owned()),
            })
            .collect(),
        target_materializations: sync_report.map_or_else(Vec::new, |report| {
            report
                .operations
                .iter()
                .map(|operation| LockedTargetMaterialization {
                    link_path: operation.link_path.clone(),
                    desired_path: operation.desired_path.clone(),
                    kind: operation.kind.as_str().to_owned(),
                    status: operation.status.as_str().to_owned(),
                    reason: operation.reason.clone(),
                })
                .collect()
        }),
        // Instruction packs are managed by the `instructions` command, not by sync;
        // the caller restores the previous lock's packs after rebuilding.
        active_instruction_packs: Vec::new(),
    };
    sort_user_lock(&mut lock);
    lock
}

/// Compare a previous user lock with a live lock preview.
#[must_use]
pub fn compare_user_lock(previous: &UserLock, current: &UserLock) -> Vec<LockDrift> {
    let mut drift = Vec::new();

    compare_sources(previous, current, &mut drift);
    compare_skill_refs(
        LockDriftCode::ActiveRemoved,
        LockDriftCode::ActiveAdded,
        "active skill",
        &previous.active_skills,
        &current.active_skills,
        &mut drift,
    );
    compare_skill_refs(
        LockDriftCode::UnlinkedRemoved,
        LockDriftCode::UnlinkedAdded,
        "unlinked skill",
        &previous.unlinked_skills,
        &current.unlinked_skills,
        &mut drift,
    );
    compare_skill_refs(
        LockDriftCode::PendingApprovalRemoved,
        LockDriftCode::PendingApprovalAdded,
        "pending approval skill",
        &previous.pending_approval_skills,
        &current.pending_approval_skills,
        &mut drift,
    );

    drift.sort_by(|left, right| {
        drift_code_name(left.code)
            .cmp(drift_code_name(right.code))
            .then_with(|| left.subject.cmp(&right.subject))
    });
    drift
}

fn locked_sources(sources: &[SourceConfig]) -> Vec<LockedSource> {
    let mut locked = sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| LockedSource {
            id: source.id.clone(),
            kind: source.kind,
            path: source.path.clone(),
            commit: git::rev_parse_head(&source.path).ok(),
        })
        .collect::<Vec<_>>();
    locked.sort_by(|left, right| left.id.cmp(&right.id));
    locked
}

fn sort_user_lock(lock: &mut UserLock) {
    lock.sources.sort_by(|left, right| left.id.cmp(&right.id));
    sort_locked_skills(&mut lock.active_skills);
    sort_locked_skills(&mut lock.pending_approval_skills);
    sort_locked_skills(&mut lock.unlinked_skills);
    lock.target_materializations.sort_by(|left, right| {
        left.link_path
            .cmp(&right.link_path)
            .then_with(|| left.kind.cmp(&right.kind))
    });
}

fn sort_locked_skills(skills: &mut [LockedSkill]) {
    skills.sort_by(|left, right| {
        left.slot_name
            .cmp(&right.slot_name)
            .then_with(|| left.source_ref.cmp(&right.source_ref))
    });
}

fn compare_sources(previous: &UserLock, current: &UserLock, drift: &mut Vec<LockDrift>) {
    let previous_sources = previous
        .sources
        .iter()
        .map(|source| (source.id.as_str(), source.commit.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let current_sources = current
        .sources
        .iter()
        .map(|source| (source.id.as_str(), source.commit.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let source_ids = previous_sources
        .keys()
        .chain(current_sources.keys())
        .copied()
        .collect::<BTreeSet<_>>();

    for source_id in source_ids {
        match (
            previous_sources.get(source_id),
            current_sources.get(source_id),
        ) {
            (Some(previous_commit), Some(current_commit)) if previous_commit != current_commit => {
                drift.push(LockDrift {
                    code: LockDriftCode::SourceCommitChanged,
                    subject: source_id.to_owned(),
                    message: format!("source `{source_id}` commit differs from lock"),
                });
            }
            (Some(_), None) => drift.push(LockDrift {
                code: LockDriftCode::SourceRemoved,
                subject: source_id.to_owned(),
                message: format!("source `{source_id}` is no longer configured"),
            }),
            (None, Some(_)) => drift.push(LockDrift {
                code: LockDriftCode::SourceAdded,
                subject: source_id.to_owned(),
                message: format!("source `{source_id}` is not present in the lock"),
            }),
            _ => {}
        }
    }
}

fn compare_skill_refs(
    removed_code: LockDriftCode,
    added_code: LockDriftCode,
    label: &str,
    previous: &[LockedSkill],
    current: &[LockedSkill],
    drift: &mut Vec<LockDrift>,
) {
    let previous_refs = previous
        .iter()
        .map(|skill| skill.source_ref.as_str())
        .collect::<BTreeSet<_>>();
    let current_refs = current
        .iter()
        .map(|skill| skill.source_ref.as_str())
        .collect::<BTreeSet<_>>();

    for source_ref in previous_refs.difference(&current_refs) {
        drift.push(LockDrift {
            code: removed_code,
            subject: (*source_ref).to_owned(),
            message: format!("{label} `{source_ref}` is no longer in the live resolution"),
        });
    }
    for source_ref in current_refs.difference(&previous_refs) {
        drift.push(LockDrift {
            code: added_code,
            subject: (*source_ref).to_owned(),
            message: format!("{label} `{source_ref}` is not present in the lock"),
        });
    }
}

fn unlinked_reason_name(reason: UnlinkedReason) -> &'static str {
    match reason {
        UnlinkedReason::Shadowed => "shadowed",
    }
}

fn drift_code_name(code: LockDriftCode) -> &'static str {
    match code {
        LockDriftCode::SourceCommitChanged => "source_commit_changed",
        LockDriftCode::SourceRemoved => "source_removed",
        LockDriftCode::SourceAdded => "source_added",
        LockDriftCode::ActiveRemoved => "active_removed",
        LockDriftCode::ActiveAdded => "active_added",
        LockDriftCode::UnlinkedRemoved => "unlinked_removed",
        LockDriftCode::UnlinkedAdded => "unlinked_added",
        LockDriftCode::PendingApprovalRemoved => "pending_approval_removed",
        LockDriftCode::PendingApprovalAdded => "pending_approval_added",
    }
}

impl std::fmt::Display for LockDriftCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(drift_code_name(*self))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::materialize::{
        MaterializeOperation, MaterializeOperationKind, MaterializeOperationStatus,
    };
    use crate::resolver::{ResolvedSkill, UnlinkedSkill, UnlinkedStatus};

    #[test]
    fn build_user_lock_should_sort_skills_and_materializations() {
        let resolution = Resolution {
            active_skills: vec![skill("team:b", "b"), skill("team:a", "a")],
            pending_approval_skills: Vec::new(),
            blocked_skills: Vec::new(),
            unlinked_skills: vec![UnlinkedSkill {
                skill: skill("team:c", "c"),
                status: UnlinkedStatus::Unlinked,
                reason: UnlinkedReason::Shadowed,
                shadowed_by: "team:a".to_owned(),
            }],
            diagnostics: Vec::new(),
        };
        let report = SyncReport {
            store: PathBuf::from("/store"),
            dry_run: false,
            operations: vec![operation("/target/b"), operation("/target/a")],
            resolution: resolution.clone(),
            degraded_sources: Vec::new(),
        };

        let lock = build_user_lock(&[], &resolution, Some(&report));

        assert_eq!(lock.active_skills[0].source_ref, "team:a");
        assert_eq!(
            lock.target_materializations[0].link_path,
            PathBuf::from("/target/a")
        );
        assert_eq!(lock.unlinked_skills[0].reason.as_deref(), Some("shadowed"));
    }

    #[test]
    fn compare_user_lock_should_report_active_drift() {
        let previous = UserLock {
            active_skills: vec![locked_skill("local:review")],
            ..UserLock::empty()
        };
        let current = UserLock {
            active_skills: vec![locked_skill("local:test")],
            ..UserLock::empty()
        };

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::ActiveRemoved)
        );
        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::ActiveAdded)
        );
    }

    #[test]
    fn compare_sources_should_report_source_commit_changed_when_commit_differs() {
        let previous = UserLock {
            sources: vec![locked_source("company", Some("aaaa"))],
            ..UserLock::empty()
        };
        let current = UserLock {
            sources: vec![locked_source("company", Some("bbbb"))],
            ..UserLock::empty()
        };

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::SourceCommitChanged
                    && entry.subject == "company")
        );
    }

    #[test]
    fn compare_sources_should_report_source_removed_when_source_disappears() {
        let previous = UserLock {
            sources: vec![locked_source("company", Some("aaaa"))],
            ..UserLock::empty()
        };
        let current = UserLock::empty();

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::SourceRemoved
                    && entry.subject == "company")
        );
    }

    #[test]
    fn compare_sources_should_report_source_added_when_source_appears() {
        let previous = UserLock::empty();
        let current = UserLock {
            sources: vec![locked_source("company", Some("aaaa"))],
            ..UserLock::empty()
        };

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::SourceAdded
                    && entry.subject == "company")
        );
    }

    #[test]
    fn compare_user_lock_should_report_unlinked_added_when_skill_becomes_unlinked() {
        let previous = UserLock::empty();
        let current = UserLock {
            unlinked_skills: vec![locked_skill("company:review")],
            ..UserLock::empty()
        };

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::UnlinkedAdded
                    && entry.subject == "company:review")
        );
    }

    #[test]
    fn compare_user_lock_should_report_unlinked_removed_when_skill_stops_being_unlinked() {
        let previous = UserLock {
            unlinked_skills: vec![locked_skill("company:review")],
            ..UserLock::empty()
        };
        let current = UserLock::empty();

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::UnlinkedRemoved
                    && entry.subject == "company:review")
        );
    }

    #[test]
    fn compare_user_lock_should_report_pending_approval_added_when_skill_becomes_pending() {
        let previous = UserLock::empty();
        let current = UserLock {
            pending_approval_skills: vec![locked_skill("company:review")],
            ..UserLock::empty()
        };

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::PendingApprovalAdded
                    && entry.subject == "company:review")
        );
    }

    #[test]
    fn compare_user_lock_should_report_pending_approval_removed_when_skill_stops_being_pending() {
        let previous = UserLock {
            pending_approval_skills: vec![locked_skill("company:review")],
            ..UserLock::empty()
        };
        let current = UserLock::empty();

        let drift = compare_user_lock(&previous, &current);

        assert!(
            drift
                .iter()
                .any(|entry| entry.code == LockDriftCode::PendingApprovalRemoved
                    && entry.subject == "company:review")
        );
    }

    fn skill(source_ref: &str, slot_name: &str) -> ResolvedSkill {
        ResolvedSkill {
            source_ref: source_ref.to_owned(),
            slot_name: slot_name.to_owned(),
            id: None,
            source_id: "team".to_owned(),
            source_kind: SourceKind::Team,
            source_priority: 10,
            path: PathBuf::from(format!("/store/{slot_name}")),
            local_override: false,
        }
    }

    fn operation(link_path: &str) -> MaterializeOperation {
        MaterializeOperation {
            kind: MaterializeOperationKind::Create,
            link_path: PathBuf::from(link_path),
            desired_path: Some(PathBuf::from("/store/skill")),
            status: MaterializeOperationStatus::Applied,
            reason: None,
        }
    }

    fn locked_skill(source_ref: &str) -> LockedSkill {
        LockedSkill {
            source_ref: source_ref.to_owned(),
            slot_name: source_ref
                .split_once(':')
                .map_or(source_ref, |(_, slot_name)| slot_name)
                .to_owned(),
            id: None,
            source_id: "local".to_owned(),
            source_kind: SourceKind::Local,
            reason: None,
        }
    }

    fn locked_source(id: &str, commit: Option<&str>) -> LockedSource {
        LockedSource {
            id: id.to_owned(),
            kind: SourceKind::Team,
            path: PathBuf::from(format!("/store/sources/{id}")),
            commit: commit.map(str::to_owned),
        }
    }
}
