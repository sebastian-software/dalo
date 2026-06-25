//! Deterministic source resolution.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::config::UserConfig;
use crate::inventory::{self, SourceInventory};
use crate::source::{SourceConfig, SourceKind};
use crate::store::ApprovalRecord;

/// Resolver input.
#[derive(Debug, Clone)]
pub struct ResolutionInput<'a> {
    /// Enabled sources.
    pub sources: &'a [SourceConfig],
    /// Scanned inventories.
    pub inventories: Vec<SourceInventory>,
    /// Local approval records.
    pub approvals: Vec<ApprovalRecord>,
}

/// Outcome of scanning one enabled source for the live resolve pipeline.
#[derive(Debug, Clone)]
pub struct SourceScan {
    /// Scanned source.
    pub source: SourceConfig,
    /// Successful inventory, or `None` when the scan failed.
    pub inventory: Option<SourceInventory>,
    /// Scan error message when the scan failed.
    pub error: Option<String>,
}

/// Live resolve pipeline result shared by `status` and `doctor`.
///
/// Holds the per-source scan outcomes so callers can render per-source detail
/// without re-scanning, plus the resolution computed from those inventories.
#[derive(Debug, Clone)]
pub struct LiveResolution {
    /// Per-source scan outcomes for enabled sources, in config order.
    pub scans: Vec<SourceScan>,
    /// Resolution computed from the successful inventories.
    pub resolution: Resolution,
}

/// Final resolution result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolution {
    /// Active skills that should be visible to targets.
    pub active_skills: Vec<ResolvedSkill>,
    /// Would-be winners that need local approval before becoming active.
    pub pending_approval_skills: Vec<ResolvedSkill>,
    /// Managed skills not linked because another skill won the slot.
    pub unlinked_skills: Vec<UnlinkedSkill>,
    /// Resolver diagnostics.
    pub diagnostics: Vec<ResolutionDiagnostic>,
}

/// Active or pending resolved skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedSkill {
    /// Source-qualified ref.
    pub source_ref: String,
    /// Slot name.
    pub slot_name: String,
    /// Optional stable ID.
    pub id: Option<String>,
    /// Source ID.
    pub source_id: String,
    /// Source kind.
    pub source_kind: SourceKind,
    /// Source priority.
    pub source_priority: i32,
    /// Skill directory path.
    pub path: PathBuf,
    /// Whether this is a local override over another source.
    pub local_override: bool,
}

/// Unlinked managed skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnlinkedSkill {
    /// Skill that was not linked.
    pub skill: ResolvedSkill,
    /// User-facing state.
    pub status: UnlinkedStatus,
    /// Machine reason.
    pub reason: UnlinkedReason,
    /// Winning skill that caused this skill to be unlinked.
    pub shadowed_by: String,
}

/// User-facing unlinked state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlinkedStatus {
    /// Skill exists in the store but is not linked.
    Unlinked,
}

/// Internal unlinked reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlinkedReason {
    /// Another managed source won the same slot name.
    Shadowed,
}

/// Resolver diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolutionDiagnostic {
    /// Diagnostic code.
    pub code: ResolutionDiagnosticCode,
    /// Human-readable message.
    pub message: String,
    /// Related source-qualified ref when available.
    pub source_ref: Option<String>,
}

/// Resolver diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionDiagnosticCode {
    /// A higher-priority candidate is waiting for approval.
    PendingApproval,
    /// A local skill overrides another managed source.
    LocalOverride,
    /// A managed skill is shadowed by another managed skill.
    Shadowed,
}

#[derive(Debug, Clone)]
struct Candidate {
    skill: ResolvedSkill,
    owners: Vec<String>,
    trusted: bool,
}

/// Scan every enabled source and resolve it into a deterministic skill set.
///
/// Shared by `status` and `doctor` so the scan-then-resolve pipeline lives in
/// one place. Scan failures are captured per source rather than aborting, so
/// callers can surface them as diagnostics.
pub fn resolve_from_config(config: &UserConfig, approvals: Vec<ApprovalRecord>) -> LiveResolution {
    let enabled = config
        .sources
        .iter()
        .filter(|source| source.enabled)
        .cloned()
        .collect::<Vec<_>>();

    let mut scans = Vec::with_capacity(enabled.len());
    let mut inventories = Vec::new();
    for source in &enabled {
        match scan_enabled_source(source) {
            Ok(inventory) => {
                inventories.push(inventory.clone());
                scans.push(SourceScan {
                    source: source.clone(),
                    inventory: Some(inventory),
                    error: None,
                });
            }
            Err(error) => scans.push(SourceScan {
                source: source.clone(),
                inventory: None,
                error: Some(error),
            }),
        }
    }

    let resolution = resolve(&ResolutionInput {
        sources: &enabled,
        inventories,
        approvals,
    });

    LiveResolution { scans, resolution }
}

/// Scan one enabled source, returning a human-readable error on failure.
fn scan_enabled_source(source: &SourceConfig) -> Result<SourceInventory, String> {
    if !source.path.exists() {
        return Err("source path does not exist".to_owned());
    }
    inventory::scan_source(&source.id, &source.path).map_err(|error| error.to_string())
}

/// Resolve active sources and inventories into a deterministic skill set.
#[must_use]
pub fn resolve(input: &ResolutionInput) -> Resolution {
    let source_by_id = input
        .sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| (source.id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = Vec::new();

    for inventory in &input.inventories {
        let Some(source) = source_by_id.get(inventory.source_id.as_str()) else {
            continue;
        };

        for skill in &inventory.skills {
            candidates.push(Candidate {
                skill: ResolvedSkill {
                    source_ref: skill.source_ref.clone(),
                    slot_name: skill.slot_name.clone(),
                    id: skill.id.clone(),
                    source_id: source.id.clone(),
                    source_kind: source.kind,
                    source_priority: source.priority,
                    path: skill.path.clone(),
                    local_override: false,
                },
                owners: skill.owners.clone(),
                trusted: source.trusted,
            });
        }
    }

    candidates.sort_by(|left, right| {
        left.skill
            .source_priority
            .cmp(&right.skill.source_priority)
            .then_with(|| left.skill.source_id.cmp(&right.skill.source_id))
            .then_with(|| left.skill.source_ref.cmp(&right.skill.source_ref))
    });

    let mut groups: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates {
        // The slot name keys the group and also lives on the candidate; take a
        // single owned key up front so the candidate moves into the bucket
        // unchanged without a second clone in the common (existing-key) path.
        let slot_name = candidate.skill.slot_name.clone();
        groups.entry(slot_name).or_default().push(candidate);
    }

    let mut active_skills = Vec::new();
    let mut pending_approval_skills = Vec::new();
    let mut unlinked_skills = Vec::new();
    let mut diagnostics = Vec::new();

    for group in groups.into_values() {
        let Some(winner_index) = group
            .iter()
            .position(|candidate| is_approved(candidate, &input.approvals))
        else {
            if let Some(candidate) = group.first() {
                pending_approval_skills.push(candidate.skill.clone());
                diagnostics.push(ResolutionDiagnostic {
                    code: ResolutionDiagnosticCode::PendingApproval,
                    message: format!("skill `{}` is pending approval", candidate.skill.source_ref),
                    source_ref: Some(candidate.skill.source_ref.clone()),
                });
            }
            continue;
        };

        for candidate in group.iter().take(winner_index) {
            pending_approval_skills.push(candidate.skill.clone());
            diagnostics.push(ResolutionDiagnostic {
                code: ResolutionDiagnosticCode::PendingApproval,
                message: format!("skill `{}` is pending approval", candidate.skill.source_ref),
                source_ref: Some(candidate.skill.source_ref.clone()),
            });
        }

        let mut winner = group[winner_index].skill.clone();
        winner.local_override = winner.source_kind == SourceKind::Local && group.len() > 1;
        if winner.local_override {
            diagnostics.push(ResolutionDiagnostic {
                code: ResolutionDiagnosticCode::LocalOverride,
                message: format!(
                    "local skill `{}` overrides another source",
                    winner.source_ref
                ),
                source_ref: Some(winner.source_ref.clone()),
            });
        }

        for candidate in group.iter().skip(winner_index + 1) {
            unlinked_skills.push(UnlinkedSkill {
                skill: candidate.skill.clone(),
                status: UnlinkedStatus::Unlinked,
                reason: UnlinkedReason::Shadowed,
                shadowed_by: winner.source_ref.clone(),
            });
            diagnostics.push(ResolutionDiagnostic {
                code: ResolutionDiagnosticCode::Shadowed,
                message: format!(
                    "skill `{}` is unlinked because `{}` wins the same slot",
                    candidate.skill.source_ref, winner.source_ref
                ),
                source_ref: Some(candidate.skill.source_ref.clone()),
            });
        }

        active_skills.push(winner);
    }

    active_skills.sort_by(|left, right| {
        left.slot_name
            .cmp(&right.slot_name)
            .then_with(|| left.source_ref.cmp(&right.source_ref))
    });
    pending_approval_skills.sort_by(|left, right| {
        left.slot_name
            .cmp(&right.slot_name)
            .then_with(|| left.source_ref.cmp(&right.source_ref))
    });
    unlinked_skills.sort_by(|left, right| {
        left.skill
            .slot_name
            .cmp(&right.skill.slot_name)
            .then_with(|| left.skill.source_ref.cmp(&right.skill.source_ref))
    });
    diagnostics.sort_by(|left, right| {
        diagnostic_code_name(left.code)
            .cmp(diagnostic_code_name(right.code))
            .then_with(|| left.source_ref.cmp(&right.source_ref))
    });

    Resolution {
        active_skills,
        pending_approval_skills,
        unlinked_skills,
        diagnostics,
    }
}

fn is_approved(candidate: &Candidate, approvals: &[ApprovalRecord]) -> bool {
    // Local skills and skills from a source the user marked trusted are always
    // approved; everything else needs an explicit approval record.
    if candidate.skill.source_kind == SourceKind::Local || candidate.trusted {
        return true;
    }

    approvals.iter().any(|approval| {
        approval_matches_skill(approval, candidate)
            || approval_matches_source(approval, candidate)
            || approval_matches_owner(approval, candidate)
    })
}

fn approval_matches_skill(approval: &ApprovalRecord, candidate: &Candidate) -> bool {
    approval.scope == "skill"
        && (approval.value == candidate.skill.source_ref
            || approval.value == candidate.skill.slot_name
            || candidate.skill.id.as_ref() == Some(&approval.value))
}

fn approval_matches_source(approval: &ApprovalRecord, candidate: &Candidate) -> bool {
    approval.scope == "source" && approval.value == candidate.skill.source_id
}

fn approval_matches_owner(approval: &ApprovalRecord, candidate: &Candidate) -> bool {
    matches!(approval.scope.as_str(), "author" | "org")
        && candidate
            .owners
            .iter()
            .any(|owner| owner == &approval.value)
}

fn diagnostic_code_name(code: ResolutionDiagnosticCode) -> &'static str {
    match code {
        ResolutionDiagnosticCode::PendingApproval => "pending_approval",
        ResolutionDiagnosticCode::LocalOverride => "local_override",
        ResolutionDiagnosticCode::Shadowed => "shadowed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{SkillRecord, SourceInventory};

    #[test]
    fn resolve_should_pick_lower_priority_skill_for_same_slot() {
        let resolution = resolve_with(
            vec![
                source("team-a", SourceKind::Team, 10),
                source("team-b", SourceKind::Team, 20),
            ],
            vec![
                inventory("team-a", vec![skill("team-a", "copy-editing")]),
                inventory("team-b", vec![skill("team-b", "copy-editing")]),
            ],
            vec![approval("source", "team-a"), approval("source", "team-b")],
        );

        assert_eq!(
            resolution.active_skills[0].source_ref,
            "team-a:copy-editing"
        );
    }

    #[test]
    fn resolve_should_tie_break_equal_priority_by_source_id() {
        let resolution = resolve_with(
            vec![
                source("z-team", SourceKind::Team, 10),
                source("a-team", SourceKind::Team, 10),
            ],
            vec![
                inventory("z-team", vec![skill("z-team", "copy-editing")]),
                inventory("a-team", vec![skill("a-team", "copy-editing")]),
            ],
            vec![approval("source", "z-team"), approval("source", "a-team")],
        );

        assert_eq!(
            resolution.active_skills[0].source_ref,
            "a-team:copy-editing"
        );
    }

    #[test]
    fn resolve_should_mark_local_winner_as_local_override() {
        let resolution = resolve_with(
            vec![
                source("local", SourceKind::Local, 0),
                source("company", SourceKind::Team, 10),
            ],
            vec![
                inventory("local", vec![skill("local", "review")]),
                inventory("company", vec![skill("company", "review")]),
            ],
            vec![approval("source", "company")],
        );

        assert!(resolution.active_skills[0].local_override);
    }

    #[test]
    fn resolve_should_keep_lower_priority_approved_skill_when_winner_is_unapproved() {
        let resolution = resolve_with(
            vec![
                source("new-team", SourceKind::Team, 0),
                source("old-team", SourceKind::Team, 10),
            ],
            vec![
                inventory("new-team", vec![skill("new-team", "review")]),
                inventory("old-team", vec![skill("old-team", "review")]),
            ],
            vec![approval("source", "old-team")],
        );

        assert_eq!(resolution.active_skills[0].source_ref, "old-team:review");
    }

    #[test]
    fn resolve_should_match_author_approval_against_owners() {
        let mut team_skill = skill("company", "review");
        team_skill.owners = vec!["core-team".to_owned()];
        let resolution = resolve_with(
            vec![source("company", SourceKind::Team, 10)],
            vec![inventory("company", vec![team_skill])],
            vec![approval("author", "core-team")],
        );

        assert_eq!(resolution.active_skills[0].source_ref, "company:review");
    }

    #[test]
    fn resolve_should_hold_unapproved_team_skill_as_pending() {
        let resolution = resolve_with(
            vec![source("company", SourceKind::Team, 10)],
            vec![inventory("company", vec![skill("company", "review")])],
            Vec::new(),
        );

        assert!(resolution.active_skills.is_empty());
    }

    #[test]
    fn resolve_should_list_unapproved_team_skill_in_pending_approval() {
        let resolution = resolve_with(
            vec![source("company", SourceKind::Team, 10)],
            vec![inventory("company", vec![skill("company", "review")])],
            Vec::new(),
        );

        assert_eq!(
            resolution.pending_approval_skills[0].source_ref,
            "company:review"
        );
    }

    #[test]
    fn resolve_should_approve_trusted_source_without_record() {
        let mut trusted = source("company", SourceKind::Team, 10);
        trusted.trusted = true;
        let resolution = resolve_with(
            vec![trusted],
            vec![inventory("company", vec![skill("company", "review")])],
            Vec::new(),
        );

        assert_eq!(resolution.active_skills[0].source_ref, "company:review");
    }

    #[test]
    fn resolve_should_be_order_independent_across_input_permutations() {
        // Same-priority and distinct-priority sources, plus a shadowed slot, so the
        // resolver must lean on the full tie-break chain rather than input order.
        let sources = vec![
            source("z-team", SourceKind::Team, 10),
            source("a-team", SourceKind::Team, 10),
            source("c-team", SourceKind::Team, 5),
        ];
        let inventories = vec![
            inventory(
                "z-team",
                vec![skill("z-team", "review"), skill("z-team", "format")],
            ),
            inventory("a-team", vec![skill("a-team", "review")]),
            inventory("c-team", vec![skill("c-team", "format")]),
        ];
        let approvals = vec![
            approval("source", "z-team"),
            approval("source", "a-team"),
            approval("source", "c-team"),
        ];
        let baseline =
            resolve_with(sources.clone(), inventories.clone(), approvals.clone()).active_skills;

        let permuted = [
            (vec![2usize, 0, 1], vec![2usize, 1, 0]),
            (vec![1, 2, 0], vec![0, 2, 1]),
            (vec![2, 1, 0], vec![1, 0, 2]),
            (vec![0, 2, 1], vec![2, 0, 1]),
        ]
        .into_iter()
        .map(|(source_order, inventory_order)| {
            resolve_with(
                permute(&sources, &source_order),
                permute(&inventories, &inventory_order),
                approvals.clone(),
            )
            .active_skills
        })
        .collect::<Vec<_>>();

        assert!(permuted.iter().all(|active| *active == baseline));
    }

    fn permute<T: Clone>(items: &[T], order: &[usize]) -> Vec<T> {
        order.iter().map(|index| items[*index].clone()).collect()
    }

    fn resolve_with(
        sources: Vec<SourceConfig>,
        inventories: Vec<SourceInventory>,
        approvals: Vec<ApprovalRecord>,
    ) -> Resolution {
        resolve(&ResolutionInput {
            sources: &sources,
            inventories,
            approvals,
        })
    }

    fn source(id: &str, kind: SourceKind, priority: i32) -> SourceConfig {
        SourceConfig {
            id: id.to_owned(),
            kind,
            path: PathBuf::from(format!("/tmp/{id}")),
            priority,
            enabled: true,
            trusted: false,
            url: None,
            branch: None,
            update_policy: None,
        }
    }

    fn inventory(source_id: &str, skills: Vec<SkillRecord>) -> SourceInventory {
        SourceInventory {
            source_id: source_id.to_owned(),
            skills,
            warnings: Vec::new(),
        }
    }

    fn skill(source_id: &str, slot_name: &str) -> SkillRecord {
        SkillRecord {
            source_id: source_id.to_owned(),
            source_ref: format!("{source_id}:{slot_name}"),
            id: None,
            slot_name: slot_name.to_owned(),
            path: PathBuf::from(format!("/tmp/{source_id}/{slot_name}")),
            skill_file: PathBuf::from(format!("/tmp/{source_id}/{slot_name}/SKILL.md")),
            description: None,
            requires: Vec::new(),
            owners: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn approval(scope: &str, value: &str) -> ApprovalRecord {
        ApprovalRecord {
            scope: scope.to_owned(),
            value: value.to_owned(),
        }
    }
}
