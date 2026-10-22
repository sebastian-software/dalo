//! Status model and renderable command output.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adopt::{
    AdoptReport, KeepReport, RemoveOwnedReport, ResolveListReport, TargetScanWarning, UnkeepReport,
    UnmanagedSkill,
};
use crate::agent::AgentInventoryWarning;
use crate::approval::{AcceptedRiskSummary, ApprovalListReport, ApprovalReport};
use crate::audit::{self, ActiveAuditFailure, AuditCoverage, AuditReport, AuditStatus};
use crate::autosync::{AutosyncMutationReport, AutosyncStatusReport};
use crate::catalog::{
    self, CatalogAdvanceReport, CatalogDrift, CatalogInspectReport, CatalogSelectReport,
};
use crate::doctor::{DoctorCode, DoctorFinding, DoctorReport, DoctorSeverity};
use crate::error::DaloResult;
use crate::hook::HookListReport;
use crate::instructions::{
    self, DiscoveredPack, InstructionBlockDrift, InstructionPackReport, TopicOverlap,
};
use crate::inventory::{InventoryWarning, InventoryWarningCode};
use crate::lockfile::{self, LockDrift, LockDriftCode};
use crate::materialize::{
    self, MaterializeOperation, MaterializeOperationStatus, MissingSkillCommand,
    SkillDeliveryReport, SyncReport,
};
use crate::plan::InstallationPlan;
use crate::plugin::{PluginInventoryWarning, PluginResolution};
use crate::resolver::{self, Resolution};
use crate::source::{
    SourceAddReport, SourceConfig, SourceHeadCache, SourceKind, SourceListReport,
    SourceNamespaceReport, SourcePriorityReport, SourceProvenance, SourceRemoveReport,
};
use crate::store::{self, InitReport, StorePaths};
use crate::target::{TargetDetectReport, TargetLinkReport, TargetUnlinkReport};
use crate::team_manifest::{
    TeamCatalogUpdateReport, TeamManifestAction, TeamManifestMutationReport, TeamManifestView,
};
use crate::term;
use crate::tool::ToolListReport;

/// Full status report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    /// Store root.
    pub store: PathBuf,
    /// Local assistant freshness and delivery; advisory, never check-blocking.
    pub assistant: crate::assistant::AssistantStatusReport,
    /// Source scan summaries.
    pub sources: Vec<SourceStatus>,
    /// Configured materialization targets.
    pub targets: Vec<TargetStatus>,
    /// Inventory warnings.
    pub inventory_warnings: Vec<InventoryWarning>,
    /// Canonical agent-package inventory warnings.
    pub agent_inventory_warnings: Vec<AgentInventoryWarning>,
    /// Passive portable-plugin inventory warnings.
    pub plugin_inventory_warnings: Vec<PluginInventoryWarning>,
    /// Target-independent passive plugin resolution.
    pub plugins: PluginResolution,
    /// Shared typed multi-target planning facts when plugins are selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation_plan: Option<InstallationPlan>,
    /// Discovered plugin-local executable contracts and exact trust state.
    pub tools: ToolListReport,
    /// Discovered plugin-local hook contracts and independent trust state.
    pub hooks: HookListReport,
    /// Read-only native hook sidecar reconciliation state per linked provider.
    pub hook_targets: Vec<crate::hook_sync::HookTargetReport>,
    /// Read-only provider-native plugin package reconciliation state.
    pub plugin_targets: Vec<crate::plugin_projection::PluginTargetReport>,
    /// Resolution output.
    pub resolution: Resolution,
    /// Readiness of every declared release binary, joined for the human active-skill block.
    ///
    /// JSON already carries each active skill's declarations, so this is omitted there.
    #[serde(skip)]
    pub binary_states: Vec<crate::binary::BinaryStatusReport>,
    /// Dry-run materialization operations that expose target-level blockers.
    pub materialization: Vec<MaterializeOperation>,
    /// Target-aware delivery selections and provider artifact provenance.
    pub deliveries: Vec<SkillDeliveryReport>,
    /// Active skills whose deterministic security audit blocks sync.
    pub blocking_audits: Vec<String>,
    /// Active skills whose security audit could not be completed.
    pub audit_failures: Vec<ActiveAuditFailure>,
    /// Previous-lock comparison against the live resolution.
    pub lock: LockStatus,
    /// Catalog source lock read or schema error, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_lock_error: Option<String>,
    /// Unmanaged skills found in linked targets.
    pub unmanaged_skills: Vec<UnmanagedSkill>,
    /// Non-fatal target directory scan warnings.
    pub target_warnings: Vec<TargetScanWarning>,
    /// Discovered instruction packs (available and enabled).
    pub instruction_packs: Vec<DiscoveredPack>,
    /// Declared-topic overlaps among active instruction packs (advisory).
    pub instruction_pack_overlaps: Vec<TopicOverlap>,
    /// Active instruction blocks that are missing, malformed, or stale.
    pub instruction_block_drifts: Vec<InstructionBlockDrift>,
    /// Native scheduler installation and latest durable run state.
    pub autosync: AutosyncStatusReport,
    /// Enabled tracking team sources whose checkout has local changes, which
    /// makes the next `sync` refuse to refresh them.
    ///
    /// Only the human report lists them, so [`build_status_report`] leaves this
    /// empty and the caller that prints fills it from
    /// [`dirty_tracking_sources`]; `doctor` reports `dirty_source`.
    #[serde(skip)]
    pub dirty_sources: Vec<String>,
}

/// Compact state-aware guidance for an entry point or repeated initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextActionReport {
    /// Store root.
    pub store: PathBuf,
    /// Whether the store has its configuration file.
    pub initialized: bool,
    /// Number of enabled linked targets.
    pub linked_targets: usize,
    /// Number of configured sources.
    pub sources: usize,
    /// Number of currently active skills.
    pub active_skills: usize,
    /// Number of skills awaiting approval.
    pub pending_approvals: usize,
    /// Current onboarding or synchronization state.
    pub state: NextActionState,
    /// Short explanation of the recommended action or healthy state.
    pub message: String,
    /// One copyable command, absent when the store is fully synchronized.
    pub command: Option<String>,
}

/// State selected for the single next action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActionState {
    /// The store still needs to be initialized.
    Uninitialized,
    /// No enabled target is linked.
    NoTarget,
    /// No source exposes a skill yet.
    NoSkills,
    /// A skill needs an explicit approval record.
    PendingApproval,
    /// The live resolution or materialized links need synchronization.
    SyncNeeded,
    /// A problem needs the full status report before it can be resolved.
    NeedsAttention,
    /// The store has no outstanding next action.
    Synced,
}

/// User lock status derived during `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LockStatus {
    /// User lock path.
    pub path: PathBuf,
    /// Persisted schema version.
    pub schema_version: u32,
    /// Drift between the previous lock and the live resolution.
    pub drift: Vec<LockDrift>,
}

/// One source status entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceStatus {
    /// Source ID.
    pub id: String,
    /// Source kind.
    pub kind: SourceKind,
    /// Source path.
    pub path: PathBuf,
    /// Priority.
    pub priority: i32,
    /// Optional prefix applied to every materialized skill from this source.
    pub namespace: Option<String>,
    /// Whether the source is enabled.
    pub enabled: bool,
    /// Whether the source path exists.
    pub exists: bool,
    /// Number of scanned skills.
    pub skill_count: usize,
    /// Number of scanned canonical agent packages.
    pub agent_count: usize,
    /// Number of scanned valid passive plugin packages.
    pub plugin_count: usize,
    /// Optional non-fatal scan error.
    pub error: Option<String>,
    /// Origin and pin information assembled without network access.
    pub provenance: SourceProvenance,
}

/// One configured target shown by `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetStatus {
    /// Logical target ID.
    pub id: String,
    /// Target directory.
    pub path: PathBuf,
    /// Whether the target is enabled for materialization.
    pub linked: bool,
    /// Whether the target directory exists.
    pub exists: bool,
}

/// Build the current status report.
#[must_use = "the status report should be rendered or inspected"]
pub fn build_status_report(store_root: &Path) -> DaloResult<StatusReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let config = store::read_config(&paths)?;
    let approvals = store::read_approvals(&paths)?;
    let previous_lock = store::read_user_lock(&paths)?;
    let state = store::read_state(&paths)?;
    let (source_lock, source_lock_error) = match catalog::read_source_lock(&paths) {
        Ok(source_lock) => (Some(source_lock), None),
        Err(error) => (
            None,
            Some(format!(
                "catalog source lock `{}` could not be read: {error}; repair or restore it, then rerun `dalo status --check`",
                paths.source_lock_file.display()
            )),
        ),
    };

    // The shared pipeline scans every enabled source once and resolves it; we
    // reuse its per-source scan outcomes here for the status detail instead of
    // re-scanning. Disabled sources are not scanned, so we render them directly.
    let resolved =
        resolver::resolve_from_config_with_plugin_inventories(&config, approvals.approvals.clone());
    let plugin_inventories = resolver::plugin_inventories(&resolved);
    let reconciliation_inventories = resolver::inventories_with_plugins(&resolved);
    let live = resolved.live;
    let binary_states = {
        let scanned = live
            .scans
            .iter()
            .filter_map(|scan| scan.inventory.clone())
            .collect::<Vec<_>>();
        crate::binary::list_from_inventories(
            &paths,
            &config.sources,
            &approvals.approvals,
            &scanned,
        )
        .binaries
    };
    let scan_by_id = live
        .scans
        .iter()
        .map(|scan| (scan.source.id.as_str(), scan))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut sources = Vec::new();
    let mut inventory_warnings = Vec::new();
    let mut agent_inventory_warnings = Vec::new();
    let mut plugin_inventory_warnings = Vec::new();
    let mut source_head_cache = SourceHeadCache::default();

    for source in &config.sources {
        let status = if let Some(scan) = scan_by_id.get(source.id.as_str()) {
            if let Some(inventory) = &scan.inventory {
                inventory_warnings.extend(inventory.warnings.iter().cloned());
                agent_inventory_warnings.extend(inventory.agent_warnings.iter().cloned());
                plugin_inventory_warnings.extend(inventory.plugin_warnings.iter().cloned());
            }
            SourceStatus {
                id: source.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                priority: source.priority,
                namespace: source.namespace.clone(),
                enabled: true,
                exists: source.path.exists(),
                skill_count: scan.inventory.as_ref().map_or(0, |inv| inv.skills.len()),
                agent_count: scan.inventory.as_ref().map_or(0, |inv| inv.agents.len()),
                plugin_count: scan.inventory.as_ref().map_or(0, |inv| inv.plugins.len()),
                error: scan.error.clone(),
                provenance: crate::source::source_provenance_with_head_cache(
                    source,
                    source_lock.as_ref(),
                    &mut source_head_cache,
                ),
            }
        } else {
            SourceStatus {
                id: source.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                priority: source.priority,
                namespace: source.namespace.clone(),
                enabled: false,
                exists: source.path.exists(),
                skill_count: 0,
                agent_count: 0,
                plugin_count: 0,
                error: None,
                provenance: crate::source::source_provenance_with_head_cache(
                    source,
                    source_lock.as_ref(),
                    &mut source_head_cache,
                ),
            }
        };
        sources.push(status);
    }

    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    inventory_warnings.sort_by(|left, right| left.path.cmp(&right.path));
    agent_inventory_warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });
    plugin_inventory_warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });

    let mut targets = state
        .targets
        .iter()
        .map(|target| TargetStatus {
            id: target.id.clone(),
            path: target.path.clone(),
            linked: target.enabled,
            exists: target.path.exists(),
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| left.id.cmp(&right.id));

    let audits = audit::audit_active_skills_with_config(&paths, &live.resolution, false, &config);
    let audit_degraded_sources = live.degraded_sources(&audits.failures);
    let mut plugins = live.plugins;
    let mut live_resolution = live.resolution;
    for failure in &audits.failures {
        if let Some(source) = sources
            .iter_mut()
            .find(|source| source.id == failure.source_id)
        {
            let reason = format!(
                "security audit failed for {}: {}",
                failure.source_ref, failure.reason
            );
            source.error = Some(
                source
                    .error
                    .as_ref()
                    .map_or(reason.clone(), |existing| format!("{existing}; {reason}")),
            );
        }
    }
    resolver::degrade_audit_failures(&mut live_resolution, &audits.failures);
    let active_instruction_refs = previous_lock
        .active_instruction_packs
        .iter()
        .map(|pack| format!("{}:{}", pack.source_id, pack.pack_id))
        .collect::<std::collections::BTreeSet<_>>();
    crate::plugin::apply_component_resolution(
        &mut plugins,
        &live_resolution,
        &live.agents,
        &active_instruction_refs,
    );
    let materialization = materialize::materialize_with_degraded_sources(
        &paths,
        &live_resolution,
        true,
        &audit_degraded_sources,
    )?;
    let tools = crate::tool::list_from_inventories_with_head_cache(
        &paths,
        &config.sources,
        &approvals.approvals,
        &plugin_inventories,
        &mut source_head_cache,
    );
    let hooks = crate::hook::list_from_inventories_with_head_cache(
        &paths,
        &config.sources,
        &approvals.approvals,
        &plugin_inventories,
        &tools.tools,
        &mut source_head_cache,
    )?;
    let selected_plugin_refs = plugins
        .plugins
        .iter()
        .filter(|plugin| plugin.state == crate::plugin::PluginState::Selected)
        .map(|plugin| plugin.source_ref.clone())
        .collect::<Vec<_>>();
    let hook_targets = crate::hook_sync::reconcile_with_hooks(
        &paths,
        &state,
        &selected_plugin_refs,
        &hooks.hooks,
        true,
    )?;
    let plugin_targets = crate::plugin_projection::reconcile(
        &paths,
        &state,
        &plugins,
        &reconciliation_inventories,
        &tools.tools,
        &hooks.hooks,
        true,
    )?;
    let mut installation_plan = (!plugins.plugins.is_empty()).then(|| {
        crate::plan::build_from_facts_with_active_skills(
            store_root,
            &state,
            &plugins,
            &reconciliation_inventories,
            &materialization.resolution.active_skills,
            &materialization.operations,
            None,
        )
    });
    if let Some(plan) = installation_plan.as_mut() {
        crate::plan::attach_tool_status_from_report(plan, &tools.tools);
        crate::plan::attach_hook_status_from_report(plan, &hooks.hooks);
        plan.native_plugins = plugin_targets.clone();
    }
    let live_lock = lockfile::build_user_lock_with_head_cache(
        &config.sources,
        &materialization.resolution,
        Some(&materialization),
        Some(&plugins),
        &mut source_head_cache,
    );
    let deliveries = materialization.deliveries;
    let resolution = materialization.resolution;
    let mut drift = lockfile::compare_user_lock(&previous_lock, &live_lock);
    suppress_initial_local_source_drift(&previous_lock, &mut drift);
    let assistant = crate::assistant::status(&paths);
    if assistant.state == crate::assistant::AssistantState::Current
        && !assistant.delivered_paths.is_empty()
    {
        // First-use companion delivery deliberately does not acknowledge a
        // whole-store sync. Its known administrative skill needs no such sync.
        drift.retain(|item| {
            !(item.code == crate::lockfile::LockDriftCode::ActiveAdded
                && item.subject == "local:dalo")
        });
    }
    let lock = LockStatus {
        path: paths.lock_file.clone(),
        schema_version: previous_lock.schema_version,
        drift,
    };
    let unmanaged_scan = crate::adopt::discover_unmanaged_skill_scan(&paths)?;

    let instruction_packs = instructions::discover_packs(
        &paths,
        &config.sources,
        &previous_lock.active_instruction_packs,
    );
    let active_packs = instruction_packs
        .iter()
        .filter(|pack| pack.enabled)
        .cloned()
        .collect::<Vec<_>>();
    let instruction_pack_overlaps = instructions::topic_overlaps(&active_packs);
    let instruction_block_drifts = instructions::instruction_block_drifts(
        &paths,
        &config.sources,
        &previous_lock.active_instruction_packs,
    );
    let autosync = crate::autosync::status(&paths).unwrap_or_else(|error| AutosyncStatusReport {
        configured: config.settings.autosync,
        installed: paths.autosync_file.exists(),
        enabled: false,
        backend: None,
        schedule: None,
        executable: None,
        store: None,
        identifier: None,
        artifacts: Vec::new(),
        scheduler_error: Some(format!("autosync state could not be inspected: {error}")),
        disabled_reason: None,
        last_run: None,
    });

    Ok(StatusReport {
        store: store_root.to_path_buf(),
        assistant,
        sources,
        targets,
        inventory_warnings,
        agent_inventory_warnings,
        plugin_inventory_warnings,
        plugins,
        installation_plan,
        tools,
        hooks,
        hook_targets,
        plugin_targets,
        resolution,
        binary_states,
        materialization: materialization.operations,
        deliveries,
        blocking_audits: audits.blocking,
        audit_failures: audits.failures,
        lock,
        source_lock_error,
        unmanaged_skills: unmanaged_scan.unmanaged_skills,
        target_warnings: unmanaged_scan.warnings,
        instruction_packs,
        instruction_pack_overlaps,
        instruction_block_drifts,
        autosync,
        dirty_sources: Vec::new(),
    })
}

/// Enabled tracking team sources whose checkout has local changes.
///
/// The next `sync` stops on such a source before it refreshes anything, so the
/// human status report names it. This costs one Git call per tracking source,
/// which JSON and `next` callers do not need to pay.
pub fn dirty_tracking_sources(store_root: &Path) -> DaloResult<Vec<String>> {
    let config = store::read_config(&StorePaths::new(store_root.to_path_buf()))?;
    Ok(config
        .sources
        .iter()
        .filter(|source| {
            source.enabled
                && source.kind == SourceKind::Team
                && source.update_policy.as_deref() == Some("track")
                && source.path.is_dir()
                && crate::git::is_dirty(&source.path).unwrap_or(false)
        })
        .map(|source| source.id.clone())
        .collect())
}

/// Build compact onboarding guidance from the current store state.
#[must_use = "the next action should be rendered or inspected"]
pub fn build_next_action_report(store_root: &Path) -> DaloResult<NextActionReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    if !paths.config_file.exists() {
        return Ok(NextActionReport {
            store: store_root.to_path_buf(),
            initialized: false,
            linked_targets: 0,
            sources: 0,
            active_skills: 0,
            pending_approvals: 0,
            state: NextActionState::Uninitialized,
            message: "Initialize a store before adding targets or skills.".to_owned(),
            command: Some(store::dalo_command(store_root, "init")),
        });
    }

    let report = build_status_report(store_root)?;
    let linked_targets = report.targets.iter().filter(|target| target.linked).count();
    let sources_with_skills = report.sources.iter().any(|source| source.skill_count > 0);
    let pending_approvals = report.resolution.pending_approval_skills.len();
    let (state, message, command) = if let Some(message) = next_health_attention_message(&report) {
        (
            NextActionState::NeedsAttention,
            message,
            Some(store::dalo_command(store_root, "status")),
        )
    } else if linked_targets == 0 {
        (
            NextActionState::NoTarget,
            if !report.assistant.delivered_paths.is_empty() {
                "The Dalo skill is available in your agent. Link a target to manage additional skills.".to_owned()
            } else {
                "Link a target so Dalo knows where to materialize skills.".to_owned()
            },
            Some(store::dalo_command(store_root, "target detect")),
        )
    } else if !sources_with_skills {
        let local_skills_dir = StorePaths::new(store_root.to_path_buf()).local_skills_dir;
        (
            NextActionState::NoSkills,
            format!(
                "No skills are available yet; add a team source, create one in {}, or adopt an existing skill.",
                local_skills_dir.display()
            ),
            Some(store::dalo_command(
                store_root,
                "source add <id> <git-url-or-path>",
            )),
        )
    } else if let Some(skill) = report.resolution.pending_approval_skills.first() {
        (
            NextActionState::PendingApproval,
            format!("Approve {} before it can be linked.", skill.source_ref),
            Some(store::dalo_command(
                store_root,
                &format!("approve skill {}", skill.source_ref),
            )),
        )
    } else if let Some(message) = next_blocker_attention_message(&report) {
        (
            NextActionState::NeedsAttention,
            message,
            Some(store::dalo_command(store_root, "status")),
        )
    } else if report
        .materialization
        .iter()
        .any(|operation| operation.status == MaterializeOperationStatus::Planned)
    {
        (
            NextActionState::SyncNeeded,
            "Materialized links need synchronization.".to_owned(),
            Some(store::dalo_command(store_root, "sync")),
        )
    } else if !report.lock.drift.is_empty() {
        (
            NextActionState::SyncNeeded,
            "The live skill set changed since the last synchronization.".to_owned(),
            Some(store::dalo_command(store_root, "sync")),
        )
    } else {
        (
            NextActionState::Synced,
            "All configured skills are synchronized.".to_owned(),
            None,
        )
    };

    Ok(NextActionReport {
        store: report.store,
        initialized: true,
        linked_targets,
        sources: report.sources.len(),
        active_skills: report.resolution.active_skills.len(),
        pending_approvals,
        state,
        message,
        command,
    })
}

fn next_health_attention_message(report: &StatusReport) -> Option<String> {
    let source_errors = report
        .sources
        .iter()
        .filter(|source| source.error.is_some())
        .map(|source| source.id.as_str())
        .collect::<Vec<_>>();
    if !source_errors.is_empty() {
        return Some(if source_errors.len() == 1 {
            format!(
                "Source `{}` could not be inspected; review its detailed status.",
                source_errors[0]
            )
        } else {
            format!(
                "Sources {} could not be inspected; review their detailed status.",
                source_errors
                    .iter()
                    .map(|source| format!("`{source}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
    }

    if !report.inventory_warnings.is_empty() {
        let source_paths = report
            .sources
            .iter()
            .map(|source| (source.id.clone(), source.path.clone()))
            .collect::<Vec<_>>();
        let warning_sources =
            inventory_warning_source_ids(&source_paths, &report.inventory_warnings);
        return Some(match warning_sources.as_slice() {
            [source] => format!(
                "Source `{source}` has inventory warnings; review its detailed status before synchronizing."
            ),
            [] => "The source inventory has warnings; review detailed status before synchronizing."
                .to_owned(),
            sources => format!(
                "Sources {} have inventory warnings; review their detailed status before synchronizing.",
                sources
                    .iter()
                    .map(|source| format!("`{source}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    if !report.agent_inventory_warnings.is_empty() {
        return Some(
            "The agent inventory has warnings; review detailed status before synchronizing."
                .to_owned(),
        );
    }
    if !report.plugin_inventory_warnings.is_empty() {
        return Some(
            "The plugin inventory has warnings; review detailed status before synchronizing."
                .to_owned(),
        );
    }
    if report.unmanaged_skills.iter().any(|skill| !skill.protected) {
        return Some(
            "Linked targets contain unmanaged skills; review detailed status before synchronizing."
                .to_owned(),
        );
    }
    if !report.instruction_block_drifts.is_empty() {
        return Some(
            "Managed instruction blocks are missing, malformed, or stale; review detailed status."
                .to_owned(),
        );
    }
    if autosync_needs_attention(&report.autosync) {
        return Some(
            "Scheduled synchronization is unhealthy; review its detailed status.".to_owned(),
        );
    }

    None
}

fn next_blocker_attention_message(report: &StatusReport) -> Option<String> {
    if !report.blocking_audits.is_empty()
        || !report.audit_failures.is_empty()
        || report
            .materialization
            .iter()
            .any(|operation| operation.status == MaterializeOperationStatus::Blocked)
        || report
            .resolution
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.requires_review())
        || report
            .plugins
            .plugins
            .iter()
            .any(|plugin| plugin.state == crate::plugin::PluginState::Blocked)
    {
        return Some(
            "The store has a blocker; inspect its detailed status before changing it.".to_owned(),
        );
    }
    None
}

fn inventory_warning_source_ids(
    sources: &[(String, PathBuf)],
    warnings: &[InventoryWarning],
) -> Vec<String> {
    let mut source_ids = std::collections::BTreeSet::new();
    for warning in warnings {
        let matches = sources
            .iter()
            .filter(|(_, path)| warning.path.starts_with(path))
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [source_id] => {
                source_ids.insert((*source_id).clone());
            }
            [] => {}
            _ => return Vec::new(),
        }
    }
    source_ids.into_iter().collect()
}

fn autosync_needs_attention(status: &AutosyncStatusReport) -> bool {
    status.scheduler_error.is_some()
        || (status.configured && !status.installed)
        || (status.installed
            && (!status.configured
                || !status.enabled
                || status
                    .executable
                    .as_ref()
                    .is_some_and(|path| !crate::autosync::executable_available(path))))
        || (status.installed
            && status.last_run.as_ref().is_some_and(|run| {
                run.outcome == crate::autosync::AutosyncRunOutcome::Blocked
                    || crate::autosync::running_run_is_stale(
                        run,
                        status.schedule,
                        crate::autosync::now_unix(),
                    )
            }))
}

fn suppress_initial_local_source_drift(
    previous_lock: &lockfile::UserLock,
    drift: &mut Vec<LockDrift>,
) {
    if previous_lock.sources.is_empty()
        && previous_lock.active_skills.is_empty()
        && previous_lock.pending_approval_skills.is_empty()
        && previous_lock.unlinked_skills.is_empty()
        && previous_lock.target_materializations.is_empty()
    {
        drift.retain(|entry| {
            !(entry.code == LockDriftCode::SourceAdded && entry.subject == "local")
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HumanPathRoot {
    path: PathBuf,
    label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HumanTargetRoot {
    path: PathBuf,
    label: String,
}

/// Invocation-local path labels for human output.
///
/// The context is deliberately independent of terminal detection and width so
/// redirected output stays identical to interactive output. Structured reports
/// keep their original absolute paths; only their human renderer uses labels.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HumanPathContext {
    roots: Vec<HumanPathRoot>,
    targets: Vec<HumanTargetRoot>,
    home: Option<PathBuf>,
}

impl HumanPathContext {
    fn home_only() -> Self {
        Self {
            roots: Vec::new(),
            targets: Vec::new(),
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    fn store_only(store: &Path) -> Self {
        Self::from_targets(store, std::iter::empty::<(PathBuf, Vec<String>)>())
    }

    fn for_resolve_list(store: &Path) -> Self {
        let paths = StorePaths::new(store.to_path_buf());
        let targets = store::read_state(&paths).map_or_else(
            |_| Vec::new(),
            |state| {
                state
                    .targets
                    .into_iter()
                    .map(|target| (target.path, vec![target.id]))
                    .collect()
            },
        );
        Self::from_targets(store, targets)
    }

    fn for_status(report: &StatusReport) -> Self {
        let mut targets = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        for target in &report.targets {
            targets
                .entry(store::comparable_path(&target.path))
                .or_default()
                .insert(target.id.clone());
        }
        for delivery in &report.deliveries {
            if let Some(root) = delivery.link_path.parent() {
                targets
                    .entry(store::comparable_path(root))
                    .or_default()
                    .extend(delivery.target_ids.iter().cloned());
            }
        }
        for operation in &report.materialization {
            if let Some(root) = operation.link_path.parent() {
                targets.entry(store::comparable_path(root)).or_default();
            }
        }
        Self::from_targets(
            &report.store,
            targets
                .into_iter()
                .map(|(path, identities)| (path, identities.into_iter().collect())),
        )
    }

    fn for_sync(report: &SyncReport) -> Self {
        let mut targets = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        for delivery in &report.deliveries {
            if let Some(root) = delivery.link_path.parent() {
                targets
                    .entry(store::comparable_path(root))
                    .or_default()
                    .extend(delivery.target_ids.iter().cloned());
            }
        }
        for operation in &report.operations {
            if let Some(root) = operation.link_path.parent() {
                targets.entry(store::comparable_path(root)).or_default();
            }
        }
        let targets = targets
            .into_iter()
            .map(|(path, identities)| (path, identities.into_iter().collect()));
        Self::from_targets(&report.store, targets)
    }

    fn for_doctor(report: &DoctorReport) -> Self {
        let paths = StorePaths::new(report.store.clone());
        let targets = store::read_state(&paths).map_or_else(
            |_| Vec::new(),
            |state| {
                state
                    .targets
                    .into_iter()
                    .map(|target| (target.path, vec![target.id]))
                    .collect()
            },
        );
        Self::from_targets(&report.store, targets)
    }

    fn from_targets(
        store: &Path,
        targets: impl IntoIterator<Item = (PathBuf, Vec<String>)>,
    ) -> Self {
        Self::from_targets_and_home(store, targets, std::env::var_os("HOME").map(PathBuf::from))
    }

    fn from_targets_and_home(
        store: &Path,
        targets: impl IntoIterator<Item = (PathBuf, Vec<String>)>,
        home: Option<PathBuf>,
    ) -> Self {
        let mut grouped = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        for (path, identities) in targets {
            grouped.entry(path).or_default().extend(identities);
        }

        let mut fallback = 0usize;
        let targets = grouped
            .into_iter()
            .map(|(path, identities)| {
                let identities = if identities.is_empty() {
                    fallback += 1;
                    format!("path-{fallback}")
                } else {
                    identities
                        .iter()
                        .map(|identity| terminal_safe_text(identity))
                        .collect::<Vec<_>>()
                        .join("+")
                };
                HumanTargetRoot {
                    path,
                    label: format!("target[{identities}]"),
                }
            })
            .collect::<Vec<_>>();

        let mut root_labels = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        for root in [store.to_path_buf(), store::comparable_path(store)] {
            root_labels
                .entry(root)
                .or_default()
                .insert("store".to_owned());
        }
        for target in &targets {
            for root in [target.path.clone(), store::comparable_path(&target.path)] {
                root_labels
                    .entry(root)
                    .or_default()
                    .insert(target.label.clone());
            }
        }
        let mut roots = root_labels
            .into_iter()
            .map(|(path, labels)| HumanPathRoot {
                path,
                label: labels.into_iter().collect::<Vec<_>>().join("+"),
            })
            .collect::<Vec<_>>();
        roots.sort_by(|left, right| {
            right
                .path
                .components()
                .count()
                .cmp(&left.path.components().count())
                .then_with(|| left.path.cmp(&right.path))
        });

        Self {
            roots,
            targets,
            home,
        }
    }

    fn path(&self, path: &Path) -> String {
        for root in &self.roots {
            if let Ok(relative) = path.strip_prefix(&root.path) {
                return labeled_path(&root.label, relative);
            }
        }
        let comparable = store::comparable_path(path);
        for root in &self.roots {
            if let Ok(relative) = comparable.strip_prefix(store::comparable_path(&root.path)) {
                return labeled_path(&root.label, relative);
            }
        }
        self.root(path)
    }

    fn root(&self, path: &Path) -> String {
        if let Some(home) = &self.home
            && let Ok(relative) = path.strip_prefix(home)
        {
            if relative.as_os_str().is_empty() {
                return "~".to_owned();
            }
            return format!("~/{}", terminal_safe_path(relative));
        }
        if let Some(home) = &self.home
            && let Ok(relative) =
                store::comparable_path(path).strip_prefix(store::comparable_path(home))
        {
            if relative.as_os_str().is_empty() {
                return "~".to_owned();
            }
            return format!("~/{}", terminal_safe_path(relative));
        }
        terminal_safe_path(path)
    }

    fn text(&self, value: &str) -> String {
        let mut rendered = terminal_safe_text(value);
        for root in &self.roots {
            rendered = replace_path_root(
                &rendered,
                &terminal_safe_path(&root.path),
                &format!("{}:", root.label),
            );
        }
        if let Some(home) = &self.home {
            rendered = replace_path_root(&rendered, &terminal_safe_path(home), "~");
            rendered = replace_path_root(
                &rendered,
                &terminal_safe_path(&store::comparable_path(home)),
                "~",
            );
        }
        rendered
    }

    fn target_roots(&self) -> &[HumanTargetRoot] {
        &self.targets
    }
}

/// Render a human-facing path with the current user's home directory compacted.
#[must_use]
pub fn compact_human_path(path: &Path) -> String {
    HumanPathContext::home_only().path(path)
}

/// Render human-facing text with the current user's home directory compacted.
#[must_use]
pub fn compact_human_text(value: &str) -> String {
    HumanPathContext::home_only().text(value)
}

/// Render human-facing text with store and home paths compacted.
#[must_use]
pub fn compact_store_text(store: &Path, value: &str) -> String {
    HumanPathContext::store_only(store).text(value)
}

/// Render a human-facing path with the store and home paths compacted.
#[must_use]
pub fn compact_store_path(store: &Path, path: &Path) -> String {
    HumanPathContext::store_only(store).path(path)
}

fn labeled_path(label: &str, relative: &Path) -> String {
    if relative.as_os_str().is_empty() {
        format!("{label}:/")
    } else {
        format!("{label}:/{}", terminal_safe_path(relative))
    }
}

fn replace_path_root(value: &str, root: &str, replacement: &str) -> String {
    if root.is_empty() {
        return value.to_owned();
    }
    let mut rendered = String::with_capacity(value.len());
    let mut previous_end = 0;
    for (index, _) in value.match_indices(root) {
        let after_index = index + root.len();
        let after = &value[after_index..];
        rendered.push_str(&value[previous_end..index]);
        let starts_at_boundary = index == 0
            || value[..index].chars().next_back().is_some_and(|character| {
                character.is_whitespace()
                    || matches!(character, '`' | '\'' | '"' | '(' | '[' | '{' | '=' | ':')
            });
        if starts_at_boundary && (after.is_empty() || after.starts_with('/')) {
            rendered.push_str(replacement);
        } else {
            rendered.push_str(root);
        }
        previous_end = after_index;
    }
    rendered.push_str(&value[previous_end..]);
    rendered
}

/// Print a human-readable init report.
pub fn print_init_report(report: &InitReport, next: Option<&NextActionReport>) {
    let paths = HumanPathContext::store_only(&report.store);
    println!("dalo store: {}", paths.root(&report.store));

    for operation in &report.operations {
        let status = format!("{:<8}", operation.status.as_str());
        println!(
            "{} {:<12} {}",
            term::operation_status(&status),
            operation.action.as_str(),
            paths.path(&operation.path)
        );
    }
    println!();

    let state_repaired = report.operations.iter().any(|operation| {
        operation
            .path
            .file_name()
            .is_some_and(|name| name == "state.toml")
            && operation.status == store::InitOperationStatus::Repaired
    });
    if state_repaired {
        println!("WARNING: state.toml was unreadable and was reset to empty state.");
        println!("A state.toml.corrupt-* backup was saved beside it.");
        println!("Restore target registrations, owned links, and protected slots before syncing.");
    }

    if !report.validation_warnings.is_empty() {
        println!("Store needs attention:");
        for warning in &report.validation_warnings {
            println!(
                "  warning {}: {}",
                paths.path(&warning.path),
                paths.text(&warning.message)
            );
        }
        println!("Fix the files above before using dalo.");
        return;
    }

    if state_repaired {
        return;
    }

    println!("Store ready.");
    let created_config = report.operations.iter().any(|operation| {
        operation
            .path
            .file_name()
            .is_some_and(|name| name == "config.toml")
            && operation.status == store::InitOperationStatus::Created
    });
    if created_config {
        println!("Next steps:");
        println!(
            "  1. {}",
            store::dalo_command(
                &report.store,
                &format!("target link <{}> [path]", crate::target::LINK_HINT_TARGETS)
            )
        );
        let local_skills_dir = StorePaths::new(report.store.clone()).local_skills_dir;
        println!("  2. Choose a skill path:");
        println!(
            "     team:     {}",
            store::dalo_command(&report.store, "source add <id> <git-url-or-path>")
        );
        println!(
            "     local:    create {}/<name>/SKILL.md",
            local_skills_dir.display()
        );
        println!(
            "     existing: {}",
            store::dalo_command(&report.store, "adopt <skill>")
        );
        println!("  3. {}", store::dalo_command(&report.store, "sync"));
    } else if let Some(next) = next {
        println!();
        print_next_action_report(next);
    }
}

/// Print a compact, state-aware entry-point report.
pub fn print_next_action_report(report: &NextActionReport) {
    let paths = HumanPathContext::store_only(&report.store);
    println!("Dalo");
    println!("  store: {}", paths.root(&report.store));
    println!(
        "  initialized: {}",
        if report.initialized { "yes" } else { "no" }
    );
    println!("  linked targets: {}", report.linked_targets);
    println!("  sources: {}", report.sources);
    println!("  active skills: {}", report.active_skills);
    println!("  pending approvals: {}", report.pending_approvals);
    println!();
    if let Some(command) = &report.command {
        println!("{}: {command}", term::next_label("Next"));
        println!("  {}", paths.text(&report.message));
    } else {
        println!("All synced ✓");
        println!("  {}", paths.text(&report.message));
    }
}

/// Print local approval records and persisted risk acceptances.
pub fn print_approval_list(report: &ApprovalListReport) {
    if report.approvals.is_empty() {
        println!("no approvals recorded");
    } else {
        for approval in &report.approvals {
            println!(
                "{} {}",
                term::terminal_safe_text(&approval.scope),
                term::terminal_safe_text(&approval.value)
            );
            match approval.granted_at_unix {
                Some(granted_at_unix) => {
                    println!("  granted at: {}", format_unix_utc(granted_at_unix));
                }
                None => println!("  grant date unavailable (legacy record)"),
            }
        }
    }

    if !report.accepted_risks.is_empty() {
        println!();
        println!("accepted-risk audits:");
        for acceptance in &report.accepted_risks {
            print_accepted_risk_summary(acceptance);
        }
        println!();
    }

    if !report.accepted_risks.is_empty()
        || report
            .approvals
            .iter()
            .any(|approval| approval.value.contains("@sha256:"))
    {
        println!(
            "legend: @sha256:... binds an approval to an exact reviewed contract or recipe hash"
        );
        println!(
            "accepted-risk scope bindings cover the exact content, audit engines, coverage, and findings"
        );
    }
}

fn print_accepted_risk_summary(acceptance: &AcceptedRiskSummary) {
    let state = if acceptance.active {
        "active"
    } else {
        "inactive"
    };
    println!(
        "  {} ({state})",
        term::terminal_safe_text(&acceptance.source_ref),
    );
    println!(
        "    content hash: {}",
        term::terminal_safe_text(&acceptance.content_hash)
    );
    println!(
        "    accepted at: {}: {}",
        format_unix_utc(acceptance.accepted_at_unix),
        term::terminal_safe_text(&acceptance.reason)
    );
    println!(
        "    scope binding: {}",
        term::terminal_safe_text(&acceptance.scope_hash)
    );
    println!(
        "    run: {}",
        term::terminal_safe_text(&acceptance.audit_command)
    );
}

/// Print one approval mutation result.
pub fn print_approval_report(report: &ApprovalReport, store_root: &Path) {
    let verb = if report.dry_run && report.action != "unchanged" {
        "planned"
    } else {
        report.action.as_str()
    };
    println!("{verb} {} {}", report.scope, report.value);
    print_approval_next_step(report, store_root);
}

/// Print a skill approval whose preflight was clean as one line, followed by
/// any short specification note. The full audit stays in `dalo audit`.
pub fn print_clean_skill_approval(report: &ApprovalReport, audit: &AuditReport, store_root: &Path) {
    let verb = match (report.dry_run, report.action.as_str()) {
        (_, "unchanged") => "already approved",
        (true, _) => "would approve",
        _ => "approved",
    };
    println!(
        "{verb} {} (checked, no findings)",
        terminal_safe_text(&report.value)
    );
    if let Some(note) = spec_note_line(store_root, audit) {
        println!("{note}");
    }
}

/// Print the sync hint that follows a skill approval change.
pub fn print_approval_next_step(report: &ApprovalReport, store_root: &Path) {
    if report.scope == "skill" && !report.dry_run && report.action != "unchanged" {
        let reason = if report.action == "revoked" {
            "to remove its links"
        } else {
            "to link it"
        };
        print_sync_next_step(store_root, reason);
    }
}

fn print_sync_next_step(store_root: &Path, reason: &str) {
    println!("next: {} {reason}", store::dalo_command(store_root, "sync"));
}

/// Whether a preflight result can be reported as one line instead of the full
/// audit block.
///
/// That holds for a clean, complete deterministic result without findings, an
/// agent review, a risk acceptance, or declared release binaries. Specification
/// notes are informational, outside the verdict, and summarized separately.
#[must_use]
pub fn audit_is_quiet(report: &AuditReport) -> bool {
    report.status == AuditStatus::Clean
        && report.coverage == AuditCoverage::Complete
        && report.static_findings.is_empty()
        && report.agent_review.is_none()
        && report.risk_acceptance.is_none()
        && report.binaries.is_empty()
}

/// One short line for a report's informational specification findings.
fn spec_note_line(store_root: &Path, report: &AuditReport) -> Option<String> {
    let first = report.spec_findings.first()?;
    let count = report.spec_findings.len();
    let command = store::dalo_command(
        store_root,
        &format!(
            "audit {}",
            crate::error::shell_quote_argument(&report.source_ref)
        ),
    );
    Some(format!(
        "{count} spec {}: {}{} ({} shows {})",
        if count == 1 { "note" } else { "notes" },
        terminal_safe_text(&first.message),
        if count > 1 {
            format!("; {} more", count - 1)
        } else {
            String::new()
        },
        terminal_safe_text(&command),
        if count == 1 { "it" } else { "them" }
    ))
}

/// Print full audit blocks for results that need a look, and one line for
/// the clean rest, as `source add` and `source select` report their preflight.
fn print_preflight_results(audits: &[AuditReport], store_root: &Path) {
    for audit in audits.iter().filter(|audit| !audit_is_quiet(audit)) {
        print_audit_report(audit);
    }
    let quiet = audits
        .iter()
        .filter(|audit| audit_is_quiet(audit))
        .collect::<Vec<_>>();
    if quiet.is_empty() {
        return;
    }
    if quiet.len() == audits.len() {
        println!("{} checked, no findings", skill_count_phrase(quiet.len()));
    } else {
        println!(
            "{} more {} checked, no findings",
            quiet.len(),
            if quiet.len() == 1 { "skill" } else { "skills" }
        );
    }
    print_spec_notes(&quiet, store_root);
}

fn print_spec_notes(audits: &[&AuditReport], store_root: &Path) {
    for audit in audits {
        if let Some(note) = spec_note_line(store_root, audit) {
            println!("  {}: {note}", terminal_safe_text(&audit.source_ref));
        }
    }
}

fn skill_count_phrase(count: usize) -> String {
    format!("{count} {}", if count == 1 { "skill" } else { "skills" })
}

/// Print a human-readable layered skill security audit.
pub fn print_audit_report(report: &AuditReport) {
    for line in audit_report_lines(report) {
        println!("{line}");
    }
}

fn audit_report_lines(report: &AuditReport) -> Vec<String> {
    let mut lines = vec![
        format!("security audit: {}", terminal_safe_text(&report.source_ref)),
        format!(
            "  content hash: {}",
            terminal_safe_text(&report.content_hash)
        ),
        format!(
            "  coverage: {}",
            match report.coverage {
                AuditCoverage::Complete => "complete",
                AuditCoverage::Partial => "partial",
            }
        ),
        format!(
            "  result: {}{}",
            match report.status {
                AuditStatus::Clean => "clean",
                AuditStatus::Review => "review",
                AuditStatus::Blocked => "blocked",
            },
            report
                .max_severity
                .map_or_else(String::new, |severity| format!(
                    " (max {})",
                    severity.as_str()
                ))
        ),
    ];
    for finding in &report.static_findings {
        lines.push(audit_finding_line("static", finding));
    }
    if !report.spec_findings.is_empty() {
        lines.push(format!(
            "  spec: {} finding(s); informational, not part of the result",
            report.spec_findings.len()
        ));
        for finding in &report.spec_findings {
            // The rule id is the only stable handle for a specification finding.
            lines.push(audit_finding_line(&format!("spec {}", finding.id), finding));
        }
    }
    for binary in &report.binaries {
        let platforms = binary
            .platforms
            .iter()
            .map(|platform| terminal_safe_text(platform))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "  binary {}: {}@{} ({platforms}) contract sha256:{}",
            terminal_safe_text(&binary.identity),
            terminal_safe_text(&binary.repo),
            terminal_safe_text(&binary.tag),
            terminal_safe_text(&binary.contract_hash)
        ));
    }
    if let Some(review) = &report.agent_review {
        lines.push(format!(
            "  agent review: {} (isolation: {}; non-authoritative)",
            review.provider.as_str(),
            review.isolation.as_str()
        ));
        lines.push(format!(
            "    assessment: {}",
            terminal_safe_text(agent_review_assessment(
                &review.summary,
                review.findings.len()
            ))
        ));
        lines.push(format!(
            "    additional findings: {}",
            review.findings.len()
        ));
        for capability in &review.expected_capabilities {
            lines.push(format!(
                "    capability: {}",
                terminal_safe_text(capability)
            ));
        }
        for action in &review.expected_actions {
            lines.push(format!(
                "    expected action: {}",
                terminal_safe_text(action)
            ));
        }
        for behavior in &review.undeclared_behaviors {
            lines.push(format!("    undeclared: {}", terminal_safe_text(behavior)));
        }
        for finding in &review.findings {
            lines.push(audit_finding_line("agent", finding));
        }
        lines.push(format!("    note: {}", agent_review_disclaimer()));
    }
    if let Some(acceptance) = &report.risk_acceptance {
        lines.push(format!(
            "  risk accepted: {}",
            terminal_safe_text(&acceptance.reason)
        ));
    } else if report.status == AuditStatus::Blocked {
        lines.push("  installation policy: blocked until risk is explicitly accepted".to_owned());
    }
    lines.push(
        "  note: no findings means no known issue was detected; it is not a safety guarantee"
            .to_owned(),
    );
    lines
}

fn agent_review_disclaimer() -> &'static str {
    "this review can add findings but cannot approve content; no additional findings are not an endorsement"
}

fn agent_review_assessment(summary: &str, findings_len: usize) -> &str {
    if findings_len == 0 {
        "no additional findings reported by the agent reviewer"
    } else {
        summary
    }
}

fn audit_finding_line(layer: &str, finding: &crate::audit::AuditFinding) -> String {
    let location = finding.line.map_or_else(
        || terminal_safe_text(&finding.path),
        |line| format!("{}:{line}", terminal_safe_text(&finding.path)),
    );
    format!(
        "  {} {} {} [{}]: {}",
        layer,
        finding.severity.as_str(),
        location,
        terminal_safe_text(&finding.category),
        terminal_safe_text(&finding.message)
    )
}

fn should_print_hook_target(target: &crate::hook_sync::HookTargetReport) -> bool {
    !crate::hook_sync::is_human_output_inert(target)
}

fn should_print_resolution_diagnostic(
    resolution: &Resolution,
    diagnostic: &resolver::ResolutionDiagnostic,
) -> bool {
    resolution.pending_approval_skills.is_empty()
        || diagnostic.code != resolver::ResolutionDiagnosticCode::PendingApproval
}

/// How much detail `dalo status` prints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusView {
    /// Include source provenance, empty counts, and an idle scheduler.
    pub all: bool,
}

/// One item that blocks or waits for the person running `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NeedsYouItem {
    /// What it is and why it needs attention.
    summary: String,
    /// Labeled, copyable commands that resolve it.
    commands: Vec<String>,
}

impl NeedsYouItem {
    fn run(summary: String, command: String) -> Self {
        Self {
            summary,
            commands: vec![format!("run: {command}")],
        }
    }
}

/// `review (local)` for a source-qualified skill or plugin reference.
fn source_ref_label(source_ref: &str) -> String {
    match source_ref.split_once(':') {
        Some((source, name)) => format!(
            "{} ({})",
            terminal_safe_text(name),
            terminal_safe_text(source)
        ),
        None => terminal_safe_text(source_ref),
    }
}

/// Drift the next sync simply records, grouped per subject with what changes.
fn waiting_for_sync_lines(drift: &[LockDrift]) -> Vec<(String, String)> {
    let mut lines: Vec<(String, String)> = Vec::new();
    for item in drift {
        let (subject, change) = match item.code {
            LockDriftCode::SourceAdded => (
                format!("{} (source)", terminal_safe_text(&item.subject)),
                "new source",
            ),
            LockDriftCode::SourceRemoved => (
                format!("{} (source)", terminal_safe_text(&item.subject)),
                "source removed",
            ),
            LockDriftCode::ActiveAdded => (source_ref_label(&item.subject), "new"),
            LockDriftCode::ActiveRemoved => (source_ref_label(&item.subject), "no longer active"),
            LockDriftCode::UnlinkedAdded => (source_ref_label(&item.subject), "now shadowed"),
            LockDriftCode::UnlinkedRemoved => {
                (source_ref_label(&item.subject), "no longer shadowed")
            }
            LockDriftCode::PendingApprovalAdded => {
                (source_ref_label(&item.subject), "waiting for approval")
            }
            LockDriftCode::PendingApprovalRemoved => (
                source_ref_label(&item.subject),
                "no longer waiting for approval",
            ),
            LockDriftCode::PluginAdded => {
                (format!("{} plugin", source_ref_label(&item.subject)), "new")
            }
            LockDriftCode::PluginRemoved => (
                format!("{} plugin", source_ref_label(&item.subject)),
                "removed",
            ),
            _ => (terminal_safe_text(&item.subject), "changed"),
        };
        if let Some((_, changes)) = lines.iter_mut().find(|(known, _)| *known == subject) {
            changes.push_str(", ");
            changes.push_str(change);
        } else {
            lines.push((subject, change.to_owned()));
        }
    }
    lines
}

/// Whether every drift item is one the next sync simply records.
#[must_use]
pub fn drift_awaits_sync(drift: &[LockDrift]) -> bool {
    !drift.is_empty() && drift.iter().all(|item| item.code.awaits_sync())
}

/// The drift items a person actually waits on a sync for.
///
/// A skill that newly waits for approval is waiting for approval, not for
/// sync: `needs you` and `pending approval` already list it with its approve
/// command, so it is left out of the waiting-for-sync count and section.
#[must_use]
pub fn waiting_for_sync_drift(drift: &[LockDrift]) -> Vec<LockDrift> {
    drift
        .iter()
        .filter(|item| item.code != LockDriftCode::PendingApprovalAdded)
        .cloned()
        .collect()
}

/// `1 skill`, `2 skills`, or `3 changes` waiting for the next sync.
#[must_use]
pub fn waiting_for_sync_phrase(drift: &[LockDrift]) -> String {
    let subjects = waiting_for_sync_lines(drift).len();
    let skills_only = drift.iter().all(|item| {
        matches!(
            item.code,
            LockDriftCode::ActiveAdded
                | LockDriftCode::ActiveRemoved
                | LockDriftCode::UnlinkedAdded
                | LockDriftCode::UnlinkedRemoved
                | LockDriftCode::PendingApprovalAdded
                | LockDriftCode::PendingApprovalRemoved
        )
    });
    let noun = match (skills_only, subjects == 1) {
        (true, true) => "skill",
        (true, false) => "skills",
        (false, true) => "change",
        (false, false) => "changes",
    };
    format!("{subjects} {noun} waiting for sync")
}

/// Everything that blocks or waits, in the order a person would handle it.
fn needs_you_items(report: &StatusReport, paths: &HumanPathContext) -> Vec<NeedsYouItem> {
    let store = &report.store;
    let mut items = Vec::new();

    // The same condition makes `status --check` report active skills without
    // linked targets; a companion already delivered to agent folders counts.
    let undelivered = report.resolution.active_skills.iter().any(|skill| {
        !(skill.source_ref == "local:dalo"
            && report.assistant.state == crate::assistant::AssistantState::Current
            && !report.assistant.delivered_paths.is_empty())
    });
    if undelivered && !report.targets.iter().any(|target| target.linked) {
        items.push(NeedsYouItem::run(
            "no agent folder is linked yet, so active skills reach no agent".to_owned(),
            store::dalo_command(store, "target detect"),
        ));
    }

    for skill in &report.resolution.pending_approval_skills {
        items.push(NeedsYouItem::run(
            format!(
                "{}: waiting for approval",
                source_ref_label(&skill.source_ref)
            ),
            store::dalo_command(store, &format!("approve skill {}", skill.source_ref)),
        ));
    }

    let conflicts = report
        .materialization
        .iter()
        .filter(|operation| is_unmanaged_entry_conflict(operation))
        .map(|operation| store::comparable_path(&operation.link_path))
        .collect::<BTreeSet<_>>();
    for skill in report
        .unmanaged_skills
        .iter()
        .filter(|skill| !skill.protected)
    {
        let occupant = unmanaged_occupant(&skill.path);
        let summary = if conflicts.contains(&store::comparable_path(&skill.path)) {
            occupant.description().to_owned()
        } else {
            match occupant {
                UnmanagedOccupant::Folder => "a folder Dalo does not manage".to_owned(),
                UnmanagedOccupant::Entry => "an entry Dalo does not manage".to_owned(),
            }
        };
        items.push(NeedsYouItem {
            summary: format!("{}: {summary}", paths.path(&skill.path)),
            commands: unmanaged_fix_lines(store, &skill.id, occupant),
        });
    }
    let listed_unmanaged = report
        .unmanaged_skills
        .iter()
        .map(|skill| store::comparable_path(&skill.path))
        .collect::<BTreeSet<_>>();
    for operation in report.materialization.iter().filter(|operation| {
        operation.status == MaterializeOperationStatus::Blocked
            && !listed_unmanaged.contains(&store::comparable_path(&operation.link_path))
            && !operation
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("required closure blocked"))
    }) {
        // An occupied slot that the unmanaged scan does not list, such as one
        // still recorded as owned, gets the same words but the inspection
        // command, because its ID is not one `resolve keep` accepts.
        let reason = if is_unmanaged_entry_conflict(operation) {
            unmanaged_occupant(&operation.link_path)
                .description()
                .to_owned()
        } else {
            paths.text(operation.reason.as_deref().unwrap_or("blocked"))
        };
        items.push(NeedsYouItem::run(
            format!("{}: {reason}", paths.path(&operation.link_path)),
            store::dalo_command(store, "resolve list"),
        ));
    }

    for blocked in &report.resolution.blocked_skills {
        items.push(NeedsYouItem {
            summary: format!(
                "{}: waits for `{}` ({})",
                source_ref_label(&blocked.skill.source_ref),
                terminal_safe_text(&blocked.requirement),
                resolver::closure_block_reason_name(blocked.reason)
            ),
            commands: Vec::new(),
        });
    }

    for source_ref in &report.blocking_audits {
        items.push(NeedsYouItem::run(
            format!(
                "{}: blocked by a security finding",
                source_ref_label(source_ref)
            ),
            store::dalo_command(store, &format!("audit {source_ref}")),
        ));
    }
    for failure in &report.audit_failures {
        items.push(NeedsYouItem::run(
            format!(
                "{}: security audit could not finish",
                source_ref_label(&failure.source_ref)
            ),
            store::dalo_command(store, &format!("audit {}", failure.source_ref)),
        ));
    }

    for source in report.sources.iter().filter(|source| {
        source.error.is_some()
            && !report
                .audit_failures
                .iter()
                .any(|failure| failure.source_id == source.id)
    }) {
        items.push(NeedsYouItem::run(
            format!(
                "{} (source): could not be inspected",
                terminal_safe_text(&source.id)
            ),
            store::dalo_command(store, "doctor"),
        ));
    }
    for source_id in &report.dirty_sources {
        if let Some(source) = report.sources.iter().find(|source| &source.id == source_id) {
            items.push(NeedsYouItem {
                summary: format!(
                    "{} (source): has local changes, which stop the next sync",
                    terminal_safe_text(source_id)
                ),
                commands: vec![format!(
                    "inspect: git -C {} status",
                    terminal_safe_text(&crate::error::shell_quote_path(&source.path))
                )],
            });
        }
    }

    let waiting = waiting_for_sync_drift(&report.lock.drift);
    if drift_awaits_sync(&report.lock.drift) && !waiting.is_empty() {
        let subjects = waiting_for_sync_lines(&waiting);
        let summary = match subjects.as_slice() {
            [(subject, _)] => format!("{subject}: waiting for sync"),
            _ => {
                const NAMED: usize = 3;
                let mut names = subjects
                    .iter()
                    .take(NAMED)
                    .map(|(subject, _)| subject.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                if subjects.len() > NAMED {
                    names.push_str(&format!(", and {} more", subjects.len() - NAMED));
                }
                format!("{}: {names}", waiting_for_sync_phrase(&waiting))
            }
        };
        items.push(NeedsYouItem::run(
            summary,
            store::dalo_command(store, "sync"),
        ));
    }
    items
}

fn print_needs_you(items: &[NeedsYouItem]) {
    if items.is_empty() {
        return;
    }
    println!("needs you:");
    for item in items {
        println!("  {}", item.summary);
        for command in &item.commands {
            println!("    {}", terminal_safe_text(command));
        }
    }
}

/// Print a human-readable status report.
///
/// `needs you` leads with every item that blocks or waits and its command, so
/// the first lines answer what needs attention. The sections below keep the
/// detail; `view.all` adds source provenance, empty counts, and an idle
/// scheduler. JSON output is unaffected.
pub fn print_status_report(report: &StatusReport, view: StatusView) {
    let paths = HumanPathContext::for_status(report);
    println!("dalo store: {}", paths.root(&report.store));
    print_needs_you(&needs_you_items(report, &paths));
    let unmanaged_conflicts = report
        .materialization
        .iter()
        .filter(|operation| is_unmanaged_entry_conflict(operation))
        .map(|operation| operation.link_path.as_path())
        .collect::<BTreeSet<_>>();
    print_delivery_reports(&report.deliveries, &paths, &unmanaged_conflicts);
    if !report.tools.tools.is_empty() {
        println!("local tools (inert inventory):");
        for tool in &report.tools.tools {
            println!(
                "  {} state={} contract=sha256:{}",
                terminal_safe_text(&tool.tool.source_ref),
                tool.state,
                terminal_safe_text(&tool.tool.contract_hash)
            );
            println!("    {}", terminal_safe_text(&tool.diagnostic));
        }
    }
    if !report.hooks.hooks.is_empty() {
        println!("portable hooks (inert until sync):");
        for hook in &report.hooks.hooks {
            println!(
                "  {} state={} tool_state={} contract=sha256:{}",
                terminal_safe_text(&hook.hook.source_ref),
                hook.state,
                hook.tool_state,
                terminal_safe_text(&hook.hook.contract_hash)
            );
            println!("    {}", terminal_safe_text(&hook.diagnostic));
        }
    }
    for target in &report.plugin_targets {
        let projection_hash = if target.projection_hash.is_empty() {
            "-".to_owned()
        } else {
            terminal_safe_text(&target.projection_hash)
        };
        println!(
            "native plugin {} {}: state={} path={} hash={} ({})",
            terminal_safe_text(&target.target),
            terminal_safe_text(&target.plugin),
            target.state,
            paths.path(&target.path),
            projection_hash,
            terminal_safe_text(&target.diagnostic)
        );
        for component in &target.components {
            println!(
                "  {} kind={} state={} ({})",
                terminal_safe_text(&component.identity),
                terminal_safe_text(&component.kind),
                component.state,
                terminal_safe_text(&component.diagnostic)
            );
        }
    }
    for target in &report.hook_targets {
        if !should_print_hook_target(target) {
            continue;
        }
        let action = target
            .action
            .map(|action| format!(" action={action}"))
            .unwrap_or_default();
        println!(
            "native hooks {}: state={}{} path={} ({})",
            target.target,
            target.state,
            action,
            paths.path(&target.path),
            paths.text(&target.diagnostic)
        );
    }
    // An unconfigured, uninstalled scheduler with no run history is the
    // default, not news; doctor still reports it as `autosync_not_installed`.
    if view.all
        || report.autosync.configured
        || report.autosync.installed
        || report.autosync.scheduler_error.is_some()
        || report.autosync.last_run.is_some()
    {
        print_autosync_status_report(&report.autosync);
    }
    println!("sources:");
    if report.sources.is_empty() {
        println!("  none");
    } else {
        for source in &report.sources {
            let error = source
                .error
                .as_ref()
                .map_or(String::new(), |error| format!(" ({error})"));
            let namespace = source
                .namespace
                .as_deref()
                .map_or(String::new(), |namespace| format!(" namespace={namespace}"));
            if view.all {
                let state = if source.enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                println!(
                    "  {:<12} {:<5} priority={:<4} skills={:<3} agents={:<3} plugins={:<3} {}{}{}",
                    source.id,
                    source.kind,
                    source.priority,
                    source.skill_count,
                    source.agent_count,
                    source.plugin_count,
                    state,
                    error,
                    namespace,
                );
                print_source_provenance(&source.provenance, "    ");
                continue;
            }
            let mut counts = format!("skills={}", source.skill_count);
            if source.agent_count > 0 {
                counts.push_str(&format!(" agents={}", source.agent_count));
            }
            if source.plugin_count > 0 {
                counts.push_str(&format!(" plugins={}", source.plugin_count));
            }
            println!(
                "  {:<12} {:<5} priority={:<4} {}{}{}{}",
                source.id,
                source.kind,
                source.priority,
                counts,
                if source.enabled { "" } else { " disabled" },
                error,
                namespace,
            );
        }
    }

    if !report.plugins.plugins.is_empty() {
        println!("plugins:");
        for plugin in &report.plugins.plugins {
            println!(
                "  {} state={}",
                terminal_safe_text(&plugin.source_ref),
                plugin.state
            );
            for reason in &plugin.blocking_reasons {
                println!("    blocked: {}", terminal_safe_text(reason));
            }
        }
    }
    if !report.plugins.diagnostics.is_empty() {
        println!("plugin diagnostics:");
        for diagnostic in &report.plugins.diagnostics {
            println!(
                "  {} {}: {}",
                diagnostic.code,
                terminal_safe_text(&diagnostic.subject),
                terminal_safe_text(&diagnostic.message)
            );
        }
    }

    println!("targets:");
    if report.targets.is_empty() {
        println!(
            "  none linked (run: {})",
            store::dalo_command(
                &report.store,
                &format!("target link <{}> [path]", crate::target::LINK_HINT_TARGETS)
            )
        );
    } else {
        for target in &report.targets {
            let state = if !target.linked {
                "unlinked"
            } else if target.exists {
                "linked"
            } else {
                "missing"
            };
            println!(
                "  {:<12} {:<7} {}",
                target.id,
                state,
                paths.root(&target.path)
            );
        }
    }

    println!("active skills:");
    if report.resolution.active_skills.is_empty() {
        println!("  none");
    } else {
        const HUMAN_LIST_LIMIT: usize = 20;
        for skill in report
            .resolution
            .active_skills
            .iter()
            .take(HUMAN_LIST_LIMIT)
        {
            let marker = if skill.local_override {
                " local_override"
            } else {
                ""
            };
            println!("  {} -> {}{}", skill.slot_name, skill.source_ref, marker);
            for binary in &skill.binaries {
                let state = report
                    .binary_states
                    .iter()
                    .find(|item| item.binary.source_ref == binary.source_ref)
                    .map_or_else(|| "unknown".to_owned(), |item| item.state.to_string());
                println!(
                    "    binary {}: {state}",
                    terminal_safe_text(&binary.source_ref)
                );
            }
        }
        let omitted = report
            .resolution
            .active_skills
            .len()
            .saturating_sub(HUMAN_LIST_LIMIT);
        if omitted > 0 {
            let skill_word = if omitted == 1 { "skill" } else { "skills" };
            println!("  … {omitted} more active {skill_word} (use --json for the full inventory)");
        }
    }

    // `needs you` already carries each approval command.
    if !report.resolution.pending_approval_skills.is_empty() {
        println!("pending approval:");
        for skill in &report.resolution.pending_approval_skills {
            println!("  {} -> {}", skill.slot_name, skill.source_ref);
            if let Some(compatibility) = &skill.compatibility {
                println!("    compatibility: {}", terminal_safe_text(compatibility));
            }
        }
    }

    if !report.resolution.unlinked_skills.is_empty() {
        println!("unlinked skills:");
        for skill in &report.resolution.unlinked_skills {
            println!(
                "  {} -> {} reason=shadowed by={}",
                skill.skill.slot_name, skill.skill.source_ref, skill.shadowed_by
            );
        }
    }

    if !report.resolution.blocked_skills.is_empty() {
        println!("blocked skills (required closure not linkable):");
        for blocked in &report.resolution.blocked_skills {
            println!(
                "  {} -> {} requires=`{}` reason={}",
                blocked.skill.slot_name,
                blocked.skill.source_ref,
                blocked.requirement,
                resolver::closure_block_reason_name(blocked.reason)
            );
        }
    }

    if !report.blocking_audits.is_empty() {
        println!("security audit blocks:");
        for source_ref in &report.blocking_audits {
            println!(
                "  {source_ref} (run: {}; accept only with an explicit --accept-risk reason)",
                store::dalo_command(&report.store, &format!("audit {source_ref}"))
            );
        }
    }

    if !report.audit_failures.is_empty() {
        println!("security audit failures:");
        for failure in &report.audit_failures {
            println!("  {}: {}", failure.source_ref, failure.reason);
        }
    }

    // A real entry in a slot is reported once under `needs you`, with both
    // ways out; this section keeps the other blocked operations.
    let blocked_operations = report
        .materialization
        .iter()
        .filter(|operation| {
            operation.status == MaterializeOperationStatus::Blocked
                && !unmanaged_conflicts.contains(operation.link_path.as_path())
        })
        .collect::<Vec<_>>();
    if !blocked_operations.is_empty() {
        println!("materialization blocks:");
        for operation in blocked_operations {
            let reason = operation.reason.as_deref().unwrap_or("blocked");
            println!(
                "  {}: {}",
                paths.path(&operation.link_path),
                paths.text(reason)
            );
        }
    }

    if report
        .resolution
        .diagnostics
        .iter()
        .any(|diagnostic| should_print_resolution_diagnostic(&report.resolution, diagnostic))
    {
        println!("resolution diagnostics:");
        for diagnostic in
            report.resolution.diagnostics.iter().filter(|diagnostic| {
                should_print_resolution_diagnostic(&report.resolution, diagnostic)
            })
        {
            println!(
                "  {}: {}",
                resolver::diagnostic_code_name(diagnostic.code),
                store::contextualize_dalo_commands(&report.store, &diagnostic.message)
            );
        }
    }

    if drift_awaits_sync(&report.lock.drift) {
        // Additions and removals the next sync records are not drift in the
        // sense of something moving underneath the lock. A skill waiting for
        // approval is listed under `pending approval` instead.
        let lines = waiting_for_sync_lines(&waiting_for_sync_drift(&report.lock.drift));
        if !lines.is_empty() {
            println!("waiting for sync:");
        }
        let width = lines
            .iter()
            .map(|(subject, _)| subject.chars().count())
            .max()
            .unwrap_or(0);
        for (subject, change) in lines {
            let padding = " ".repeat(width.saturating_sub(subject.chars().count()));
            println!("  {subject}{padding}  {change}");
        }
    } else if !report.lock.drift.is_empty() {
        println!("lock drift:");
        for drift in &report.lock.drift {
            println!("  {} {}: {}", drift.code, drift.subject, drift.message);
        }
    }

    if let Some(error) = &report.source_lock_error {
        println!("source lock error: {}", term::terminal_safe_text(error));
    }

    if !report.unmanaged_skills.is_empty() {
        println!("unmanaged skills:");
        for skill in &report.unmanaged_skills {
            if skill.protected {
                print_unmanaged_skill_with_repair_hint(skill, &report.store, Some(&paths));
            } else {
                // `needs you` carries the commands for every unprotected entry.
                println!(
                    "  {} -> {}",
                    terminal_safe_text(&skill.id),
                    paths.path(&skill.path)
                );
            }
        }
    }

    if !report.inventory_warnings.is_empty() {
        print_inventory_warnings(
            &report.inventory_warnings,
            Some(&report.store),
            Some(&paths),
        );
    }

    if !report.agent_inventory_warnings.is_empty() {
        println!("agent inventory warnings:");
        for warning in &report.agent_inventory_warnings {
            println!(
                "  {} {}: {}",
                warning.code,
                paths.path(&warning.path),
                paths.text(&warning.message)
            );
        }
    }

    if !report.plugin_inventory_warnings.is_empty() {
        println!("plugin inventory warnings:");
        for warning in &report.plugin_inventory_warnings {
            println!(
                "  {} {}: {}",
                warning.code,
                paths.path(&warning.path),
                paths.text(&warning.message)
            );
        }
    }

    if !report.target_warnings.is_empty() {
        println!("target warnings:");
        for warning in &report.target_warnings {
            println!(
                "  {} {}: {}",
                warning.code.as_str(),
                paths.path(&warning.path),
                paths.text(&warning.message)
            );
        }
    }

    if !report.instruction_packs.is_empty() {
        println!("instruction packs:");
        for pack in &report.instruction_packs {
            let state = if pack.enabled { "enabled" } else { "available" };
            println!("  {} ({state})", pack.pack_ref());
        }
    }

    if !report.instruction_pack_overlaps.is_empty() {
        println!("instruction pack topic overlaps:");
        for overlap in &report.instruction_pack_overlaps {
            println!(
                "  {} <-> {} share: {}",
                overlap.packs[0],
                overlap.packs[1],
                overlap.topics.join(", ")
            );
        }
    }

    if !report.instruction_block_drifts.is_empty() {
        println!("instruction block drift:");
        for drift in &report.instruction_block_drifts {
            println!(
                "  {}:{} {} at {} ({})",
                drift.source_id,
                drift.pack_id,
                instruction_block_drift_kind_label(drift.kind),
                paths.path(&drift.target),
                paths.text(&drift.message)
            );
        }
    }
}

fn print_delivery_reports(
    deliveries: &[SkillDeliveryReport],
    paths: &HumanPathContext,
    suppressed_paths: &BTreeSet<&Path>,
) {
    let visible = deliveries
        .iter()
        .filter(|delivery| {
            (delivery.mode != crate::inventory::SkillDeliveryMode::Direct || delivery.blocked)
                && !suppressed_paths.contains(delivery.link_path.as_path())
        })
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return;
    }
    println!("skill delivery:");
    for delivery in visible {
        let provider = delivery.provider.as_deref().unwrap_or("-");
        let artifact = delivery
            .artifact_path
            .as_ref()
            .map_or_else(|| "-".to_owned(), |path| paths.path(path));
        println!(
            "  {} targets={} mode={} provider={} artifact={}{}",
            delivery.source_ref,
            delivery.target_ids.join(","),
            delivery.mode.as_str(),
            provider,
            artifact,
            if delivery.blocked { " blocked" } else { "" }
        );
        if let Some(reason) = &delivery.reason {
            println!("    {reason}");
        }
        if let Some(output) = &delivery.planned_output {
            println!("    planned output: {}", output.display());
        }
        if let Some(cache_state) = delivery.cache_state {
            println!("    generated cache: {}", cache_state.as_str());
        }
        if let Some(derivation_hash) = &delivery.derivation_hash {
            println!("    derivation: sha256:{derivation_hash}");
        }
    }
}

/// Format a Unix timestamp (seconds) as a UTC calendar time such as
/// `2026-07-17 09:34:09 UTC`, avoiding a calendar dependency.
fn format_unix_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    // Howard Hinnant's civil-from-days algorithm for the proleptic Gregorian
    // calendar. `secs` is unsigned, so the era is always non-negative.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

/// Print scheduler installation and latest durable run status.
pub fn print_autosync_status_report(report: &AutosyncStatusReport) {
    if report.configured != report.installed {
        println!(
            "autosync: configuration mismatch (configured={}, installed={})",
            report.configured, report.installed
        );
    }
    if !report.installed {
        println!("autosync: not installed");
    } else {
        println!(
            "autosync: {} via {} ({})",
            if report.enabled {
                "enabled"
            } else {
                "disabled"
            },
            report.backend.map_or("unknown", |backend| backend.as_str()),
            report
                .schedule
                .map_or("unknown", |schedule| schedule.as_str())
        );
        if !report.enabled
            && let Some(reason) = &report.disabled_reason
        {
            println!("  disabled: {reason}");
        }
        for artifact in &report.artifacts {
            println!("  artifact: {artifact}");
        }
    }
    if let Some(error) = &report.scheduler_error {
        println!("  scheduler error: {error}");
    }
    if let Some(run) = &report.last_run {
        println!(
            "  last run: {} at {}",
            run.outcome.as_str(),
            format_unix_utc(run.last_attempted_at_unix)
        );
        // Only flag staleness for an installed job: without an install the real
        // schedule is unknown, so the daily fallback interval could misfire on a
        // leftover run-state from a previously weekly schedule. Matches the
        // `installed` gate in `doctor` and `status --check`.
        if report.installed
            && crate::autosync::running_run_is_stale(
                run,
                report.schedule,
                crate::autosync::now_unix(),
            )
        {
            println!(
                "  warning: this run started but never recorded a terminal outcome; it was likely interrupted"
            );
        }
        if let Some(success) = run.last_successful_at_unix {
            println!("  last success: {}", format_unix_utc(success));
        }
        if let Some(reason) = &run.reason {
            println!("  reason: {reason}");
        }
    }
}

/// Print install or uninstall result followed by resulting status.
pub fn print_autosync_mutation_report(report: &AutosyncMutationReport) {
    println!("autosync: {}", report.action);
    print_autosync_status_report(&report.status);
}

/// Facts about one sync run that only its human-readable report uses.
///
/// The JSON report is unchanged; this only decides which steady-state lines a
/// person needs to see again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncOutputContext {
    /// The security preflight covered at least one active skill that is new to
    /// the lock, comes from a source whose recorded commit moved, or had no
    /// cached result at its exact content hash.
    pub preflight_covered_changes: bool,
    /// `local_override` and `shadowed` diagnostics whose slot resolution the
    /// previous lock already recorded, as `(code, source_ref)` pairs.
    pub settled_diagnostics: BTreeSet<(String, String)>,
}

impl SyncOutputContext {
    /// Compare a sync's resolution with the lock it started from.
    ///
    /// `current_sources` are the sources of the lock this sync writes; a dry run
    /// that builds no lock passes `None` and only compares skills and audits.
    #[must_use]
    pub fn new(
        previous: &lockfile::UserLock,
        current_sources: Option<&[lockfile::LockedSource]>,
        resolution: &Resolution,
        fresh_audits: &[String],
    ) -> Self {
        let previous_active = previous
            .active_skills
            .iter()
            .map(|skill| (skill.slot_name.as_str(), skill.source_ref.as_str()))
            .collect::<BTreeSet<_>>();
        let previous_active_refs = previous
            .active_skills
            .iter()
            .map(|skill| skill.source_ref.as_str())
            .collect::<BTreeSet<_>>();
        let previous_unlinked = previous
            .unlinked_skills
            .iter()
            .map(|skill| skill.source_ref.as_str())
            .collect::<BTreeSet<_>>();
        let moved_sources = current_sources.map_or_else(BTreeSet::new, |current| {
            current
                .iter()
                .filter(|source| {
                    previous
                        .sources
                        .iter()
                        .find(|recorded| recorded.id == source.id)
                        .is_none_or(|recorded| recorded.commit != source.commit)
                })
                .map(|source| source.id.as_str())
                .collect::<BTreeSet<_>>()
        });
        let preflight_covered_changes = !fresh_audits.is_empty()
            || resolution.active_skills.iter().any(|skill| {
                !previous_active_refs.contains(skill.source_ref.as_str())
                    || moved_sources.contains(skill.source_id.as_str())
            });

        let mut settled_diagnostics = BTreeSet::new();
        for diagnostic in &resolution.diagnostics {
            let Some(source_ref) = diagnostic.source_ref.as_deref() else {
                continue;
            };
            let settled = match diagnostic.code {
                resolver::ResolutionDiagnosticCode::Shadowed => {
                    previous_unlinked.contains(source_ref)
                        && resolution
                            .unlinked_skills
                            .iter()
                            .filter(|unlinked| unlinked.skill.source_ref == source_ref)
                            .all(|unlinked| {
                                previous_active.contains(&(
                                    unlinked.skill.slot_name.as_str(),
                                    unlinked.shadowed_by.as_str(),
                                ))
                            })
                }
                resolver::ResolutionDiagnosticCode::LocalOverride => {
                    previous_active_refs.contains(source_ref)
                        && resolution
                            .unlinked_skills
                            .iter()
                            .filter(|unlinked| unlinked.shadowed_by == source_ref)
                            .all(|unlinked| {
                                previous_unlinked.contains(unlinked.skill.source_ref.as_str())
                            })
                }
                _ => false,
            };
            if settled {
                settled_diagnostics.insert((
                    resolver::diagnostic_code_name(diagnostic.code).to_owned(),
                    source_ref.to_owned(),
                ));
            }
        }
        Self {
            preflight_covered_changes,
            settled_diagnostics,
        }
    }

    fn is_settled(&self, diagnostic: &resolver::ResolutionDiagnostic) -> bool {
        diagnostic.source_ref.as_deref().is_some_and(|source_ref| {
            self.settled_diagnostics.contains(&(
                resolver::diagnostic_code_name(diagnostic.code).to_owned(),
                source_ref.to_owned(),
            ))
        })
    }
}

/// Print a human-readable sync report.
pub fn print_sync_report(report: &SyncReport, context: &SyncOutputContext) {
    let paths = HumanPathContext::for_sync(report);
    println!("dalo store: {}", paths.root(&report.store));
    for target in paths.target_roots() {
        println!("{}: {}", target.label, paths.root(&target.path));
    }
    let unmanaged_conflicts = report
        .operations
        .iter()
        .filter(|operation| is_unmanaged_entry_conflict(operation))
        .map(|operation| operation.link_path.as_path())
        .collect::<BTreeSet<_>>();
    print_delivery_reports(&report.deliveries, &paths, &unmanaged_conflicts);
    if report.operations.is_empty() {
        if !report.instruction_operations.is_empty()
            || !report.instruction_removal_operations.is_empty()
        {
            println!("skill links unchanged");
        } else if report.linked_targets == 0 && !report.resolution.active_skills.is_empty() {
            println!(
                "nothing materialized: {} skills resolved but no targets are linked; run `{}`",
                report.resolution.active_skills.len(),
                store::dalo_command(
                    &report.store,
                    &format!("target link <{}> [path]", crate::target::LINK_HINT_TARGETS)
                )
            );
        } else if report.resolution.pending_approval_skills.is_empty()
            && report.resolution.blocked_skills.is_empty()
            && !report
                .resolution
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.requires_review())
            && report.degraded_sources.is_empty()
        {
            println!("nothing to sync: 0 skills materialized; store is up to date");
            for catalog in &report.unselected_catalogs {
                println!(
                    "note: catalog `{}` has {} available {}, none selected; run `{}`",
                    catalog.source_id,
                    catalog.available_skills,
                    if catalog.available_skills == 1 {
                        "skill"
                    } else {
                        "skills"
                    },
                    store::dalo_command(
                        &report.store,
                        &format!("source select {} <skill>", catalog.source_id)
                    )
                );
            }
        } else {
            println!("nothing materialized: resolution is incomplete");
        }
    } else {
        for operation in &report.operations {
            let status = format!("{:<8}", operation.status.as_str());
            let kind = format!("{:<10}", operation.kind.as_str());
            if is_unmanaged_entry_conflict(operation) {
                let occupant = unmanaged_occupant(&operation.link_path);
                println!(
                    "{} {} {}: {}",
                    term::operation_status(&status),
                    term::operation_status(&kind),
                    paths.path(&operation.link_path),
                    occupant.description()
                );
                for line in unmanaged_fix_lines(
                    &report.store,
                    &unmanaged_selector(&operation.link_path).to_string_lossy(),
                    occupant,
                ) {
                    println!("{:20}{}", "", terminal_safe_text(&line));
                }
                continue;
            }
            let desired = operation
                .desired_path
                .as_ref()
                .map_or(String::new(), |path| format!(" -> {}", paths.path(path)));
            let reason = operation
                .reason
                .as_ref()
                .map_or(String::new(), |reason| format!(" ({})", paths.text(reason)));
            println!(
                "{} {} {}{}{}",
                term::operation_status(&status),
                term::operation_status(&kind),
                paths.path(&operation.link_path),
                desired,
                reason
            );
        }
        print_sync_summary(report);
    }
    print_missing_skill_commands(&report.missing_commands);
    let prefix = if report.operations.is_empty() {
        "  "
    } else {
        ""
    };
    for target in &report.hook_targets {
        if !should_print_hook_target(target) {
            continue;
        }
        let action = target
            .action
            .map(|action| format!(" action={action}"))
            .unwrap_or_default();
        println!(
            "{prefix}hooks {}: state={}{} projected={} ({})",
            target.target, target.state, action, target.projected_hooks, target.diagnostic
        );
    }
    for target in &report.plugin_targets {
        println!(
            "{prefix}plugin {} {}: state={} path={} ({})",
            terminal_safe_text(&target.target),
            terminal_safe_text(&target.plugin),
            target.state,
            paths.path(&target.path),
            paths.text(&target.diagnostic)
        );
    }
    for skill in &report.resolution.pending_approval_skills {
        println!(
            "{prefix}pending approval: {} (run: {})",
            skill.source_ref,
            store::dalo_command(
                &report.store,
                &format!("approve skill {}", skill.source_ref)
            )
        );
        if let Some(compatibility) = &skill.compatibility {
            println!(
                "{prefix}  compatibility: {}",
                terminal_safe_text(compatibility)
            );
        }
    }
    for blocked in &report.resolution.blocked_skills {
        println!(
            "{prefix}blocked: {} requires {}",
            blocked.skill.source_ref, blocked.requirement
        );
    }
    for source in &report.degraded_sources {
        println!("{prefix}degraded source: {} ({})", source.id, source.reason);
    }
    if !report.inventory_warnings.is_empty() {
        print_inventory_warnings(
            &report.inventory_warnings,
            Some(&report.store),
            Some(&paths),
        );
    }
    for operation in &report.instruction_operations {
        println!(
            "{prefix}instruction {}: {}:{} -> {} ({})",
            operation.action,
            operation.source_id,
            operation.pack_id,
            paths.path(&operation.target),
            operation.commit
        );
    }
    for operation in &report.instruction_removal_operations {
        println!(
            "{prefix}instruction {}: {}:{} -> {}",
            operation.action,
            operation.source_id,
            operation.pack_id,
            paths.path(&operation.target)
        );
        if let Some(warning) = &operation.warning {
            println!("{prefix}  warning: {warning}");
        }
    }
    for catalog in &report.manifest_catalogs {
        println!(
            "{prefix}catalog: {} policy={} requested={} {} -> {} (upstream {})",
            catalog.source_id,
            catalog.update_policy.as_str(),
            catalog.requested_ref,
            catalog.old_commit.as_deref().unwrap_or("none"),
            catalog.commit,
            if catalog.upstream_checked {
                "checked"
            } else {
                "not checked"
            }
        );
    }
    if !report.unrefreshed_tracking_sources.is_empty() {
        println!(
            "{prefix}note: --dry-run did not refresh tracking {}; upstream changes are not reflected; run `{}` to fetch {}",
            pluralized_source_list(&report.unrefreshed_tracking_sources),
            store::dalo_command(&report.store, "sync"),
            if report.unrefreshed_tracking_sources.len() == 1 {
                "it"
            } else {
                "them"
            }
        );
    }
    // A local override or shadowing the previous sync already recorded is a
    // steady state the user chose; it stays visible in `status`.
    for diagnostic in report.resolution.diagnostics.iter().filter(|diagnostic| {
        should_print_resolution_diagnostic(&report.resolution, diagnostic)
            && !context.is_settled(diagnostic)
    }) {
        println!(
            "{prefix}diagnostic: {}: {}",
            resolver::diagnostic_code_name(diagnostic.code),
            store::contextualize_dalo_commands(&report.store, &diagnostic.message)
        );
    }
    // The trailer qualifies a preflight this run performed on new or changed
    // content; an unchanged sync has nothing new to qualify.
    if context.preflight_covered_changes && !report.resolution.active_skills.is_empty() {
        println!("{prefix}security preflight: deterministic checks only");
    }
}

fn print_sync_summary(report: &SyncReport) {
    let mut created = 0;
    let mut relinked = 0;
    let mut removed = 0;
    let mut unchanged = 0;
    let mut planned = 0;
    let mut blocked = 0;
    for operation in &report.operations {
        match operation.status {
            crate::materialize::MaterializeOperationStatus::Applied => match operation.kind {
                crate::materialize::MaterializeOperationKind::Create => created += 1,
                crate::materialize::MaterializeOperationKind::Relink => relinked += 1,
                crate::materialize::MaterializeOperationKind::Remove => removed += 1,
                _ => {}
            },
            crate::materialize::MaterializeOperationStatus::Existing => unchanged += 1,
            crate::materialize::MaterializeOperationStatus::Planned => planned += 1,
            crate::materialize::MaterializeOperationStatus::Blocked => blocked += 1,
        }
    }
    let mut outcomes = Vec::new();
    if created > 0 {
        outcomes.push(format!("{created} created"));
    }
    if relinked > 0 {
        outcomes.push(format!("{relinked} relinked"));
    }
    if removed > 0 {
        outcomes.push(format!("{removed} removed"));
    }
    if unchanged > 0 {
        outcomes.push(format!("{unchanged} unchanged"));
    }
    if planned > 0 {
        outcomes.push(format!("{planned} planned"));
    }
    if blocked > 0 {
        outcomes.push(format!("{blocked} blocked"));
    }
    if outcomes.is_empty() {
        outcomes.push("no link changes".to_owned());
    }
    let verb = if report.dry_run {
        "would sync"
    } else {
        "synced"
    };
    println!(
        "{verb}: {} {} across {} {} ({})",
        report.resolution.active_skills.len(),
        if report.resolution.active_skills.len() == 1 {
            "skill"
        } else {
            "skills"
        },
        report.linked_targets,
        if report.linked_targets == 1 {
            "target"
        } else {
            "targets"
        },
        outcomes.join(", ")
    );
}

/// Print one note listing declared skill commands that are not on `PATH`.
///
/// Sync and install print this once, after the summary. Each entry names the
/// skill and command, followed by its `compatibility` text as the install hint.
fn print_missing_skill_commands(missing: &[MissingSkillCommand]) {
    if missing.is_empty() {
        return;
    }
    println!(
        "note: {} declared command(s) missing from PATH:",
        missing.len()
    );
    for entry in missing {
        let hint = entry
            .compatibility
            .as_deref()
            .map_or_else(|| "no install hint declared".to_owned(), terminal_safe_text);
        println!(
            "  {} wants {}: {hint}",
            terminal_safe_text(&entry.source_ref),
            terminal_safe_text(&entry.command),
        );
    }
}

fn pluralized_source_list(ids: &[String]) -> String {
    let source_word = if ids.len() == 1 { "source" } else { "sources" };
    format!(
        "{source_word} {}",
        ids.iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Print a human-readable source add report.
pub fn print_source_add_report(report: &SourceAddReport) {
    print_source_add_report_inner(report, None);
}

/// Print a review-first project source declaration preview or result.
pub fn print_project_add_report(report: &crate::project::ProjectAddReport) {
    let verb = if report.applied { "added" } else { "would add" };
    println!(
        "{verb} project source {} from {}",
        terminal_safe_text(&report.source_id),
        terminal_safe_text(&report.url)
    );
    println!(
        "  project: {}",
        terminal_safe_text(&report.project.display().to_string())
    );
    println!(
        "  ref: {} -> {}",
        terminal_safe_text(&report.requested_ref),
        terminal_safe_text(&report.commit)
    );
    println!("  skills:");
    for skill in &report.skills {
        let identity = skill.id.as_deref().unwrap_or(&skill.path);
        println!(
            "    {} ({}){}",
            terminal_safe_text(identity),
            terminal_safe_text(&skill.path),
            skill
                .description
                .as_deref()
                .map(|description| format!(": {}", terminal_safe_text(description)))
                .unwrap_or_default()
        );
    }
    println!("  targets:");
    for target in &report.targets {
        println!(
            "    {} -> {}",
            terminal_safe_text(&target.id),
            terminal_safe_text(&target.directory)
        );
    }
    if !report.applied {
        println!("  declaration change:");
        for line in report.declaration_change.lines() {
            println!("    {}", terminal_safe_text(line));
        }
    }
    println!("next: {}", terminal_safe_text(&report.next_command));
    if report.dry_run {
        println!("note: --dry-run; no project files were changed");
    }
}

/// Print the declaration change and local effects of a project removal.
pub fn print_project_remove_report(report: &crate::project::ProjectRemoveReport) {
    let verb = if report.applied {
        "updated"
    } else {
        "would update"
    };
    println!(
        "{verb} project declaration for {}",
        terminal_safe_text(&report.source_id)
    );
    println!(
        "  selection: {:?} -> {:?}",
        report
            .selection_before
            .iter()
            .map(|value| terminal_safe_text(value))
            .collect::<Vec<_>>(),
        report
            .selection_after
            .iter()
            .map(|value| terminal_safe_text(value))
            .collect::<Vec<_>>()
    );
    print_project_removal_effects(&report.effects);
    println!("\n{}", terminal_safe_text(&report.declaration_change));
    println!("{}", terminal_safe_text(&report.next_command));
}

/// Print local effects without claiming that unavailable pins were resolved.
pub fn print_project_removal_effects(effects: &crate::project::ProjectRemovalEffects) {
    for checkout in &effects.retained_checkouts {
        println!(
            "  retain checkout: {}{}",
            terminal_safe_text(&checkout.path.display().to_string()),
            if checkout.dirty { " (local edits)" } else { "" }
        );
    }
    for skill in &effects.deactivated_skills {
        println!("  stop delivery: {}", terminal_safe_text(skill));
    }
    println!(
        "  source-scoped approvals to revoke: {}",
        effects.approvals_to_revoke
    );
    for operation in &effects.link_operations {
        println!(
            "  {} {} [{}]{}",
            operation.kind.as_str(),
            terminal_safe_text(&operation.link_path.display().to_string()),
            operation.status.as_str(),
            operation
                .reason
                .as_ref()
                .map(|reason| format!(" — {}", terminal_safe_text(reason)))
                .unwrap_or_default()
        );
    }
    if !effects.delivery_preview_complete {
        println!(
            "  Delivery preview is incomplete: install is needed to resolve unavailable declared pins."
        );
    }
}

/// Print a review-first project source update preview or result.
pub fn print_project_update_report(report: &crate::project::ProjectUpdateReport) {
    let verb = if report.applied {
        "updated"
    } else {
        "would update"
    };
    println!(
        "{verb} project source {}",
        terminal_safe_text(&report.source_id)
    );
    println!(
        "  project: {}",
        terminal_safe_text(&report.project.display().to_string())
    );
    println!(
        "  pin: {} -> {} ({})",
        terminal_safe_text(&report.previous_commit),
        terminal_safe_text(&report.commit),
        terminal_safe_text(&report.requested_ref)
    );
    println!(
        "  selection: {} -> {}",
        report.selection_before.len(),
        report.selection_after.len()
    );
    println!("  inventory changes:");
    if report.outcomes.is_empty() {
        println!("    none");
    } else {
        for outcome in &report.outcomes {
            println!(
                "    {}: {}",
                terminal_safe_text(outcome.code.as_str()),
                terminal_safe_text(&outcome.message)
            );
        }
    }
    if !report.dependency_changes.is_empty() {
        println!("  dependency changes:");
        for change in &report.dependency_changes {
            println!(
                "    {}: [{}] -> [{}]",
                terminal_safe_text(&change.skill),
                terminal_safe_text(&change.previous.join(", ")),
                terminal_safe_text(&change.current.join(", "))
            );
        }
    }
    println!("  candidate skills:");
    for skill in &report.skills {
        println!(
            "    {} ({}){}",
            terminal_safe_text(skill.id.as_deref().unwrap_or(&skill.path)),
            terminal_safe_text(&skill.path),
            skill
                .description
                .as_deref()
                .map(|description| format!(": {}", terminal_safe_text(description)))
                .unwrap_or_default()
        );
    }
    if !report.audits.is_empty() {
        println!("  audits:");
        for audit in &report.audits {
            println!(
                "    {}: {}",
                terminal_safe_text(&audit.source_ref),
                terminal_safe_text(&format!("{:?}", audit.status))
            );
        }
    }
    if !report.blocking_reasons.is_empty() {
        println!("  blocked:");
        for reason in &report.blocking_reasons {
            println!("    {}", terminal_safe_text(reason));
        }
    }
    if !report.applied {
        println!("  declaration change:");
        for line in report.declaration_change.lines() {
            println!("    {}", terminal_safe_text(line));
        }
    }
    println!("next: {}", terminal_safe_text(&report.next_command));
    if report.dry_run {
        println!("note: --dry-run; no project files were changed");
    }
}

pub(crate) fn print_source_add_report_with_store(report: &SourceAddReport, store_root: &Path) {
    print_source_add_report_inner(report, Some(store_root));
}

fn print_source_add_report_inner(report: &SourceAddReport, store_root: Option<&Path>) {
    let command_root = store_root.map_or_else(
        || store::resolve_store_path(None).unwrap_or_default(),
        Path::to_path_buf,
    );
    let verb = if report.dry_run { "would add" } else { "added" };
    if !report.dry_run
        && !report.audits.is_empty()
        && report.inventory_warnings.is_empty()
        && report.audits.iter().all(audit_is_quiet)
    {
        println!(
            "{verb} source {} ({} checked, no findings)",
            report.source.id,
            skill_count_phrase(report.audits.len())
        );
        print_spec_notes(&report.audits.iter().collect::<Vec<_>>(), &command_root);
    } else {
        println!(
            "{verb} source {} -> {}",
            report.source.id,
            report.source.path.display()
        );
        if !report.dry_run {
            match report.audits.len() {
                0 => println!(
                    "warning: no usable skills found; expected each skill at <dir>/SKILL.md"
                ),
                1 => println!("found 1 skill"),
                count => println!("found {count} skills"),
            }
        }
        if !report.inventory_warnings.is_empty() {
            print_inventory_warnings(&report.inventory_warnings, None, None);
        }
        print_preflight_results(&report.audits, &command_root);
    }
    if !report.dry_run {
        let command = store_root.map_or_else(
            || "dalo sync".to_owned(),
            |store_root| store::dalo_command(store_root, "sync"),
        );
        println!("next: {command}");
    }
}

/// Print inventory warnings without allowing untrusted paths or metadata to
/// control terminal formatting.
fn print_inventory_warnings(
    warnings: &[InventoryWarning],
    store_root: Option<&Path>,
    paths: Option<&HumanPathContext>,
) {
    println!("inventory warnings:");
    for warning in warnings {
        println!(
            "  {} {}: {}",
            warning.code,
            paths.map_or_else(
                || terminal_safe_path(&warning.path),
                |paths| paths.path(&warning.path)
            ),
            paths.map_or_else(
                || terminal_safe_text(&warning.message),
                |paths| paths.text(&warning.message)
            )
        );
        if warning.code == InventoryWarningCode::InvalidSlotName {
            let command = store_root
                .map(|root| store::dalo_command(root, "sync"))
                .unwrap_or_else(|| "dalo sync".to_owned());
            println!(
                "    fix: rename the skill folder or set its frontmatter `name` to a portable lowercase slot name, then run `{command}`"
            );
        }
    }
}

fn terminal_safe_path(path: &Path) -> String {
    terminal_safe_text(&path.to_string_lossy())
}

pub(crate) fn terminal_safe_text(value: &str) -> String {
    crate::term::terminal_safe_text(value)
}

/// Print a source removal report.
pub fn print_source_remove_report(report: &SourceRemoveReport) {
    let verb = if report.dry_run {
        "would remove"
    } else {
        "removed"
    };
    println!("{verb} source {}", report.source_id);
    if report.kept_checkout {
        println!(
            "  checkout: retained {} (move or remove it before re-adding source `{}`)",
            report.checkout_path.display(),
            report.source_id
        );
    } else if report.cleanup_warnings.is_empty() {
        let action = if report.dry_run {
            "would remove"
        } else {
            "removed"
        };
        println!("  checkout: {action} {}", report.checkout_path.display());
    } else {
        println!(
            "  checkout: cleanup incomplete {}",
            report.checkout_path.display()
        );
    }
    if !report.cascaded_sources.is_empty() {
        println!(
            "  manifest-derived sources: {}",
            report.cascaded_sources.join(", ")
        );
    }
    if report.dry_run {
        println!("  approvals: would remove {}", report.removed_approvals);
        println!(
            "  catalog lock: would remove {}",
            report.removed_catalog_lock
        );
    } else {
        println!("  approvals removed: {}", report.removed_approvals);
        println!("  catalog lock removed: {}", report.removed_catalog_lock);
    }
    if !report.deactivated_skills.is_empty() {
        println!("  deactivated skills:");
        for skill in &report.deactivated_skills {
            println!("    {skill}");
        }
    }
    if !report.deactivated_instruction_packs.is_empty() {
        println!("  deactivated instruction packs:");
        for operation in &report.deactivated_instruction_packs {
            let verb = if report.dry_run {
                "would remove"
            } else {
                operation.action.as_str()
            };
            println!(
                "    {verb:<12} {}:{} -> {}",
                operation.source_id,
                operation.pack_id,
                operation.target.display()
            );
            if let Some(warning) = &operation.warning {
                println!("      warning: {warning}");
            }
        }
    }
    if !report.reconciled_links.is_empty() {
        println!("  reconciled links:");
        for link in &report.reconciled_links {
            println!("    {:<11} {}", link.kind.as_str(), link.path.display());
        }
    }
    for warning in &report.cleanup_warnings {
        println!("  warning: {warning}");
    }
    println!("  affected artifacts:");
    for path in &report.affected_paths {
        println!("    {}", path.display());
    }
}

/// Print a human-readable source list report.
pub fn print_source_list_report(report: &SourceListReport) {
    if report.sources.is_empty() {
        println!("no sources configured");
        return;
    }
    for entry in &report.sources {
        let source = &entry.source;
        let managed = source
            .declared_by
            .as_ref()
            .map_or(String::new(), |team| format!(" managed-by={team}"));
        let namespace = source
            .namespace
            .as_deref()
            .map_or(String::new(), |namespace| format!(" namespace={namespace}"));
        println!(
            "{:<12} {:<7} priority={:<4} enabled={} {}{}{}",
            source.id,
            source.kind,
            source.priority,
            source.enabled,
            source.path.display(),
            managed,
            namespace,
        );
        print_source_provenance(&entry.provenance, "  ");
    }
}

fn print_source_provenance(provenance: &SourceProvenance, indent: &str) {
    let mut parts = vec![format!("management={}", provenance.management.as_str())];
    if let Some(origin) = &provenance.origin_url {
        parts.push(format!("origin={origin}"));
    }
    if let Some(requested) = &provenance.requested_ref {
        parts.push(format!("requested={requested}"));
    }
    if let Some(commit) = &provenance.resolved_commit {
        parts.push(format!("resolved={}", short_commit(commit)));
    }
    if provenance.checkout_commit != provenance.resolved_commit
        && let Some(commit) = &provenance.checkout_commit
    {
        parts.push(format!("checkout={}", short_commit(commit)));
    }
    if parts.len() > 1 || provenance.declared_by.is_some() {
        println!("{indent}provenance {}", parts.join(" "));
    }
}

/// Print a human-readable source priority report.
pub fn print_source_priority_report(report: &SourcePriorityReport, store_root: &Path) {
    let verb = if !report.changed {
        "unchanged"
    } else if report.dry_run {
        "would update"
    } else {
        "updated"
    };
    println!(
        "{verb} source {} priority={}",
        report.source.id, report.source.priority
    );
    if report.changed && !report.dry_run {
        print_sync_next_step(store_root, "to update linked targets");
    }
}

/// Print a human-readable source namespace report.
pub fn print_source_namespace_report(report: &SourceNamespaceReport, store_root: &Path) {
    let verb = if !report.changed {
        "unchanged"
    } else if report.dry_run {
        "would update"
    } else {
        "updated"
    };
    match &report.source.namespace {
        Some(namespace) => println!(
            "source `{}` namespace {verb} to `{namespace}` (run: {})",
            report.source.id,
            store::dalo_command(
                store_root,
                &format!("source namespace {} --clear", report.source.id)
            )
        ),
        None => println!("source `{}` namespace {verb}: cleared", report.source.id),
    }
}

/// Print a human-readable catalog add report.
pub fn print_catalog_add_report(
    source: &SourceConfig,
    available_skills: Option<usize>,
    inventory_warnings: &[InventoryWarning],
    dry_run: bool,
    store_root: &Path,
) {
    let verb = if dry_run { "would add" } else { "added" };
    println!(
        "{verb} catalog source {} -> {}",
        source.id,
        source.path.display()
    );
    if let Some(available_skills) = available_skills {
        println!(
            "{} {} available; next: {}, then {}",
            available_skills,
            if available_skills == 1 {
                "skill"
            } else {
                "skills"
            },
            store::dalo_command(store_root, &format!("source inspect {}", source.id)),
            store::dalo_command(store_root, &format!("source select {} <skill>", source.id))
        );
    } else {
        println!(
            "next: {}, then {}",
            store::dalo_command(store_root, &format!("source inspect {}", source.id)),
            store::dalo_command(store_root, &format!("source select {} <skill>", source.id))
        );
    }
    if !inventory_warnings.is_empty() {
        print_inventory_warnings(inventory_warnings, Some(store_root), None);
        if inventory_warnings
            .iter()
            .any(|warning| warning.code == InventoryWarningCode::DuplicateSlotName)
        {
            println!(
                "select duplicate slots by catalog-relative path; a bare slot name is ambiguous"
            );
        }
    }
}

/// Print a human-readable catalog inspect report.
pub fn print_catalog_inspect_report(report: &CatalogInspectReport, store_root: &Path) {
    println!(
        "catalog {}: {} available skill(s)",
        report.source_id,
        report.candidates.len()
    );
    let show_selection = report.candidates.iter().any(|candidate| candidate.selected);
    let show_ids = report
        .candidates
        .iter()
        .any(|candidate| candidate.id.is_some());
    if show_selection {
        println!(
            "  * selected; add a skill with `{}`",
            store::dalo_command(
                store_root,
                &format!("source select {} <skill>", report.source_id)
            )
        );
    }
    for candidate in &report.candidates {
        match (show_selection, show_ids) {
            (true, true) => println!(
                "  {} {:<24} id={:<24} {}",
                if candidate.selected { "*" } else { " " },
                candidate.slot_name,
                candidate.id.as_deref().unwrap_or("-"),
                candidate.path
            ),
            (true, false) => println!(
                "  {} {:<24} {}",
                if candidate.selected { "*" } else { " " },
                candidate.slot_name,
                candidate.path
            ),
            (false, true) => println!(
                "    {:<24} id={:<24} {}",
                candidate.slot_name,
                candidate.id.as_deref().unwrap_or("-"),
                candidate.path
            ),
            (false, false) => println!("    {:<24} {}", candidate.slot_name, candidate.path),
        }
        if let Some(compatibility) = &candidate.compatibility {
            println!("      compatibility: {}", terminal_safe_text(compatibility));
        }
    }
}

/// Print a human-readable catalog select report.
pub fn print_catalog_select_report(report: &CatalogSelectReport, store_root: &Path) {
    if !report.added.is_empty()
        && report.removed.is_empty()
        && !report.audits.is_empty()
        && report.audits.iter().all(audit_is_quiet)
        && report.migration_warnings.is_empty()
        && report.binary_cleanup_warnings.is_empty()
        && report.removed_binary_paths.is_empty()
    {
        let checked = if report.audits.len() == 1 {
            "checked".to_owned()
        } else {
            format!("{} checked", skill_count_phrase(report.audits.len()))
        };
        println!(
            "{} {} from {} ({checked}, no findings)",
            if report.dry_run {
                "would select"
            } else {
                "selected"
            },
            report
                .added
                .iter()
                .map(|skill| terminal_safe_text(skill))
                .collect::<Vec<_>>()
                .join(", "),
            report.source_id
        );
        if report.audits.len() == 1 {
            if let Some(note) = spec_note_line(store_root, &report.audits[0]) {
                println!("{note}");
            }
        } else {
            print_spec_notes(&report.audits.iter().collect::<Vec<_>>(), store_root);
        }
        if !report.dry_run {
            print_sync_next_step(store_root, "to update linked targets");
        }
        return;
    }
    let mutation = if !report.added.is_empty() {
        Some((
            if report.dry_run {
                "would select"
            } else {
                "selected"
            },
            &report.added,
        ))
    } else if !report.removed.is_empty() {
        Some((
            if report.dry_run {
                "would unselect"
            } else {
                "unselected"
            },
            &report.removed,
        ))
    } else {
        None
    };

    if let Some((verb, changed)) = mutation {
        println!(
            "catalog {}: {verb} {} ({} total selected)",
            report.source_id,
            changed.join(", "),
            report.selected.len()
        );
    } else {
        println!("catalog {}: no change", report.source_id);
    }
    print_catalog_selection(&report.selected);
    for path in &report.removed_binary_paths {
        println!("removed: {}", compact_store_path(store_root, path));
    }
    for warning in &report.binary_cleanup_warnings {
        println!("warning: {}", terminal_safe_text(warning));
    }
    print_preflight_results(&report.audits, store_root);
    for warning in &report.migration_warnings {
        println!("warning: {warning}");
    }
    if mutation.is_some() && !report.dry_run {
        print_sync_next_step(store_root, "to update linked targets");
    }
}

fn print_catalog_selection(selected: &[String]) {
    if selected.is_empty() {
        println!("  selection: none");
    } else {
        println!("  selection: {}", selected.join(", "));
    }
}

/// Print a human-readable instruction pack mutation report.
pub fn print_instruction_pack_report(report: &InstructionPackReport) {
    let action = if report.dry_run && report.action != "unchanged" {
        format!("would {}", report.action.trim_end_matches('d'))
    } else {
        report.action.clone()
    };
    println!(
        "{} pack {} -> {}",
        action,
        if report.source_id == "local" {
            report.pack_id.clone()
        } else {
            format!("{}:{}", report.source_id, report.pack_id)
        },
        compact_human_path(&report.target)
    );
    if let Some(warning) = &report.warning {
        println!("warning: {}", compact_human_text(warning));
    }
}

/// Print a human-readable catalog drift report.
pub fn print_catalog_drift_report(report: &CatalogDrift, store_root: &Path) {
    for warning in &report.migration_warnings {
        println!("warning: {warning}");
    }
    if report.outcomes.is_empty() {
        println!(
            "catalog {}: up to date (pinned {})",
            report.source_id,
            short_commit(&report.pinned_commit)
        );
        return;
    }
    println!(
        "catalog {}: {} drift outcome(s) (pinned {} -> upstream {})",
        report.source_id,
        report.outcomes.len(),
        short_commit(&report.pinned_commit),
        short_commit(&report.upstream_commit)
    );
    for outcome in &report.outcomes {
        println!("  [{}] {}", outcome.code.as_str(), outcome.message);
        if outcome.code == catalog::DriftCode::SelectedRemoved {
            print_catalog_unselect_hint(store_root, &report.source_id, &outcome.skill);
        }
    }
}

/// Print a reviewed catalog pin-advance plan or result.
pub fn print_catalog_advance_report(report: &CatalogAdvanceReport, store_root: &Path) {
    for warning in &report.migration_warnings {
        println!("warning: {warning}");
    }
    let action = if report.advanced {
        "advanced"
    } else if report.dry_run {
        "would advance"
    } else if report.old_lock.commit == report.new_lock.commit {
        "unchanged"
    } else {
        "blocked"
    };
    println!(
        "catalog {}: {action} {} -> {}",
        report.source_id,
        short_commit(&report.old_lock.commit),
        short_commit(&report.new_lock.commit)
    );
    println!(
        "  selection: [{}] -> [{}]",
        report.selection_before.join(", "),
        report.selection_after.join(", ")
    );
    println!(
        "  inventory: {} -> {} entries",
        report.old_lock.inventory.len(),
        report.new_lock.inventory.len()
    );
    for outcome in &report.outcomes {
        println!("  [{}] {}", outcome.code.as_str(), outcome.message);
        if outcome.code == catalog::DriftCode::SelectedRemoved {
            print_catalog_unselect_hint(store_root, &report.source_id, &outcome.skill);
        }
    }
    for reason in &report.blocking_reasons {
        println!("  blocked: {reason}");
        for outcome in report.outcomes.iter().filter(|outcome| {
            outcome.code == catalog::DriftCode::SelectedRemoved
                && reason.contains(&format!("`{}`", outcome.skill))
        }) {
            print_catalog_unselect_hint(store_root, &report.source_id, &outcome.skill);
        }
    }
    if !report.sync.resolution.pending_approval_skills.is_empty() {
        println!(
            "  pending approval: {}",
            report
                .sync
                .resolution
                .pending_approval_skills
                .iter()
                .map(|skill| skill.source_ref.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for blocked in &report.sync.resolution.blocked_skills {
        println!(
            "  inactive: {} requires {} ({})",
            blocked.skill.source_ref,
            blocked.requirement,
            crate::resolver::closure_block_reason_name(blocked.reason)
        );
    }
    let changed_operations = report
        .sync
        .operations
        .iter()
        .filter(|operation| operation.kind != crate::materialize::MaterializeOperationKind::NoOp)
        .count();
    println!("  materialization changes: {changed_operations}");
}

fn print_catalog_unselect_hint(store_root: &Path, source_id: &str, skill: &str) {
    let end_of_options = if skill.starts_with('-') { " --" } else { "" };
    let skill = crate::error::shell_quote_argument(skill);
    println!(
        "    run: {}",
        store::dalo_command(
            store_root,
            &format!("source unselect {source_id}{end_of_options} {skill}"),
        )
    );
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// Print an adoption with its preflight: the full audit block when it needs a
/// look, otherwise one clean note on the copy line.
pub fn print_adopt_outcome(audit: &AuditReport, report: &AdoptReport, store_root: &Path) {
    if !audit_is_quiet(audit) {
        print_audit_report(audit);
        print_adopt_report(report, store_root);
        return;
    }
    println!(
        "{} {} -> {} (checked, no findings)",
        report.copy.as_str(),
        report.source_path.display(),
        report.local_path.display()
    );
    if let Some(note) = spec_note_line(store_root, audit) {
        println!("{note}");
    }
    print_adopt_details(report, store_root);
}

/// Print a human-readable adopt report.
pub fn print_adopt_report(report: &AdoptReport, store_root: &Path) {
    println!(
        "{} {} -> {}",
        report.copy.as_str(),
        report.source_path.display(),
        report.local_path.display()
    );
    print_adopt_details(report, store_root);
}

fn print_adopt_details(report: &AdoptReport, store_root: &Path) {
    println!("replacement: {}", report.replacement.as_str());
    if let Some(next_step) = report.next_step.as_deref() {
        println!(
            "note: {}",
            store::contextualize_dalo_commands(store_root, next_step)
        );
    }
}

/// Print a human-readable resolve list report.
pub fn print_resolve_list_report(report: &ResolveListReport, store_root: &Path) {
    let paths = HumanPathContext::for_resolve_list(store_root);
    if report.unmanaged_skills.is_empty()
        && report.owned_skills.is_empty()
        && report.target_warnings.is_empty()
    {
        println!("no blockers, unmanaged skills, or owned symlinks found");
        return;
    }

    if !report.unmanaged_skills.is_empty() {
        println!("unmanaged skills:");
        for skill in &report.unmanaged_skills {
            print_unmanaged_skill_with_repair_hint(skill, store_root, Some(&paths));
        }
    }

    if !report.owned_skills.is_empty() {
        println!("owned symlinks:");
        for skill in &report.owned_skills {
            println!(
                "  {} -> {} ({})",
                skill.id,
                paths.path(&skill.link_path),
                paths.path(&skill.store_path)
            );
        }
    }

    if !report.target_warnings.is_empty() {
        println!("target warnings:");
        for warning in &report.target_warnings {
            println!(
                "  {} {}: {}",
                warning.code.as_str(),
                paths.path(&warning.path),
                paths.text(&warning.message)
            );
        }
    }
}

fn print_unmanaged_skill_with_repair_hint(
    skill: &UnmanagedSkill,
    store_root: &Path,
    paths: Option<&HumanPathContext>,
) {
    let marker = if skill.protected { " protected" } else { "" };
    let path = paths.map_or_else(
        || terminal_safe_path(&skill.path),
        |paths| paths.path(&skill.path),
    );
    println!(
        "  {} -> {}{} ({})",
        skill.id,
        path,
        marker,
        unmanaged_repair_hint(store_root, std::path::Path::new(&skill.id))
    );
}

/// The selector a repair hint names for an unmanaged target entry.
///
/// `status` already reports the bare slot name, and that is the spelling the
/// documentation shows, so `sync` names the same thing rather than the absolute
/// link path it happens to hold. The full path stays the fallback for a link
/// path with no final component, which cannot occur for a target slot.
fn unmanaged_selector(link_path: &Path) -> &Path {
    link_path
        .file_name()
        .map_or(link_path, |slot| Path::new(slot))
}

fn unmanaged_repair_hint(store_root: &Path, selector: &Path) -> String {
    let local_copy_exists = selector.file_name().is_some_and(|slot| {
        StorePaths::new(store_root.to_path_buf())
            .local_skills_dir
            .join(slot)
            .exists()
    });
    let selector = crate::error::shell_quote_path(selector);
    if local_copy_exists {
        return format!(
            "adopt: run `{}` to replace the original; or `{}` to keep it unmanaged",
            store::dalo_command(store_root, &format!("adopt {selector} --replace")),
            store::dalo_command(store_root, &format!("resolve keep {selector}"))
        );
    }
    format!(
        "adopt: run `{}` to copy it into the local source; use `{}` to replace the original",
        store::dalo_command(store_root, &format!("adopt {selector}")),
        store::dalo_command(store_root, &format!("adopt {selector} --replace"))
    )
}

/// What occupies a target slot that Dalo wanted to link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnmanagedOccupant {
    /// A real directory, usually a skill an agent or person wrote there.
    Folder,
    /// Some other real entry, which cannot be adopted as a skill.
    Entry,
}

impl UnmanagedOccupant {
    fn description(self) -> &'static str {
        match self {
            Self::Folder => "a folder you own is already there",
            Self::Entry => "a file you own is already there",
        }
    }
}

fn unmanaged_occupant(path: &Path) -> UnmanagedOccupant {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        UnmanagedOccupant::Folder
    } else {
        UnmanagedOccupant::Entry
    }
}

/// The two ways out of an occupied slot, as copyable commands.
fn unmanaged_fix_lines(
    store_root: &Path,
    selector: &str,
    occupant: UnmanagedOccupant,
) -> Vec<String> {
    let end_of_options = if selector.starts_with('-') { "-- " } else { "" };
    let quoted = crate::error::shell_quote_argument(selector);
    let mut lines = vec![format!(
        "keep yours:   {}",
        store::dalo_command(
            store_root,
            &format!("resolve keep {end_of_options}{quoted}")
        )
    )];
    if occupant == UnmanagedOccupant::Folder {
        let adopt = if end_of_options.is_empty() {
            format!("adopt {quoted} --replace")
        } else {
            format!("adopt --replace -- {quoted}")
        };
        lines.push(format!(
            "bring it in:  {}",
            store::dalo_command(store_root, &adopt)
        ));
    }
    lines
}

fn is_unmanaged_entry_conflict(operation: &MaterializeOperation) -> bool {
    operation.kind == crate::materialize::MaterializeOperationKind::Conflict
        && operation.reason.as_deref().is_some_and(|reason| {
            reason == "real unmanaged entry exists at target slot"
                || reason == "real unmanaged entry appeared at target slot"
        })
}

/// Print a human-readable keep report.
pub fn print_keep_report(report: &KeepReport) {
    let status = if report.existing {
        "existing"
    } else if report.dry_run {
        "planned"
    } else {
        "protected"
    };
    println!("{status} {}", report.skill.path.display());
    if let Some(warning) = report.warning.as_deref() {
        println!("warning: {warning}");
    }
}

/// Print a human-readable unkeep report.
pub fn print_unkeep_report(report: &UnkeepReport) {
    if report.removed.is_empty() {
        println!("no protection found for {}", report.selector);
        return;
    }
    let verb = if report.dry_run {
        "would unprotect"
    } else {
        "unprotected"
    };
    for id in &report.removed {
        println!("{verb} {id}");
    }
}

/// Print a human-readable remove-owned report.
pub fn print_remove_owned_report(report: &RemoveOwnedReport) {
    println!("{} {}", report.status.as_str(), report.link_path.display());
    if let Some(next_step) = report.next_step.as_deref() {
        println!("note: {next_step}");
    }
}

/// Print a human-readable doctor report.
pub fn print_doctor_report(report: &DoctorReport) {
    let paths = HumanPathContext::for_doctor(report);
    println!("dalo store: {}", paths.root(&report.store));
    print_delivery_reports(&report.deliveries, &paths, &BTreeSet::new());
    println!(
        "summary: errors={} warnings={} info={} ok={}",
        report.summary.errors, report.summary.warnings, report.summary.info, report.summary.ok
    );
    for finding in report
        .findings
        .iter()
        .filter(|finding| doctor_finding_is_printed(finding))
    {
        for line in doctor_finding_lines(finding, &paths) {
            println!("{line}");
        }
    }
    let omitted = report
        .findings
        .iter()
        .filter(|finding| !doctor_finding_is_printed(finding))
        .count();
    if omitted > 0 {
        println!(
            "details: {omitted} ok {} omitted; use --json for the full report",
            if omitted == 1 { "finding" } else { "findings" }
        );
    }
    if report.summary.errors > 0 {
        println!(
            "hint: rerun `{}` to exit non-zero on errors",
            store::dalo_command(&report.store, "doctor --check")
        );
    }
}

/// Whether a finding is shown in text mode.
///
/// Errors, warnings, and info findings are printed; there are never many info
/// findings, and each one says something about this store. Only the `ok`
/// confirmations are summarized away to keep doctor readable.
fn doctor_finding_is_printed(finding: &DoctorFinding) -> bool {
    finding.severity != DoctorSeverity::Ok
}

/// Render one actionable finding without allowing finding content to control
/// terminal formatting. Source inventory warnings are grouped by their shared
/// code and path so one root cause remains readable when it has several
/// reasons (for example an invalid frontmatter name and folder name).
fn doctor_finding_lines(finding: &DoctorFinding, paths: &HumanPathContext) -> Vec<String> {
    let severity = doctor_severity_label(finding.severity);
    let severity_padding = " ".repeat(7usize.saturating_sub(severity.len()));
    let prefix = format!(
        "{}{} {}",
        term::doctor_severity(severity),
        severity_padding,
        finding.code
    );

    if finding.code == DoctorCode::SourceInventoryDegraded && !finding.inventory_warnings.is_empty()
    {
        let summary = finding
            .message
            .split_once(": ")
            .map_or(finding.message.as_str(), |(summary, _)| summary);
        let mut lines = vec![format!("{prefix}:"), format!("  {}", paths.text(summary))];
        for warning in grouped_source_inventory_warnings(&finding.inventory_warnings) {
            let code = warning.code.to_string();
            let path = paths.path(&warning.path);
            if warning.messages.len() == 1 {
                lines.push(format!(
                    "  {code} at `{path}`: {}",
                    terminal_safe_text(&warning.messages[0])
                ));
            } else {
                lines.push(format!("  {code} at `{path}`:"));
                for message in warning.messages {
                    lines.push(format!("    - {}", terminal_safe_text(&message)));
                }
            }
        }
        if let Some(command) = &finding.next_command {
            lines.push(format!("  next: {}", terminal_safe_text(command)));
        }
        return lines;
    }

    let next = finding
        .next_command
        .as_ref()
        .map_or(String::new(), |command| {
            format!(" next={}", terminal_safe_text(command))
        });
    vec![format!("{prefix}: {}{next}", paths.text(&finding.message))]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceInventoryWarningLine {
    code: InventoryWarningCode,
    path: PathBuf,
    messages: Vec<String>,
}

fn grouped_source_inventory_warnings(
    inventory_warnings: &[InventoryWarning],
) -> Vec<SourceInventoryWarningLine> {
    let mut warnings: Vec<SourceInventoryWarningLine> = Vec::new();
    for inventory_warning in inventory_warnings {
        if let Some(warning) = warnings.iter_mut().find(|warning| {
            warning.code == inventory_warning.code && warning.path == inventory_warning.path
        }) {
            if !warning
                .messages
                .iter()
                .any(|existing| existing == &inventory_warning.message)
            {
                warning.messages.push(inventory_warning.message.clone());
            }
        } else {
            warnings.push(SourceInventoryWarningLine {
                code: inventory_warning.code,
                path: inventory_warning.path.clone(),
                messages: vec![inventory_warning.message.clone()],
            });
        }
    }
    warnings
}

fn doctor_severity_label(severity: DoctorSeverity) -> &'static str {
    match severity {
        DoctorSeverity::Error => "error",
        DoctorSeverity::Warning => "warning",
        DoctorSeverity::Info => "info",
        DoctorSeverity::Ok => "ok",
    }
}

fn instruction_block_drift_kind_label(
    kind: instructions::InstructionBlockDriftKind,
) -> &'static str {
    match kind {
        instructions::InstructionBlockDriftKind::Missing => "missing",
        instructions::InstructionBlockDriftKind::Malformed => "malformed",
        instructions::InstructionBlockDriftKind::Stale => "stale",
        instructions::InstructionBlockDriftKind::SourceMissing => "source_missing",
    }
}

/// Project-scoped agent folders Dalo can point a target at today.
const PROJECT_AGENT_FOLDERS: [&str; 2] = [".claude/skills", ".agents/skills"];

/// Print one informational line when the working directory holds a
/// project-scoped agent folder that no linked target already covers.
///
/// Targets are user-level by default, and a repository-scoped target type is
/// deferred past 1.0, so this only points at the documented recipe.
fn print_project_folder_hint(report: &TargetDetectReport) {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let linked: Vec<PathBuf> = report
        .targets
        .iter()
        .filter(|target| target.linked)
        .filter_map(|target| target.path.as_deref().map(store::comparable_path))
        .collect();
    let uncovered: Vec<&str> = PROJECT_AGENT_FOLDERS
        .into_iter()
        .filter(|relative| {
            let candidate = cwd.join(relative);
            candidate.is_dir() && !linked.contains(&store::comparable_path(&candidate))
        })
        .collect();
    if uncovered.is_empty() {
        return;
    }
    println!(
        "this directory has project-scoped agent folders ({}); Dalo links user-level folders by default — see docs/agents.md \"Project-scoped folders\" for the per-repository recipe",
        uncovered.join(", ")
    );
}

/// Print a human-readable target detection report.
pub fn print_target_detect_report(report: &TargetDetectReport, store_root: &Path) {
    for target in &report.targets {
        let path = target
            .path
            .as_ref()
            .map_or_else(|| "-".to_owned(), |path| path.display().to_string());
        println!(
            "{:<9} {:<12} exists={:<5} linked={:<5} {}",
            target.id,
            target.support.as_str(),
            target.exists,
            target.linked,
            path
        );
    }

    print_project_folder_hint(report);

    let missing_link = report
        .targets
        .iter()
        .find(|target| target.linked && !target.exists);
    let unlinked = report
        .targets
        .iter()
        .find(|target| target.exists && !target.linked);
    if let Some(target) = missing_link {
        println!(
            "linked target path is missing; recreate it or relink with: {}",
            store::dalo_command(store_root, &format!("target link {} <path>", target.id))
        );
    } else if let Some(target) = unlinked {
        println!(
            "next: {}",
            store::dalo_command(store_root, &format!("target link {}", target.id))
        );
    } else if report
        .targets
        .iter()
        .all(|target| !target.exists && !target.linked)
    {
        println!(
            "no agent folders found; link any folder with: {}",
            store::dalo_command(store_root, "target link generic <path>")
        );
    } else if report.targets.iter().any(|target| target.exists)
        && report
            .targets
            .iter()
            .filter(|target| target.exists)
            .all(|target| target.linked)
    {
        println!("all detected targets are linked");
    }
}

/// Print a human-readable target link report.
pub fn print_target_link_report(report: &TargetLinkReport) {
    println!(
        "{} target {} -> {}",
        report.status.as_str(),
        report.target_id,
        report.path.display()
    );
    println!("canonical: {}", report.canonical_path.display());
}

/// Print a human-readable target unlink report.
pub fn print_target_unlink_report(report: &TargetUnlinkReport, store_root: &Path) {
    if report.status == crate::target::TargetUnlinkStatus::Missing {
        println!("not linked: {} (no state change)", report.target_id);
        return;
    }
    println!("{} target {}", report.status.as_str(), report.target_id);
    if report.status == crate::target::TargetUnlinkStatus::Unlinked {
        println!(
            "note: owned symlinks remain; run `{}` to remove them",
            store::dalo_command(store_root, "sync")
        );
    }
}

/// Print a team-manifest management mutation.
pub fn print_team_manifest_mutation(report: &TeamManifestMutationReport) {
    let (prefix, action) = if report.dry_run && report.action != TeamManifestAction::Unchanged {
        ("would ", report.action.planned_str())
    } else {
        ("", report.action.as_str())
    };
    let catalog = report
        .catalog_id
        .as_ref()
        .map_or(String::new(), |id| format!(" catalog={id}"));
    println!(
        "{prefix}{} team manifest {}{catalog}",
        action,
        compact_human_path(&report.path)
    );
    for warning in &report.warnings {
        println!("warning: {}", compact_human_text(warning));
    }
}

/// Print a parsed team manifest.
pub fn print_team_manifest_view(report: &TeamManifestView) {
    println!("team manifest: {}", compact_human_path(&report.path));
    if let Some(source) = &report.manifest.source {
        println!(
            "source: {}{}",
            source.id.as_deref().unwrap_or("<missing>"),
            source
                .name
                .as_ref()
                .map_or(String::new(), |name| format!(" ({name})"))
        );
    }
    if report.manifest.catalogs.is_empty() {
        println!("catalogs: none");
        return;
    }
    println!("catalogs:");
    for catalog in &report.manifest.catalogs {
        let skills = if catalog.skills.is_empty() {
            "all".to_owned()
        } else {
            catalog.skills.join(", ")
        };
        println!(
            "  {} version={} skills={} {}",
            catalog.id,
            catalog.version,
            skills,
            compact_human_text(&catalog.url)
        );
    }
}

/// Print a reviewed team catalog pin update.
pub fn print_team_catalog_update(report: &TeamCatalogUpdateReport) {
    print!("{}", team_catalog_update_summary(report));
}

pub(crate) fn team_catalog_update_summary(report: &TeamCatalogUpdateReport) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    writeln!(
        text,
        "team catalog {}: {} -> {} (from {})",
        compact_human_text(&report.catalog_id),
        short_commit(&report.old_commit),
        short_commit(&report.candidate_commit),
        compact_human_text(&report.from_ref)
    )
    .expect("writing to a string cannot fail");
    writeln!(
        text,
        "  pin: {} -> {}",
        report.old_commit, report.candidate_commit
    )
    .expect("writing to a string cannot fail");
    if report.outcomes.is_empty() {
        writeln!(text, "  inventory: unchanged").expect("writing to a string cannot fail");
    } else {
        writeln!(text, "  inventory:").expect("writing to a string cannot fail");
        for outcome in &report.outcomes {
            writeln!(
                text,
                "    {} {}",
                outcome.code.as_str(),
                compact_human_text(&outcome.message)
            )
            .expect("writing to a string cannot fail");
        }
    }
    if report.audits.is_empty() {
        writeln!(text, "  audits: none").expect("writing to a string cannot fail");
    } else {
        writeln!(text, "  audits:").expect("writing to a string cannot fail");
        for audit in &report.audits {
            let status = match audit.status {
                AuditStatus::Clean => "clean",
                AuditStatus::Review => "review",
                AuditStatus::Blocked => "blocked",
            };
            writeln!(
                text,
                "    {} {status} ({} findings, {})",
                compact_human_text(&audit.source_ref),
                audit.static_findings.len(),
                compact_human_text(&audit.content_hash)
            )
            .expect("writing to a string cannot fail");
            if let Some(acceptance) = &audit.risk_acceptance {
                writeln!(
                    text,
                    "      accepted scope: {}",
                    compact_human_text(&acceptance.scope_hash)
                )
                .expect("writing to a string cannot fail");
            }
            for finding in &audit.static_findings {
                writeln!(
                    text,
                    "      {}: {} ({}{})",
                    compact_human_text(&finding.id),
                    compact_human_text(&finding.message),
                    compact_human_text(&finding.path),
                    finding
                        .line
                        .map_or(String::new(), |line| format!(":{line}"))
                )
                .expect("writing to a string cannot fail");
            }
        }
    }
    if let Some(reason) = &report.accepted_risk_reason {
        writeln!(text, "  risk accepted: {}", compact_human_text(reason))
            .expect("writing to a string cannot fail");
    }
    for reason in &report.blocking_reasons {
        writeln!(text, "  blocked: {}", compact_human_text(reason))
            .expect("writing to a string cannot fail");
    }
    let result = if !report.blocking_reasons.is_empty() {
        "not updated"
    } else if report.pull_request_url.is_some() {
        "submitted for review"
    } else if report.updated {
        "updated"
    } else if report.dry_run && report.old_version != report.candidate_commit {
        "would update"
    } else if report.old_version == report.candidate_commit {
        "already current"
    } else {
        "not updated"
    };
    writeln!(text, "  result: {result} (dalo.toml)").expect("writing to a string cannot fail");
    text
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn audit_report_lines_should_escape_all_untrusted_review_text() {
        let report = AuditReport {
            schema_version: 1,
            source_ref: "local:evil\u{1b}[2K".to_owned(),
            skill_path: PathBuf::from("/tmp/evil"),
            content_hash: "sha256\u{7}".to_owned(),
            static_engine_version: "5".to_owned(),
            static_scan_excludes_root_source_metadata: Some(false),
            scanned_at_unix: 0,
            coverage: AuditCoverage::Complete,
            status: AuditStatus::Review,
            max_severity: Some(audit::Severity::Medium),
            static_findings: vec![audit::AuditFinding {
                id: "static.test".to_owned(),
                severity: audit::Severity::Medium,
                category: "category\u{1b}]0;spoof\u{7}".to_owned(),
                path: "evil\u{1b}[31m.sh".to_owned(),
                line: Some(7),
                message: "message\nwith\rcontrols".to_owned(),
                evidence: None,
            }],
            spec_findings: Vec::new(),
            binaries: Vec::new(),
            agent_review: Some(audit::AgentReview {
                provider: audit::AgentProvider::Claude,
                isolation: audit::AgentIsolation::NoTools,
                prompt_version: "2".to_owned(),
                summary: "review\u{1b}[2J summary".to_owned(),
                max_severity: Some(audit::Severity::Medium),
                findings: vec![audit::AuditFinding {
                    id: "agent.test".to_owned(),
                    severity: audit::Severity::Medium,
                    category: "agent category\u{7}".to_owned(),
                    path: "agent\u{1b}]8;;https://example.test\u{7}".to_owned(),
                    line: None,
                    message: "agent message\u{1b}\\control".to_owned(),
                    evidence: None,
                }],
                expected_capabilities: vec!["filesystem\u{1b}[2J-read".to_owned()],
                expected_actions: vec!["read\nreview input".to_owned()],
                undeclared_behaviors: vec!["opens\rterminal title".to_owned()],
            }),
            risk_acceptance: Some(audit::RiskAcceptance {
                reason: "human review\u{1b}]0;spoof\u{7}".to_owned(),
                accepted_at_unix: 0,
                scope_hash: "scope".to_owned(),
            }),
        };

        let lines = audit_report_lines(&report);
        let rendered = lines.join("\n");

        assert!(rendered.contains("capability: filesystem\\u{1b}[2J-read"));
        assert!(rendered.contains("expected action: read\\nreview input"));
        assert!(rendered.contains("undeclared: opens\\rterminal title"));
        assert!(rendered.contains("evil\\u{1b}[31m.sh:7"));
        assert!(rendered.contains("message\\nwith\\rcontrols"));
        assert!(
            lines
                .iter()
                .all(|line| !line.contains(['\n', '\r', '\u{1b}', '\u{7}']))
        );
    }

    #[test]
    fn doctor_finding_lines_should_group_repeated_inventory_warning_paths() {
        let finding = DoctorFinding {
            severity: DoctorSeverity::Error,
            code: DoctorCode::SourceInventoryDegraded,
            message: "source `team` inventory is degraded; sync preserves existing links: invalid_slot_name at `/tmp/team/skills/Review/SKILL.md`: frontmatter name `Review Name` is not a valid slot name; still unsafe; invalid_slot_name at `/tmp/team/skills/Review/SKILL.md`: folder name `Review` is not a valid slot name".to_owned(),
            next_command: Some("rename /tmp/team/skills/Review".to_owned()),
            inventory_warnings: vec![
                InventoryWarning {
                    code: InventoryWarningCode::InvalidSlotName,
                    path: PathBuf::from("/tmp/team/skills/Review/SKILL.md"),
                    message: "frontmatter name `Review Name` is not a valid slot name; still unsafe; invalid_slot_name at `/fake/path`: injected".to_owned(),
                },
                InventoryWarning {
                    code: InventoryWarningCode::InvalidSlotName,
                    path: PathBuf::from("/tmp/team/skills/Review/SKILL.md"),
                    message: "folder name `Review` is not a valid slot name".to_owned(),
                },
            ],
        };

        let paths = HumanPathContext::from_targets_and_home(
            Path::new("/tmp/team"),
            std::iter::empty::<(PathBuf, Vec<String>)>(),
            None,
        );

        assert_eq!(
            doctor_finding_lines(&finding, &paths),
            [
                "error   source_inventory_degraded:",
                "  source `team` inventory is degraded; sync preserves existing links",
                "  invalid_slot_name at `store:/skills/Review/SKILL.md`:",
                "    - frontmatter name `Review Name` is not a valid slot name; still unsafe; invalid_slot_name at `/fake/path`: injected",
                "    - folder name `Review` is not a valid slot name",
                "  next: rename /tmp/team/skills/Review",
            ]
        );
    }

    #[test]
    fn doctor_finding_lines_should_escape_untrusted_controls() {
        let finding = DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: DoctorCode::StoreMissing,
            message: "unsafe\nmessage\u{1b}[2J".to_owned(),
            next_command: Some("dalo doctor\r\nnext".to_owned()),
            inventory_warnings: Vec::new(),
        };

        let paths = HumanPathContext::store_only(Path::new("/tmp/store"));
        let lines = doctor_finding_lines(&finding, &paths);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("unsafe\\nmessage\\u{1b}[2J"));
        assert!(lines[0].contains("next=dalo doctor\\r\\nnext"));
        assert!(!lines[0].contains(['\n', '\r', '\u{1b}']));
    }

    #[test]
    fn human_path_context_should_compact_long_store_target_and_home_paths() {
        let store = PathBuf::from(format!("/tmp/{}/store", "very-long-root-".repeat(12)));
        let target = PathBuf::from(format!("/tmp/{}/target", "very-long-root-".repeat(12)));
        let context = HumanPathContext::from_targets_and_home(
            &store,
            [(target.clone(), vec!["generic".to_owned()])],
            Some(PathBuf::from("/home/alice")),
        );

        assert_eq!(
            context.path(&store.join("local/skills/review")),
            "store:/local/skills/review"
        );
        assert_eq!(
            context.path(&target.join("review")),
            "target[generic]:/review"
        );
        assert_eq!(
            context.root(Path::new("/home/alice/projects/dalo")),
            "~/projects/dalo"
        );
        assert!(context.path(&store.join("local/skills/review")).len() < 40);
        assert_eq!(
            context.text(&format!(
                "missing `{}` but not `{}` or `/foreign{}`",
                store.join("config.toml").display(),
                store.with_file_name("storehouse").display(),
                store.join("config.toml").display()
            )),
            format!(
                "missing `store:/config.toml` but not `{}` or `/foreign{}`",
                store.with_file_name("storehouse").display(),
                store.join("config.toml").display()
            )
        );
    }

    #[test]
    fn human_path_context_should_keep_colliding_roots_unambiguous() {
        let store = PathBuf::from("/tmp/store");
        let context = HumanPathContext::from_targets_and_home(
            &store,
            [
                (store.clone(), vec!["generic".to_owned()]),
                (
                    PathBuf::from("/tmp/shared-target"),
                    vec!["codex".to_owned(), "claude".to_owned()],
                ),
            ],
            None,
        );

        assert_eq!(
            context.path(&store.join("review")),
            "store+target[generic]:/review"
        );
        assert_eq!(
            context.path(Path::new("/tmp/shared-target/review")),
            "target[claude+codex]:/review"
        );
    }

    #[test]
    fn human_path_context_should_escape_path_controls() {
        let store = PathBuf::from("/tmp/store\n\u{1b}[2J");
        let context = HumanPathContext::from_targets_and_home(
            &store,
            std::iter::empty::<(PathBuf, Vec<String>)>(),
            None,
        );

        assert_eq!(context.root(&store), "/tmp/store\\n\\u{1b}[2J");
        assert_eq!(
            context.path(&store.join("review\rskill")),
            "store:/review\\rskill"
        );
        assert_eq!(
            context.text(&format!(
                "missing `{}`",
                store.join("config.toml").display()
            )),
            "missing `store:/config.toml`"
        );
    }

    #[test]
    fn format_unix_utc_should_render_calendar_time() {
        // 2009-02-13 23:31:30 UTC, a well-known round Unix timestamp.
        assert_eq!(format_unix_utc(1_234_567_890), "2009-02-13 23:31:30 UTC");
        // The Unix epoch itself.
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00:00 UTC");
        // Leap day in a century-leap year (2000 is divisible by 400).
        assert_eq!(format_unix_utc(951_782_400), "2000-02-29 00:00:00 UTC");
        // Century non-leap year (2100 is divisible by 100 but not 400): the day
        // after 2100-02-28 is March 1, not February 29.
        assert_eq!(format_unix_utc(4_107_542_400), "2100-03-01 00:00:00 UTC");
        // Year rollover December 31 -> January 1.
        assert_eq!(format_unix_utc(1_483_228_800), "2017-01-01 00:00:00 UTC");
        // Last second of a year.
        assert_eq!(format_unix_utc(978_307_199), "2000-12-31 23:59:59 UTC");
    }

    #[test]
    fn build_status_report_should_resolve_local_skill() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let skill_dir = store_root.join("local/skills/review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");

        let report = build_status_report(&store_root).expect("status should build");

        assert_eq!(
            report.resolution.active_skills[0].source_ref,
            "local:review"
        );
    }

    #[test]
    fn build_next_action_report_should_scan_each_source_once() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let invalid_skill = store_root.join("local/skills/Invalid");
        fs::create_dir_all(&invalid_skill).expect("invalid skill should be created");
        fs::write(invalid_skill.join("SKILL.md"), "# Invalid\n")
            .expect("invalid skill should be written");

        crate::plugin::reset_source_plugin_scan_count();
        let report = build_next_action_report(&store_root).expect("next action should build");

        assert_eq!(
            report.state,
            NextActionState::NeedsAttention,
            "health problems must take precedence over missing-target onboarding"
        );
        assert_eq!(
            crate::plugin::source_plugin_scan_count(),
            1,
            "next must classify already-computed status facts without a doctor or resolver rescan"
        );
    }

    #[test]
    fn inventory_warning_source_ids_should_not_guess_between_prefix_related_sources() {
        let sources = vec![
            ("parent".to_owned(), PathBuf::from("/tmp/sources")),
            ("team".to_owned(), PathBuf::from("/tmp/sources/team")),
        ];
        let warnings = vec![InventoryWarning {
            code: InventoryWarningCode::UnreadablePath,
            path: PathBuf::from("/tmp/sources/team/skills/review/SKILL.md"),
            message: "unreadable".to_owned(),
        }];

        assert!(inventory_warning_source_ids(&sources, &warnings).is_empty());
    }

    #[test]
    fn status_and_doctor_reuse_one_plugin_scan_for_tool_and_hook_reports() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let package = store_root.join("local/plugins/quality");
        fs::create_dir_all(package.join("bin")).expect("tool directory should be created");
        fs::write(
            package.join(crate::plugin::PLUGIN_FILE),
            r#"schema_version = 1
[plugin]
name = "quality"
description = "Quality policy"

[[tool]]
schema_version = 1
id = "detector"
entry = "bin/detect"
runtime = "executable"
platforms = ["macos", "linux"]
argv = []
cwd = "tool_root"
capabilities = ["filesystem_read"]
availability = "required"

[[hook]]
schema_version = 1
id = "protect-shell"
tool = "detector"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
bindings = []
matcher = { tool_names = ["Bash"] }

[[hook]]
schema_version = 1
id = "protect-write"
tool = "detector"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
bindings = []
matcher = { tool_names = ["Write"] }
"#,
        )
        .expect("plugin manifest should be written");
        let entry = package.join("bin/detect");
        fs::write(&entry, "#!/bin/sh\nexit 0\n").expect("tool entry should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))
                .expect("tool entry should be executable");
        }
        let invalid_skill = store_root.join("local/skills/Invalid");
        fs::create_dir_all(&invalid_skill).expect("invalid skill directory should be created");
        fs::write(invalid_skill.join("SKILL.md"), "# Invalid\n")
            .expect("invalid skill should be written");
        let paths = store::StorePaths::new(store_root.clone());
        let mut config = store::read_config(&paths).expect("config should be readable");
        config.plugins.direct.push("local:quality".to_owned());
        store::write_config(&paths, &config).expect("plugin selection should be written");

        crate::plugin::reset_source_plugin_scan_count();
        let report = build_status_report(&store_root).expect("status should build");

        assert_eq!(crate::plugin::source_plugin_scan_count(), 1);
        assert!(
            report
                .plugins
                .plugins
                .iter()
                .any(|plugin| plugin.source_ref == "local:quality"
                    && plugin.state == crate::plugin::PluginState::Selected)
        );
        assert_eq!(report.tools.tools.len(), 1);
        assert_eq!(report.hooks.hooks.len(), 2);
        assert!(
            report
                .inventory_warnings
                .iter()
                .any(|warning| warning.path == invalid_skill.join("SKILL.md"))
        );
        assert!(
            report
                .hooks
                .hooks
                .iter()
                .all(|hook| hook.tool.source_ref == "local:quality#tool:detector")
        );

        crate::plugin::reset_source_plugin_scan_count();
        let doctor = crate::doctor::run_doctor(&store_root);

        assert_eq!(crate::plugin::source_plugin_scan_count(), 1);
        assert!(
            doctor
                .findings
                .iter()
                .any(|finding| finding.code == crate::doctor::DoctorCode::ToolPendingApproval)
        );
        assert!(
            doctor
                .findings
                .iter()
                .any(|finding| { finding.code == crate::doctor::DoctorCode::HookToolUnavailable })
        );
    }

    #[test]
    fn agent_review_disclaimer_should_not_treat_no_findings_as_approval() {
        assert_eq!(
            agent_review_disclaimer(),
            "this review can add findings but cannot approve content; no additional findings are not an endorsement"
        );
    }

    #[test]
    fn agent_review_assessment_should_not_render_an_empty_review_as_safe() {
        assert_eq!(
            agent_review_assessment("This skill is safe.", 0),
            "no additional findings reported by the agent reviewer"
        );
        assert_eq!(
            agent_review_assessment("Found a network request.", 1),
            "Found a network request."
        );
    }

    #[test]
    fn build_status_report_should_report_missing_instruction_block() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let (store_root, target) = setup_enabled_instruction_pack(temp_dir.path(), "Body v1\n");
        fs::write(&target, "user-owned content\n").expect("target should be rewritten");

        let report = build_status_report(&store_root).expect("status should build");

        assert_eq!(report.instruction_block_drifts.len(), 1);
        assert_eq!(
            report.instruction_block_drifts[0].kind,
            instructions::InstructionBlockDriftKind::Missing
        );
    }

    #[test]
    fn build_status_report_should_report_stale_instruction_block() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let (store_root, _target) = setup_enabled_instruction_pack(temp_dir.path(), "Body v1\n");
        fs::write(
            store_root.join("local/instructions/house-style.md"),
            "Body v2\n",
        )
        .expect("pack should be updated");

        let report = build_status_report(&store_root).expect("status should build");

        assert_eq!(report.instruction_block_drifts.len(), 1);
        assert_eq!(
            report.instruction_block_drifts[0].kind,
            instructions::InstructionBlockDriftKind::Stale
        );
    }

    fn setup_enabled_instruction_pack(root: &Path, body: &str) -> (PathBuf, PathBuf) {
        let store_root = root.join("store");
        let target = root.join("AGENTS.md");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store_root.clone());
        fs::write(paths.local_instructions_dir.join("house-style.md"), body)
            .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");
        instructions::enable_pack(&paths, "house-style", &target, false)
            .expect("pack should be enabled");
        (store_root, target)
    }

    fn resolved_skill(source_ref: &str, slot_name: &str) -> resolver::ResolvedSkill {
        let (source_id, _) = source_ref.split_once(':').expect("ref has a source");
        resolver::ResolvedSkill {
            source_ref: source_ref.to_owned(),
            slot_name: slot_name.to_owned(),
            source_namespace: None,
            id: None,
            source_id: source_id.to_owned(),
            source_kind: SourceKind::Team,
            source_priority: 10,
            path: PathBuf::from(format!("/store/{slot_name}")),
            delivery: crate::inventory::SkillDelivery::Direct,
            compatibility: None,
            binaries: Vec::new(),
            local_override: false,
            requires: Vec::new(),
        }
    }

    fn override_resolution() -> Resolution {
        let mut winner = resolved_skill("local:review", "review");
        winner.local_override = true;
        Resolution {
            active_skills: vec![winner],
            pending_approval_skills: Vec::new(),
            unlinked_skills: vec![resolver::UnlinkedSkill {
                skill: resolved_skill("company:review", "review"),
                status: resolver::UnlinkedStatus::Unlinked,
                reason: resolver::UnlinkedReason::Shadowed,
                shadowed_by: "local:review".to_owned(),
            }],
            blocked_skills: Vec::new(),
            diagnostics: vec![
                resolver::ResolutionDiagnostic {
                    code: resolver::ResolutionDiagnosticCode::LocalOverride,
                    message: "local skill `local:review` overrides another source".to_owned(),
                    source_ref: Some("local:review".to_owned()),
                },
                resolver::ResolutionDiagnostic {
                    code: resolver::ResolutionDiagnosticCode::Shadowed,
                    message: "skill `company:review` is unlinked".to_owned(),
                    source_ref: Some("company:review".to_owned()),
                },
            ],
        }
    }

    #[test]
    fn sync_output_context_should_qualify_only_new_or_changed_preflights() {
        let resolution = override_resolution();
        let first = SyncOutputContext::new(&lockfile::UserLock::empty(), None, &resolution, &[]);
        assert!(first.preflight_covered_changes);
        assert!(first.settled_diagnostics.is_empty());

        let previous = lockfile::build_user_lock(&[], &resolution, None, None);
        let unchanged = SyncOutputContext::new(&previous, None, &resolution, &[]);
        assert!(!unchanged.preflight_covered_changes);
        assert_eq!(unchanged.settled_diagnostics.len(), 2);

        let edited =
            SyncOutputContext::new(&previous, None, &resolution, &["local:review".to_owned()]);
        assert!(edited.preflight_covered_changes);

        let mut moved_lock = previous;
        moved_lock.sources.push(lockfile::LockedSource {
            id: "local".to_owned(),
            kind: SourceKind::Local,
            path: PathBuf::from("/store/local"),
            subpath: None,
            commit: Some("a".repeat(40)),
        });
        let mut current_sources = moved_lock.sources.clone();
        current_sources[0].commit = Some("b".repeat(40));
        let moved = SyncOutputContext::new(&moved_lock, Some(&current_sources), &resolution, &[]);
        assert!(moved.preflight_covered_changes);
        let same = SyncOutputContext::new(&moved_lock, Some(&moved_lock.sources), &resolution, &[]);
        assert!(!same.preflight_covered_changes);
    }

    #[test]
    fn sync_output_context_should_announce_an_override_until_the_lock_records_it() {
        let resolution = override_resolution();
        let mut before_override = resolution.clone();
        before_override.active_skills = vec![resolved_skill("company:review", "review")];
        before_override.unlinked_skills.clear();
        before_override.diagnostics.clear();
        let previous = lockfile::build_user_lock(&[], &before_override, None, None);

        let context = SyncOutputContext::new(&previous, None, &resolution, &[]);

        assert!(context.settled_diagnostics.is_empty());
        assert!(
            resolution
                .diagnostics
                .iter()
                .all(|diagnostic| !context.is_settled(diagnostic))
        );
    }

    #[test]
    fn waiting_for_sync_should_group_subjects_and_name_the_change() {
        let drift = vec![
            LockDrift {
                code: LockDriftCode::SourceAdded,
                subject: "company".to_owned(),
                message: String::new(),
            },
            LockDrift {
                code: LockDriftCode::ActiveRemoved,
                subject: "company:review".to_owned(),
                message: String::new(),
            },
            LockDrift {
                code: LockDriftCode::UnlinkedAdded,
                subject: "company:review".to_owned(),
                message: String::new(),
            },
        ];

        assert!(drift_awaits_sync(&drift));
        assert_eq!(
            waiting_for_sync_phrase(&drift),
            "2 changes waiting for sync"
        );
        assert_eq!(
            waiting_for_sync_lines(&drift),
            vec![
                ("company (source)".to_owned(), "new source".to_owned()),
                (
                    "review (company)".to_owned(),
                    "no longer active, now shadowed".to_owned()
                ),
            ]
        );

        let skills_only = &drift[1..];
        assert_eq!(
            waiting_for_sync_phrase(skills_only),
            "1 skill waiting for sync"
        );
        let mut moved = drift.clone();
        moved.push(LockDrift {
            code: LockDriftCode::SourceCommitChanged,
            subject: "company".to_owned(),
            message: String::new(),
        });
        assert!(!drift_awaits_sync(&moved));
        assert!(!drift_awaits_sync(&[]));

        let selected = vec![
            LockDrift {
                code: LockDriftCode::SourceAdded,
                subject: "public".to_owned(),
                message: String::new(),
            },
            LockDrift {
                code: LockDriftCode::PendingApprovalAdded,
                subject: "public:helper".to_owned(),
                message: String::new(),
            },
        ];
        assert!(drift_awaits_sync(&selected));
        let waiting = waiting_for_sync_drift(&selected);
        assert_eq!(
            waiting_for_sync_lines(&waiting),
            vec![("public (source)".to_owned(), "new source".to_owned())]
        );
        assert_eq!(
            waiting_for_sync_phrase(&waiting),
            "1 change waiting for sync"
        );
        assert!(waiting_for_sync_drift(&selected[1..]).is_empty());
    }

    fn clean_audit(source_ref: &str) -> AuditReport {
        AuditReport {
            schema_version: 1,
            source_ref: source_ref.to_owned(),
            skill_path: PathBuf::from("/tmp/skill"),
            content_hash: "0".repeat(64),
            static_engine_version: "5".to_owned(),
            static_scan_excludes_root_source_metadata: Some(false),
            scanned_at_unix: 0,
            coverage: AuditCoverage::Complete,
            status: AuditStatus::Clean,
            max_severity: None,
            static_findings: Vec::new(),
            spec_findings: Vec::new(),
            binaries: Vec::new(),
            agent_review: None,
            risk_acceptance: None,
        }
    }

    #[test]
    fn audit_summary_should_stay_one_line_only_for_clean_complete_results() {
        let clean = clean_audit("public:review-helper");
        assert!(audit_is_quiet(&clean));

        let mut partial = clean.clone();
        partial.coverage = AuditCoverage::Partial;
        assert!(!audit_is_quiet(&partial));

        let finding = audit::AuditFinding {
            id: "static.test".to_owned(),
            severity: audit::Severity::Low,
            category: "category".to_owned(),
            path: "SKILL.md".to_owned(),
            line: None,
            message: "SKILL.md has no frontmatter\u{1b}[2J".to_owned(),
            evidence: None,
        };
        let mut low = clean.clone();
        low.static_findings.push(finding.clone());
        assert!(!audit_is_quiet(&low));

        let mut review = clean.clone();
        review.status = AuditStatus::Review;
        assert!(!audit_is_quiet(&review));

        let mut noted = clean.clone();
        noted.spec_findings = vec![finding.clone(), finding];
        assert!(audit_is_quiet(&noted));
        let store_root = PathBuf::from("/tmp/dalo-status-test-store");
        assert_eq!(spec_note_line(&store_root, &clean), None);
        let note = spec_note_line(&store_root, &noted).expect("spec notes are summarized");
        assert!(
            note.starts_with("2 spec notes: SKILL.md has no frontmatter"),
            "{note}"
        );
        assert!(note.contains("; 1 more ("), "{note}");
        assert!(
            note.contains("audit public:review-helper shows them)"),
            "{note}"
        );
        assert!(!note.contains('\u{1b}'), "{note}");
    }
}
