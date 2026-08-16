//! Read-only, schema-versioned multi-target installation planning.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agent::{self, AgentProvider, CompatibilityResult};
use crate::error::{DaloError, DaloResult};
use crate::inventory::SourceInventory;
use crate::materialize::{self, MaterializeOperation, MaterializeOperationStatus};
use crate::plugin::{
    AppliedPluginPolicy, MemberRequirement, PluginComponentState, PluginInventoryWarning,
    PluginResolution, PluginState, ResolvedPluginDependency, SelectionOrigin,
};
use crate::store::{self, MaterializationDirState, StateFile, StorePaths};

/// Current JSON schema version for read-only installation plans.
pub const INSTALLATION_PLAN_SCHEMA_VERSION: u32 = 1;

/// Complete read-only installation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallationPlan {
    /// Explicit plan schema version.
    pub schema_version: u32,
    /// Store inspected by the planner.
    pub store: PathBuf,
    /// Canonical plugin resolution shared by every target.
    pub canonical_plugins: PluginResolution,
    /// Malformed package findings kept visible without erasing valid siblings.
    pub inventory_warnings: Vec<PluginInventoryWarning>,
    /// Physical destinations, each retaining all logical target explanations.
    pub destinations: Vec<DestinationPlan>,
}

/// One de-duplicated physical destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DestinationPlan {
    /// Canonical physical path.
    pub path: PathBuf,
    /// Per-logical-target explanations for the shared destination.
    pub logical_targets: Vec<LogicalTargetPlan>,
    /// De-duplicated portable skill link operations for this destination.
    pub portable_operations: Vec<MaterializeOperation>,
}

/// One linked logical target and its adapter-specific plugin plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogicalTargetPlan {
    /// Logical target ID.
    pub id: String,
    /// Adapter capability baseline used for this explanation.
    pub verification_baseline: String,
    /// Selected plugin plans in canonical identity order.
    pub plugins: Vec<TargetPluginPlan>,
}

/// Effective selected plugin state for one logical target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetPluginPlan {
    /// Canonical selected identity.
    pub source_ref: String,
    /// Every distinct selection origin.
    pub origins: Vec<SelectionOrigin>,
    /// Applied policy decisions, separate from origins.
    pub policies: Vec<AppliedPluginPolicy>,
    /// Canonical dependency outcomes.
    pub dependencies: Vec<ResolvedPluginDependency>,
    /// Effective target-level state.
    pub state: TargetPluginState,
    /// Worst relevant target compatibility.
    pub compatibility: PlanCompatibility,
    /// Component-level explanations.
    pub components: Vec<TargetComponentPlan>,
    /// Target-level blockers.
    pub blockers: Vec<PlanBlocker>,
}

/// Target-level plugin coherence state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPluginState {
    /// Required closure can be represented for this target.
    Active,
    /// Optional or recommended behavior is visibly omitted or reduced.
    Degraded,
    /// Required behavior or safety cannot be represented.
    Blocked,
    /// Explicit local policy suppresses otherwise selected intent.
    Declined,
    /// Candidate lost its canonical plugin slot.
    Shadowed,
}

/// Plan compatibility vocabulary from RFC 0005.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanCompatibility {
    /// Native or portable representation is exact.
    Exact,
    /// Deterministic adapter mapping or authored fallback preserves behavior.
    Mapped,
    /// Provider receives guidance without enforceable native semantics.
    GuidanceOnly,
    /// Target adapter cannot consume this optional behavior.
    Unsupported,
    /// Required behavior or safety boundary blocks the projection.
    Blocked,
}

/// One plugin component planned for one logical target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetComponentPlan {
    /// Authored component reference.
    pub reference: String,
    /// Required, optional, or recommended membership.
    pub requirement: MemberRequirement,
    /// Canonical activation state before target mapping.
    pub canonical_state: PluginComponentState,
    /// Effective component state for this logical target.
    pub state: TargetComponentState,
    /// Target adapter compatibility.
    pub compatibility: PlanCompatibility,
    /// Authored fallback, when any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authored_fallback: Option<String>,
    /// Fallback selected for this target, when safe and necessary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_fallback: Option<String>,
    /// Proposed artifact or portable-only delivery.
    pub proposed_artifact: String,
    /// Exact blocker category when this component cannot be applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<PlanBlocker>,
    /// One actionable remediation when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// Effective component state vocabulary used by installation plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetComponentState {
    /// Component is active and represented for the target.
    Active,
    /// Instruction or independently managed component remains inactive.
    Inactive,
    /// Independent component approval is still missing.
    PendingApproval,
    /// Component lost its canonical namespace slot.
    Shadowed,
    /// Optional/recommended behavior or a declined plugin is visibly omitted.
    IntentionallyOmitted,
    /// Required behavior or safety cannot be represented.
    Blocked,
}

/// Typed blocker with a stable source category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanBlocker {
    /// Stable blocker source.
    pub source: PlanBlockerSource,
    /// Human-readable explanation.
    pub message: String,
}

/// Source of a planning blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanBlockerSource {
    /// Malformed source inventory.
    MalformedInput,
    /// Missing or non-winning plugin dependency.
    MissingDependency,
    /// Component has not crossed its independent approval boundary.
    MissingApproval,
    /// Requested logical target is not linked.
    TargetUnlinked,
    /// Adapter has no safe representation.
    UnsupportedBehavior,
    /// Portable safety boundary cannot be preserved.
    RequiredSafetyBoundary,
    /// Persisted or filesystem observation differs.
    Drift,
    /// Existing target content or a namespace winner conflicts.
    Conflict,
    /// Explicit policy decision suppresses intent.
    PolicyDecision,
    /// Required instruction pack remains independently inactive.
    InactiveInstruction,
}

/// Build a plan from current store state without refreshing, auditing, or
/// writing any source, target, cache, approval, or lock file.
pub fn build_installation_plan(
    store_root: &Path,
    target_filter: Option<&str>,
) -> DaloResult<InstallationPlan> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let config = store::read_config(&paths)?;
    let approvals = store::read_approvals(&paths)?;
    let lock = store::read_user_lock(&paths)?;
    let state = store::read_state(&paths)?;
    if let Some(target) = target_filter
        && !state
            .targets
            .iter()
            .any(|candidate| candidate.enabled && candidate.id == target)
    {
        return Err(DaloError::InvalidArgument {
            reason: format!("target `{target}` is not linked"),
        });
    }
    let mut live = crate::resolver::resolve_from_config(&config, approvals.approvals);
    let active_instructions = lock
        .active_instruction_packs
        .iter()
        .map(|pack| format!("{}:{}", pack.source_id, pack.pack_id))
        .collect::<BTreeSet<_>>();
    crate::plugin::apply_component_resolution(
        &mut live.plugins,
        &live.resolution,
        &live.agents,
        &active_instructions,
    );
    let materialization = materialize::materialize(&paths, &live.resolution, true)?;
    let inventories = live
        .scans
        .iter()
        .filter_map(|scan| scan.inventory.clone())
        .collect::<Vec<_>>();
    Ok(build_from_facts(
        store_root,
        &state,
        &live.plugins,
        &inventories,
        &materialization.operations,
        target_filter,
    ))
}

/// Compose typed planning facts already loaded by status or dry-run paths.
#[must_use]
pub fn build_from_facts(
    store_root: &Path,
    state: &StateFile,
    plugins: &PluginResolution,
    inventories: &[SourceInventory],
    operations: &[MaterializeOperation],
    target_filter: Option<&str>,
) -> InstallationPlan {
    let mut destinations = state
        .materialization_dirs
        .iter()
        .filter_map(|destination| {
            destination_plan(destination, plugins, inventories, operations, target_filter)
        })
        .collect::<Vec<_>>();
    destinations.sort_by(|left, right| left.path.cmp(&right.path));
    let mut inventory_warnings = inventories
        .iter()
        .flat_map(|inventory| inventory.plugin_warnings.iter().cloned())
        .collect::<Vec<_>>();
    inventory_warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });
    InstallationPlan {
        schema_version: INSTALLATION_PLAN_SCHEMA_VERSION,
        store: store_root.to_path_buf(),
        canonical_plugins: plugins.clone(),
        inventory_warnings,
        destinations,
    }
}

fn destination_plan(
    destination: &MaterializationDirState,
    plugins: &PluginResolution,
    inventories: &[SourceInventory],
    operations: &[MaterializeOperation],
    target_filter: Option<&str>,
) -> Option<DestinationPlan> {
    let mut logical_targets = destination
        .logical_targets
        .iter()
        .filter(|target| target_filter.is_none_or(|filter| target.as_str() == filter))
        .map(|target| {
            logical_target_plan(target, plugins, inventories, operations, &destination.path)
        })
        .collect::<Vec<_>>();
    if logical_targets.is_empty() {
        return None;
    }
    logical_targets.sort_by(|left, right| left.id.cmp(&right.id));
    let mut portable_operations = operations
        .iter()
        .filter(|operation| operation.link_path.parent() == Some(destination.path.as_path()))
        .cloned()
        .collect::<Vec<_>>();
    portable_operations.sort_by(|left, right| left.link_path.cmp(&right.link_path));
    Some(DestinationPlan {
        path: destination.path.clone(),
        logical_targets,
        portable_operations,
    })
}

fn logical_target_plan(
    target: &str,
    plugins: &PluginResolution,
    inventories: &[SourceInventory],
    operations: &[MaterializeOperation],
    destination: &Path,
) -> LogicalTargetPlan {
    let mut target_plugins = plugins
        .plugins
        .iter()
        .map(|plugin| target_plugin_plan(target, plugin, inventories, operations, destination))
        .collect::<Vec<_>>();
    target_plugins.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    LogicalTargetPlan {
        id: target.to_owned(),
        verification_baseline: verification_baseline(target).to_owned(),
        plugins: target_plugins,
    }
}

fn target_plugin_plan(
    target: &str,
    plugin: &crate::plugin::ResolvedPlugin,
    inventories: &[SourceInventory],
    operations: &[MaterializeOperation],
    destination: &Path,
) -> TargetPluginPlan {
    let mut blockers = Vec::new();
    let mut components = plugin
        .members
        .iter()
        .map(|member| component_plan(target, member, inventories, operations, destination))
        .collect::<Vec<_>>();
    components.sort_by(|left, right| left.reference.cmp(&right.reference));
    for dependency in &plugin.dependencies {
        if dependency.requirement == crate::plugin::DependencyRequirement::Required
            && dependency.state != crate::plugin::PluginDependencyState::Selected
        {
            blockers.push(PlanBlocker {
                source: PlanBlockerSource::MissingDependency,
                message: format!(
                    "required dependency `{}` is {:?}",
                    dependency.reference, dependency.state
                )
                .to_lowercase(),
            });
        }
    }
    for component in &components {
        if component.requirement == MemberRequirement::Required
            && let Some(blocker) = &component.blocker
        {
            blockers.push(blocker.clone());
        }
    }
    if plugin.state == PluginState::Declined {
        for component in &mut components {
            component.state = TargetComponentState::IntentionallyOmitted;
        }
        blockers.push(PlanBlocker {
            source: PlanBlockerSource::PolicyDecision,
            message: "selected plugin is declined by user-local policy".to_owned(),
        });
    }
    blockers.sort_by(|left, right| left.message.cmp(&right.message));
    blockers.dedup();
    let optional_degradation = components.iter().any(|component| {
        component.requirement != MemberRequirement::Required
            && component.compatibility >= PlanCompatibility::GuidanceOnly
    });
    let state = match plugin.state {
        PluginState::Shadowed => TargetPluginState::Shadowed,
        PluginState::Declined => TargetPluginState::Declined,
        PluginState::Blocked => TargetPluginState::Blocked,
        PluginState::Selected if !blockers.is_empty() => TargetPluginState::Blocked,
        PluginState::Selected if optional_degradation => TargetPluginState::Degraded,
        PluginState::Selected => TargetPluginState::Active,
    };
    let compatibility = if matches!(
        state,
        TargetPluginState::Blocked | TargetPluginState::Declined
    ) {
        PlanCompatibility::Blocked
    } else {
        components
            .iter()
            .filter(|component| component.requirement == MemberRequirement::Required)
            .map(|component| component.compatibility)
            .max()
            .unwrap_or(PlanCompatibility::Exact)
    };
    TargetPluginPlan {
        source_ref: plugin.source_ref.clone(),
        origins: plugin.origins.clone(),
        policies: plugin.policies.clone(),
        dependencies: plugin.dependencies.clone(),
        state,
        compatibility,
        components,
        blockers,
    }
}

fn component_plan(
    target: &str,
    member: &crate::plugin::ResolvedPluginMember,
    inventories: &[SourceInventory],
    operations: &[MaterializeOperation],
    destination: &Path,
) -> TargetComponentPlan {
    let mut compatibility = canonical_compatibility(member.state);
    let mut blocker = canonical_blocker(member);
    let mut remediation = canonical_remediation(member);
    let mut selected_fallback = None;
    let proposed_artifact = if member.reference.starts_with("skill:") {
        if let Some(identity) = &member.resolved_ref {
            let slot = identity
                .split_once(':')
                .map_or(identity.as_str(), |(_, slot)| slot);
            if let Some(operation) = operations
                .iter()
                .find(|operation| operation.link_path == destination.join(slot))
            {
                if operation.status == MaterializeOperationStatus::Blocked {
                    compatibility = PlanCompatibility::Blocked;
                    blocker = Some(PlanBlocker {
                        source: PlanBlockerSource::Conflict,
                        message: operation.reason.clone().unwrap_or_else(|| {
                            format!("target slot `{}` is blocked", operation.link_path.display())
                        }),
                    });
                    remediation =
                        Some("resolve the target slot conflict and run plan again".to_owned());
                }
                format!("portable skill link: {}", operation.link_path.display())
            } else {
                format!("portable skill `{identity}` (no target operation while inactive)")
            }
        } else {
            "portable skill unavailable".to_owned()
        }
    } else if member.reference.starts_with("agent:") {
        let agent = member.resolved_ref.as_deref().and_then(|identity| {
            inventories
                .iter()
                .flat_map(|inventory| &inventory.agents)
                .find(|agent| agent.source_ref == identity)
        });
        match (AgentProvider::parse(target).ok(), agent) {
            (Some(provider), Some(agent)) if member.state == PluginComponentState::Active => {
                let compilation = agent::compile_record(agent, provider);
                compatibility = map_agent_compatibility(compilation.overall);
                if compilation.not_targeted {
                    compatibility = PlanCompatibility::Unsupported;
                }
                if compatibility == PlanCompatibility::Blocked {
                    blocker = Some(PlanBlocker {
                        source: PlanBlockerSource::RequiredSafetyBoundary,
                        message: format!(
                            "{target} cannot preserve the canonical agent safety boundary"
                        ),
                    });
                    remediation = Some(
                        "adjust the canonical agent boundary or choose a compatible target"
                            .to_owned(),
                    );
                }
                format!(
                    "read-only {} agent projection preview: {}",
                    provider.id(),
                    provider.filename(&agent.slot_name)
                )
            }
            (None, _) if member.state == PluginComponentState::Active => {
                compatibility = PlanCompatibility::Unsupported;
                if let Some(fallback) = &member.fallback {
                    compatibility = PlanCompatibility::Mapped;
                    selected_fallback = Some(fallback.clone());
                    blocker = None;
                    remediation = None;
                } else {
                    blocker = Some(PlanBlocker {
                        source: PlanBlockerSource::UnsupportedBehavior,
                        message: format!(
                            "target `{target}` has no isolated canonical-agent adapter"
                        ),
                    });
                    remediation = Some(
                        "link a Codex or Claude target, or author an inline skill fallback"
                            .to_owned(),
                    );
                }
                "portable agent only; no native write in this slice".to_owned()
            }
            _ => "portable agent unavailable until its independent activation boundary succeeds"
                .to_owned(),
        }
    } else {
        compatibility = if member.state == PluginComponentState::Active {
            PlanCompatibility::GuidanceOnly
        } else if member.requirement == MemberRequirement::Required {
            PlanCompatibility::Blocked
        } else {
            PlanCompatibility::GuidanceOnly
        };
        "independently managed instruction block; planner never enables it".to_owned()
    };
    TargetComponentPlan {
        reference: member.reference.clone(),
        requirement: member.requirement,
        canonical_state: member.state,
        state: target_component_state(member, compatibility, selected_fallback.is_some()),
        compatibility,
        authored_fallback: member.fallback.clone(),
        selected_fallback,
        proposed_artifact,
        blocker,
        remediation,
    }
}

fn target_component_state(
    member: &crate::plugin::ResolvedPluginMember,
    compatibility: PlanCompatibility,
    fallback_selected: bool,
) -> TargetComponentState {
    if fallback_selected {
        return TargetComponentState::Active;
    }
    match member.state {
        PluginComponentState::PendingApproval => TargetComponentState::PendingApproval,
        PluginComponentState::Shadowed => TargetComponentState::Shadowed,
        PluginComponentState::Inactive | PluginComponentState::Available => {
            TargetComponentState::Inactive
        }
        PluginComponentState::Active
            if compatibility == PlanCompatibility::Unsupported
                && member.requirement != MemberRequirement::Required =>
        {
            TargetComponentState::IntentionallyOmitted
        }
        PluginComponentState::Active if compatibility < PlanCompatibility::Blocked => {
            TargetComponentState::Active
        }
        PluginComponentState::Active
        | PluginComponentState::Blocked
        | PluginComponentState::Missing
        | PluginComponentState::Ambiguous => TargetComponentState::Blocked,
    }
}

fn canonical_compatibility(state: PluginComponentState) -> PlanCompatibility {
    match state {
        PluginComponentState::Active => PlanCompatibility::Exact,
        PluginComponentState::Available | PluginComponentState::Inactive => {
            PlanCompatibility::GuidanceOnly
        }
        PluginComponentState::PendingApproval
        | PluginComponentState::Shadowed
        | PluginComponentState::Blocked
        | PluginComponentState::Missing
        | PluginComponentState::Ambiguous => PlanCompatibility::Blocked,
    }
}

fn canonical_blocker(member: &crate::plugin::ResolvedPluginMember) -> Option<PlanBlocker> {
    let source = match member.state {
        PluginComponentState::PendingApproval => PlanBlockerSource::MissingApproval,
        PluginComponentState::Inactive if member.reference.starts_with("instruction:") => {
            PlanBlockerSource::InactiveInstruction
        }
        PluginComponentState::Shadowed | PluginComponentState::Ambiguous => {
            PlanBlockerSource::Conflict
        }
        PluginComponentState::Blocked => PlanBlockerSource::RequiredSafetyBoundary,
        PluginComponentState::Missing => PlanBlockerSource::MalformedInput,
        PluginComponentState::Active
        | PluginComponentState::Available
        | PluginComponentState::Inactive => return None,
    };
    Some(PlanBlocker {
        source,
        message: format!("component `{}` is {:?}", member.reference, member.state).to_lowercase(),
    })
}

fn canonical_remediation(member: &crate::plugin::ResolvedPluginMember) -> Option<String> {
    match member.state {
        PluginComponentState::PendingApproval if member.reference.starts_with("skill:") => member
            .resolved_ref
            .as_ref()
            .map(|identity| format!("run `dalo approve skill {identity}` after review")),
        PluginComponentState::PendingApproval if member.reference.starts_with("agent:") => member
            .resolved_ref
            .as_ref()
            .map(|identity| format!("run `dalo approve agent {identity}` after review")),
        PluginComponentState::Inactive if member.reference.starts_with("instruction:") => Some(
            "enable the instruction pack explicitly for its instruction file, then plan again"
                .to_owned(),
        ),
        PluginComponentState::Missing => {
            Some("add the exact referenced component to its source".to_owned())
        }
        _ => None,
    }
}

fn map_agent_compatibility(result: CompatibilityResult) -> PlanCompatibility {
    match result {
        CompatibilityResult::Exact => PlanCompatibility::Exact,
        CompatibilityResult::Mapped => PlanCompatibility::Mapped,
        CompatibilityResult::GuidanceOnly => PlanCompatibility::GuidanceOnly,
        CompatibilityResult::Unsupported => PlanCompatibility::Unsupported,
        CompatibilityResult::Blocked => PlanCompatibility::Blocked,
    }
}

fn verification_baseline(target: &str) -> &'static str {
    match target {
        "codex" => "portable-skill-v1 + canonical-agent-codex-v1",
        "claude" => "portable-skill-v1 + canonical-agent-claude-v1",
        "openclaw" | "hermes" | "generic" => "portable-skill-v1; no verified agent adapter",
        _ => "experimental target; portable skill path only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ResolvedPluginMember;

    #[test]
    fn generic_target_selects_authored_inline_fallback_for_active_agent() {
        let member = ResolvedPluginMember {
            reference: "agent:reviewer".to_owned(),
            requirement: MemberRequirement::Required,
            state: PluginComponentState::Active,
            resolved_ref: Some("team:reviewer".to_owned()),
            fallback: Some("skill:review".to_owned()),
        };

        let planned = component_plan("generic", &member, &[], &[], Path::new("/target"));

        assert_eq!(planned.compatibility, PlanCompatibility::Mapped);
        assert_eq!(planned.selected_fallback.as_deref(), Some("skill:review"));
        assert!(planned.blocker.is_none());
    }

    #[test]
    fn pending_required_skill_names_approval_boundary_and_remediation() {
        let member = ResolvedPluginMember {
            reference: "skill:core".to_owned(),
            requirement: MemberRequirement::Required,
            state: PluginComponentState::PendingApproval,
            resolved_ref: Some("team:core".to_owned()),
            fallback: None,
        };

        let planned = component_plan("codex", &member, &[], &[], Path::new("/target"));

        assert_eq!(planned.compatibility, PlanCompatibility::Blocked);
        assert_eq!(
            planned.blocker.as_ref().map(|blocker| blocker.source),
            Some(PlanBlockerSource::MissingApproval)
        );
        assert_eq!(
            planned.remediation.as_deref(),
            Some("run `dalo approve skill team:core` after review")
        );
    }
}
