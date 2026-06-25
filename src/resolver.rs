//! Deterministic source resolution.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

use serde::Serialize;

use crate::config::UserConfig;
use crate::inventory::{self, SkillRecord, SourceInventory};
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
    /// Skills held back because their required closure cannot be linked.
    pub blocked_skills: Vec<BlockedSkill>,
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

/// A skill held back from materialization because its required closure cannot be
/// linked consistently (RFC 0003 section 4 step 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockedSkill {
    /// The dependent skill held back.
    pub skill: ResolvedSkill,
    /// The required reference that could not be satisfied.
    pub requirement: String,
    /// Why the requirement could not be satisfied.
    pub reason: ClosureBlockReason,
}

/// Why a required-closure preflight blocked a dependent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosureBlockReason {
    /// The required ref exists in no enabled source.
    Missing,
    /// The required skill is a would-be winner still waiting for approval.
    PendingApproval,
    /// The required skill is shadowed by a non-equivalent winner of its slot.
    ShadowedNotSatisfied,
    /// A same-name entry already owned by the target blocks the required skill.
    ///
    /// The resolver itself has no target view, so this is reserved for the
    /// link-time preflight; it is part of the public reason set for completeness.
    SameNameBlocked,
    /// The required skill exists but is otherwise not linked (including when a
    /// transitive dependency was itself blocked and removed from the active set).
    Unlinked,
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
    /// A skill was pulled into the selection by a same-catalog `requires`.
    RequiredExpanded,
    /// A `requires` points to another source; reported but never auto-installed.
    CrossSourceRequire,
    /// A dependent is held back because its required closure is not linkable.
    RequiredBlocked,
}

#[derive(Debug, Clone)]
struct Candidate {
    skill: ResolvedSkill,
    owners: Vec<String>,
    trusted: bool,
    /// Declared dependencies, carried through for required-closure preflight.
    requires: Vec<String>,
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
    // Phase 1: expand each catalog source's selection along its same-catalog
    // `requires` closure, so a selected skill pulls in the same-catalog skills it
    // depends on (transitively) before candidates are built.
    let effective_selection = expand_catalog_selections(&source_by_id, &input.inventories);

    let mut candidates = Vec::new();
    for inventory in &input.inventories {
        let Some(source) = source_by_id.get(inventory.source_id.as_str()) else {
            continue;
        };
        let selection = effective_selection
            .get(source.id.as_str())
            .map_or(source.selection.as_slice(), Vec::as_slice);

        for skill in &inventory.skills {
            if source.kind == SourceKind::Catalog
                && !crate::catalog::skill_is_selected(skill, selection)
            {
                // Unselected catalog skills are offers, not part of the resolved
                // set; they surface through `source inspect`.
                continue;
            }
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
                requires: skill.requires.clone(),
            });
        }
    }

    // Carry each candidate's declared dependencies by ref for the post-resolution
    // required-closure preflight.
    let requires_by_ref: BTreeMap<String, Vec<String>> = candidates
        .iter()
        .filter(|candidate| !candidate.requires.is_empty())
        .map(|candidate| {
            (
                candidate.skill.source_ref.clone(),
                candidate.requires.clone(),
            )
        })
        .collect();

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

    // Phase 3: required-closure preflight (RFC 0003 section 4 step 5). A skill
    // whose required closure cannot be linked consistently must not materialize;
    // blocking propagates to dependents through a fixpoint.
    let (mut blocked_skills, closure_diagnostics) = required_closure_preflight(
        &mut active_skills,
        &pending_approval_skills,
        &requires_by_ref,
        &input.inventories,
    );
    diagnostics.extend(closure_diagnostics);

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

    // Report selections that grew via the requires closure, so status/doctor can
    // show which selections expanded beyond what the user explicitly chose.
    for (source_id, effective) in &effective_selection {
        let Some(source) = source_by_id.get(source_id.as_str()) else {
            continue;
        };
        for reference in effective {
            if !source
                .selection
                .iter()
                .any(|selected| selected == reference)
            {
                diagnostics.push(ResolutionDiagnostic {
                    code: ResolutionDiagnosticCode::RequiredExpanded,
                    message: format!(
                        "`{reference}` was pulled into catalog `{source_id}` by a requires closure"
                    ),
                    source_ref: Some(format!("{source_id}:{reference}")),
                });
            }
        }
    }
    diagnostics.sort_by(|left, right| {
        diagnostic_code_name(left.code)
            .cmp(diagnostic_code_name(right.code))
            .then_with(|| left.source_ref.cmp(&right.source_ref))
    });

    blocked_skills.sort_by(|left, right| {
        left.skill
            .slot_name
            .cmp(&right.skill.slot_name)
            .then_with(|| left.skill.source_ref.cmp(&right.skill.source_ref))
    });

    Resolution {
        active_skills,
        pending_approval_skills,
        unlinked_skills,
        blocked_skills,
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
        ResolutionDiagnosticCode::RequiredExpanded => "required_expanded",
        ResolutionDiagnosticCode::CrossSourceRequire => "cross_source_require",
        ResolutionDiagnosticCode::RequiredBlocked => "required_blocked",
    }
}

/// Human-readable label for a closure block reason.
#[must_use]
pub fn closure_block_reason_name(reason: ClosureBlockReason) -> &'static str {
    match reason {
        ClosureBlockReason::Missing => "missing",
        ClosureBlockReason::PendingApproval => "pending approval",
        ClosureBlockReason::ShadowedNotSatisfied => "shadowed but not satisfied",
        ClosureBlockReason::SameNameBlocked => "blocked by a same-name target entry",
        ClosureBlockReason::Unlinked => "unlinked",
    }
}

/// Whether a `requires` reference matches a skill by slot, ref, or stable ID.
fn ref_matches_skill(reference: &str, skill: &SkillRecord) -> bool {
    skill.slot_name == reference
        || skill.source_ref == reference
        || skill.id.as_deref() == Some(reference)
}

fn inventory_for<'a>(
    inventories: &'a [SourceInventory],
    source_id: &str,
) -> Option<&'a [SkillRecord]> {
    inventories
        .iter()
        .find(|inventory| inventory.source_id == source_id)
        .map(|inventory| inventory.skills.as_slice())
}

/// Expand each catalog source's explicit selection with the transitive closure of
/// its same-catalog `requires`. Cross-source requires are never expanded here.
fn expand_catalog_selections(
    source_by_id: &BTreeMap<&str, &SourceConfig>,
    inventories: &[SourceInventory],
) -> BTreeMap<String, Vec<String>> {
    let mut expanded = BTreeMap::new();
    for inventory in inventories {
        let Some(source) = source_by_id.get(inventory.source_id.as_str()) else {
            continue;
        };
        if source.kind != SourceKind::Catalog {
            continue;
        }
        expanded.insert(
            source.id.clone(),
            closure_for_selection(&source.selection, &inventory.skills),
        );
    }
    expanded
}

fn closure_for_selection(selection: &[String], skills: &[SkillRecord]) -> Vec<String> {
    let mut result: Vec<String> = selection.to_vec();
    let mut seen: BTreeSet<String> = selection.iter().cloned().collect();
    let mut queue: VecDeque<String> = selection.iter().cloned().collect();
    while let Some(reference) = queue.pop_front() {
        let Some(skill) = skills
            .iter()
            .find(|skill| ref_matches_skill(&reference, skill))
        else {
            continue;
        };
        for requirement in &skill.requires {
            // Only same-catalog requirements expand the selection.
            let Some(required) = skills
                .iter()
                .find(|skill| ref_matches_skill(requirement, skill))
            else {
                continue;
            };
            // Canonicalize on slot name so different refs to the same skill dedupe.
            let canonical = required.slot_name.clone();
            if seen.insert(canonical.clone()) {
                result.push(canonical.clone());
                queue.push_back(canonical);
            }
        }
    }
    result
}

/// Resolved state of one `requires` reference against the active skill set.
enum RequirementStatus {
    /// A linked active skill fills the requirement.
    Satisfied,
    /// The dependent must be held back for this reason.
    Block(ClosureBlockReason),
}

/// Classify a same-source requirement against the active set.
fn requirement_status(
    requirement: &str,
    required: &SkillRecord,
    active: &[ResolvedSkill],
    pending_refs: &BTreeSet<&str>,
) -> RequirementStatus {
    let winner = active
        .iter()
        .find(|skill| skill.slot_name == required.slot_name);
    match winner {
        // The exact required skill, or an equivalent one (same stable ID), is active.
        Some(skill) if skill.source_ref == required.source_ref => RequirementStatus::Satisfied,
        Some(skill) if skill.id.is_some() && skill.id == required.id => {
            RequirementStatus::Satisfied
        }
        // The slot is filled by a different skill. A requirement that named a stable
        // ID is not satisfied by a non-equivalent winner; one that named a slot is.
        Some(_) => {
            if required.id.as_deref() == Some(requirement) {
                RequirementStatus::Block(ClosureBlockReason::ShadowedNotSatisfied)
            } else {
                RequirementStatus::Satisfied
            }
        }
        None if pending_refs.contains(required.source_ref.as_str()) => {
            RequirementStatus::Block(ClosureBlockReason::PendingApproval)
        }
        None => RequirementStatus::Block(ClosureBlockReason::Unlinked),
    }
}

/// Find the first active skill whose required closure cannot be satisfied.
fn find_blocked(
    active: &[ResolvedSkill],
    pending_refs: &BTreeSet<&str>,
    requires_by_ref: &BTreeMap<String, Vec<String>>,
    inventories: &[SourceInventory],
) -> Option<(usize, BlockedSkill)> {
    for (index, dependent) in active.iter().enumerate() {
        let Some(requires) = requires_by_ref.get(&dependent.source_ref) else {
            continue;
        };
        for requirement in requires {
            let same_source = inventory_for(inventories, &dependent.source_id).and_then(|skills| {
                skills
                    .iter()
                    .find(|skill| ref_matches_skill(requirement, skill))
            });
            let Some(required) = same_source else {
                // Not same-source: cross-source requires are reported later, never
                // installed; only a requirement that exists nowhere blocks here.
                let exists_elsewhere = inventories.iter().any(|inventory| {
                    inventory.source_id != dependent.source_id
                        && inventory
                            .skills
                            .iter()
                            .any(|skill| ref_matches_skill(requirement, skill))
                });
                if exists_elsewhere {
                    continue;
                }
                return Some((
                    index,
                    BlockedSkill {
                        skill: dependent.clone(),
                        requirement: requirement.clone(),
                        reason: ClosureBlockReason::Missing,
                    },
                ));
            };
            if let RequirementStatus::Block(reason) =
                requirement_status(requirement, required, active, pending_refs)
            {
                return Some((
                    index,
                    BlockedSkill {
                        skill: dependent.clone(),
                        requirement: requirement.clone(),
                        reason,
                    },
                ));
            }
        }
    }
    None
}

/// Required-closure preflight. Removes from `active` any skill whose required
/// closure cannot be linked, propagating blocks to dependents via a fixpoint, and
/// reports cross-source requires without installing them.
fn required_closure_preflight(
    active: &mut Vec<ResolvedSkill>,
    pending: &[ResolvedSkill],
    requires_by_ref: &BTreeMap<String, Vec<String>>,
    inventories: &[SourceInventory],
) -> (Vec<BlockedSkill>, Vec<ResolutionDiagnostic>) {
    let mut blocked = Vec::new();
    let mut diagnostics = Vec::new();
    let pending_refs: BTreeSet<&str> = pending
        .iter()
        .map(|skill| skill.source_ref.as_str())
        .collect();

    // Fixpoint: blocking a dependent can in turn block skills that required it.
    while let Some((index, block)) =
        find_blocked(active, &pending_refs, requires_by_ref, inventories)
    {
        diagnostics.push(ResolutionDiagnostic {
            code: ResolutionDiagnosticCode::RequiredBlocked,
            message: format!(
                "skill `{}` is blocked: requirement `{}` is {}",
                block.skill.source_ref,
                block.requirement,
                closure_block_reason_name(block.reason)
            ),
            source_ref: Some(block.skill.source_ref.clone()),
        });
        active.remove(index);
        blocked.push(block);
    }

    // With the active set final, report cross-source requires (checked, not installed).
    for dependent in active.iter() {
        let Some(requires) = requires_by_ref.get(&dependent.source_ref) else {
            continue;
        };
        for requirement in requires {
            let same_source =
                inventory_for(inventories, &dependent.source_id).is_some_and(|skills| {
                    skills
                        .iter()
                        .any(|skill| ref_matches_skill(requirement, skill))
                });
            if same_source {
                continue;
            }
            let cross_source = inventories.iter().any(|inventory| {
                inventory.source_id != dependent.source_id
                    && inventory
                        .skills
                        .iter()
                        .any(|skill| ref_matches_skill(requirement, skill))
            });
            if cross_source {
                diagnostics.push(ResolutionDiagnostic {
                    code: ResolutionDiagnosticCode::CrossSourceRequire,
                    message: format!(
                        "skill `{}` requires `{}` from another source; reported, not installed",
                        dependent.source_ref, requirement
                    ),
                    source_ref: Some(dependent.source_ref.clone()),
                });
            }
        }
    }

    (blocked, diagnostics)
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
            selection: Vec::new(),
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

    fn catalog(id: &str, priority: i32, selection: &[&str]) -> SourceConfig {
        let mut source = source(id, SourceKind::Catalog, priority);
        source.trusted = true;
        source.selection = selection.iter().map(|item| (*item).to_owned()).collect();
        source
    }

    fn skill_req(source_id: &str, slot_name: &str, requires: &[&str]) -> SkillRecord {
        let mut record = skill(source_id, slot_name);
        record.requires = requires.iter().map(|item| (*item).to_owned()).collect();
        record
    }

    fn active_slots(resolution: &Resolution) -> Vec<&str> {
        resolution
            .active_skills
            .iter()
            .map(|skill| skill.slot_name.as_str())
            .collect()
    }

    #[test]
    fn resolve_should_expand_same_catalog_required_closure() {
        let resolution = resolve_with(
            vec![catalog("cat", 10, &["alpha"])],
            vec![inventory(
                "cat",
                vec![skill_req("cat", "alpha", &["beta"]), skill("cat", "beta")],
            )],
            Vec::new(),
        );

        let active = active_slots(&resolution);
        assert!(active.contains(&"alpha"));
        // `beta` was pulled into the selection by `alpha`'s requires.
        assert!(active.contains(&"beta"));
        assert!(resolution.blocked_skills.is_empty());
    }

    #[test]
    fn resolve_should_expand_required_closure_transitively() {
        let resolution = resolve_with(
            vec![catalog("cat", 10, &["alpha"])],
            vec![inventory(
                "cat",
                vec![
                    skill_req("cat", "alpha", &["beta"]),
                    skill_req("cat", "beta", &["gamma"]),
                    skill("cat", "gamma"),
                ],
            )],
            Vec::new(),
        );

        // The closure is walked transitively: gamma is reachable only through beta.
        assert!(active_slots(&resolution).contains(&"gamma"));
    }

    #[test]
    fn resolve_should_block_dependent_on_missing_requirement() {
        let resolution = resolve_with(
            vec![catalog("cat", 10, &["alpha"])],
            vec![inventory(
                "cat",
                vec![skill_req("cat", "alpha", &["ghost"])],
            )],
            Vec::new(),
        );

        assert!(resolution.active_skills.is_empty());
        assert_eq!(resolution.blocked_skills.len(), 1);
        assert_eq!(
            resolution.blocked_skills[0].reason,
            ClosureBlockReason::Missing
        );
    }

    #[test]
    fn resolve_should_block_dependent_when_requirement_pending_approval() {
        // Same untrusted team source: only `alpha` is approved, so its required
        // `beta` is still pending and `alpha` must not materialize.
        let resolution = resolve_with(
            vec![source("team", SourceKind::Team, 10)],
            vec![inventory(
                "team",
                vec![skill_req("team", "alpha", &["beta"]), skill("team", "beta")],
            )],
            vec![approval("skill", "alpha")],
        );

        assert!(!active_slots(&resolution).contains(&"alpha"));
        assert_eq!(
            resolution.blocked_skills[0].reason,
            ClosureBlockReason::PendingApproval
        );
    }

    #[test]
    fn resolve_should_report_cross_source_require_without_installing() {
        let mut team = source("team", SourceKind::Team, 20);
        team.trusted = true;
        let resolution = resolve_with(
            vec![catalog("cat", 10, &["alpha"]), team],
            vec![
                inventory("cat", vec![skill_req("cat", "alpha", &["beta"])]),
                inventory("team", vec![skill("team", "beta")]),
            ],
            Vec::new(),
        );

        // The cross-source requirement is reported but never auto-installs `beta`
        // into the catalog selection, and it does not block `alpha`.
        assert!(active_slots(&resolution).contains(&"alpha"));
        assert!(
            resolution
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == ResolutionDiagnosticCode::CrossSourceRequire)
        );
    }
}
