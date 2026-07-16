//! Status model and renderable command output.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adopt::{
    AdoptReport, KeepReport, RemoveOwnedReport, ResolveListReport, TargetScanWarning, UnkeepReport,
    UnmanagedSkill,
};
use crate::approval::ApprovalReport;
use crate::audit::{self, AuditCoverage, AuditOptions, AuditReport, AuditStatus};
use crate::autosync::{AutosyncMutationReport, AutosyncStatusReport};
use crate::catalog::{
    self, CatalogAdvanceReport, CatalogDrift, CatalogInspectReport, CatalogSelectReport,
};
use crate::doctor::{DoctorReport, DoctorSeverity};
use crate::error::DaloResult;
use crate::instructions::{
    self, DiscoveredPack, InstructionBlockDrift, InstructionPackReport, TopicOverlap,
};
use crate::inventory::InventoryWarning;
use crate::lockfile::{self, LockDrift, LockDriftCode};
use crate::materialize::{self, MaterializeOperation, MaterializeOperationStatus, SyncReport};
use crate::resolver::{self, Resolution};
use crate::source::{
    SourceAddReport, SourceConfig, SourceKind, SourceListReport, SourcePriorityReport,
    SourceProvenance, SourceRemoveReport,
};
use crate::store::{self, ApprovalsFile, InitReport, StorePaths};
use crate::target::{TargetDetectReport, TargetLinkReport, TargetUnlinkReport};
use crate::team_manifest::{
    TeamCatalogUpdateReport, TeamManifestAction, TeamManifestMutationReport, TeamManifestView,
};
use crate::term;

/// Full status report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    /// Store root.
    pub store: PathBuf,
    /// Source scan summaries.
    pub sources: Vec<SourceStatus>,
    /// Configured materialization targets.
    pub targets: Vec<TargetStatus>,
    /// Inventory warnings.
    pub inventory_warnings: Vec<InventoryWarning>,
    /// Resolution output.
    pub resolution: Resolution,
    /// Dry-run materialization operations that expose target-level blockers.
    pub materialization: Vec<MaterializeOperation>,
    /// Active skills whose deterministic security audit blocks sync.
    pub blocking_audits: Vec<String>,
    /// Previous-lock comparison against the live resolution.
    pub lock: LockStatus,
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
    /// Whether the source is enabled.
    pub enabled: bool,
    /// Whether the source path exists.
    pub exists: bool,
    /// Number of scanned skills.
    pub skill_count: usize,
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
    let source_lock = catalog::read_source_lock(&paths).ok();

    // The shared pipeline scans every enabled source once and resolves it; we
    // reuse its per-source scan outcomes here for the status detail instead of
    // re-scanning. Disabled sources are not scanned, so we render them directly.
    let live = resolver::resolve_from_config(&config, approvals.approvals);
    let scan_by_id = live
        .scans
        .iter()
        .map(|scan| (scan.source.id.as_str(), scan))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut sources = Vec::new();
    let mut inventory_warnings = Vec::new();

    for source in &config.sources {
        let status = if let Some(scan) = scan_by_id.get(source.id.as_str()) {
            if let Some(inventory) = &scan.inventory {
                inventory_warnings.extend(inventory.warnings.iter().cloned());
            }
            SourceStatus {
                id: source.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                priority: source.priority,
                enabled: true,
                exists: source.path.exists(),
                skill_count: scan.inventory.as_ref().map_or(0, |inv| inv.skills.len()),
                error: scan.error.clone(),
                provenance: crate::source::source_provenance(source, source_lock.as_ref()),
            }
        } else {
            SourceStatus {
                id: source.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                priority: source.priority,
                enabled: false,
                exists: source.path.exists(),
                skill_count: 0,
                error: None,
                provenance: crate::source::source_provenance(source, source_lock.as_ref()),
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

    let live_resolution = live.resolution;
    let blocking_audits = collect_blocking_audits(&paths, &live_resolution)?;
    let materialization = materialize::materialize(&paths, &live_resolution, true)?;
    let resolution = materialization.resolution;
    let live_lock = lockfile::build_user_lock(&config.sources, &live_resolution, None);
    let mut drift = lockfile::compare_user_lock(&previous_lock, &live_lock);
    suppress_initial_local_source_drift(&previous_lock, &mut drift);
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
        last_run: None,
    });

    Ok(StatusReport {
        store: store_root.to_path_buf(),
        sources,
        targets,
        inventory_warnings,
        resolution,
        materialization: materialization.operations,
        blocking_audits,
        lock,
        unmanaged_skills: unmanaged_scan.unmanaged_skills,
        target_warnings: unmanaged_scan.warnings,
        instruction_packs,
        instruction_pack_overlaps,
        instruction_block_drifts,
        autosync,
    })
}

fn collect_blocking_audits(paths: &StorePaths, resolution: &Resolution) -> DaloResult<Vec<String>> {
    let mut blocked = Vec::new();
    for skill in &resolution.active_skills {
        let report = audit::audit_skill(
            paths,
            &skill.source_ref,
            &skill.path,
            &AuditOptions {
                persist: false,
                ..AuditOptions::default()
            },
        )?;
        if report.is_blocking() {
            blocked.push(skill.source_ref.clone());
        }
    }
    Ok(blocked)
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

/// Print a human-readable init report.
pub fn print_init_report(report: &InitReport) {
    println!("dalo store: {}", report.store.display());

    for operation in &report.operations {
        println!(
            "{:<8} {:<12} {}",
            operation.status.as_str(),
            operation.action.as_str(),
            operation.path.display()
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
            println!("  warning {}: {}", warning.path.display(), warning.message);
        }
        println!("Fix the files above before using dalo.");
        return;
    }

    if state_repaired {
        return;
    }

    println!("Store ready.");
    println!("Next steps:");
    println!("  1. dalo target link <codex|claude|openclaw|hermes>");
    println!("  2. dalo source add <id> <git-url>");
    println!("  3. dalo sync");
}

/// Print local approval records.
pub fn print_approval_list(report: &ApprovalsFile) {
    if report.approvals.is_empty() {
        println!("no approvals recorded");
        return;
    }
    for approval in &report.approvals {
        println!("{} {}", approval.scope, approval.value);
    }
}

/// Print one approval mutation result.
pub fn print_approval_report(report: &ApprovalReport) {
    let verb = if report.dry_run && report.action != "unchanged" {
        "planned"
    } else {
        report.action.as_str()
    };
    println!("{verb} {} {}", report.scope, report.value);
}

/// Print a human-readable layered skill security audit.
pub fn print_audit_report(report: &AuditReport) {
    println!("security audit: {}", report.source_ref);
    println!("  content hash: {}", report.content_hash);
    println!(
        "  coverage: {}",
        match report.coverage {
            AuditCoverage::Complete => "complete",
            AuditCoverage::Partial => "partial",
        }
    );
    println!(
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
    );
    for finding in &report.static_findings {
        print_audit_finding("static", finding);
    }
    if let Some(review) = &report.agent_review {
        println!(
            "  agent review: {} (isolation: {}; non-authoritative)",
            review.provider.as_str(),
            review.isolation.as_str()
        );
        println!(
            "    assessment: {}",
            agent_review_assessment(&review.summary, review.findings.len())
        );
        println!("    additional findings: {}", review.findings.len());
        for capability in &review.expected_capabilities {
            println!("    capability: {capability}");
        }
        for action in &review.expected_actions {
            println!("    expected action: {action}");
        }
        for behavior in &review.undeclared_behaviors {
            println!("    undeclared: {behavior}");
        }
        for finding in &review.findings {
            print_audit_finding("agent", finding);
        }
        println!("    note: {}", agent_review_disclaimer());
    }
    if let Some(acceptance) = &report.risk_acceptance {
        println!("  risk accepted: {}", acceptance.reason);
    } else if report.status == AuditStatus::Blocked {
        println!("  installation policy: blocked until risk is explicitly accepted");
    }
    println!("  note: no findings means no known issue was detected; it is not a safety guarantee");
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

fn print_audit_finding(layer: &str, finding: &crate::audit::AuditFinding) {
    let location = finding.line.map_or_else(
        || finding.path.clone(),
        |line| format!("{}:{line}", finding.path),
    );
    println!(
        "  {} {} {} [{}]: {}",
        layer,
        finding.severity.as_str(),
        location,
        finding.category,
        finding.message
    );
}

/// Print a human-readable status report.
pub fn print_status_report(report: &StatusReport) {
    println!("dalo store: {}", report.store.display());
    print_autosync_status_report(&report.autosync);
    println!("sources:");
    if report.sources.is_empty() {
        println!("  none");
    } else {
        for source in &report.sources {
            let state = if source.enabled {
                "enabled"
            } else {
                "disabled"
            };
            let error = source
                .error
                .as_ref()
                .map_or(String::new(), |error| format!(" ({error})"));
            println!(
                "  {:<12} {:<5} priority={:<4} skills={:<3} {}{}",
                source.id, source.kind, source.priority, source.skill_count, state, error
            );
            print_source_provenance(&source.provenance, "    ");
        }
    }

    println!("targets:");
    if report.targets.is_empty() {
        println!("  none linked (run: dalo target link <codex|claude|openclaw|hermes>)");
    } else {
        for target in &report.targets {
            let state = if !target.linked {
                "unlinked"
            } else if target.exists {
                "linked"
            } else {
                "missing"
            };
            println!("  {:<12} {:<7} {}", target.id, state, target.path.display());
        }
    }

    println!("active skills:");
    if report.resolution.active_skills.is_empty() {
        println!("  none");
    } else {
        for skill in &report.resolution.active_skills {
            let marker = if skill.local_override {
                " local_override"
            } else {
                ""
            };
            println!("  {} -> {}{}", skill.slot_name, skill.source_ref, marker);
        }
    }

    if !report.resolution.pending_approval_skills.is_empty() {
        println!("pending approval:");
        for skill in &report.resolution.pending_approval_skills {
            println!(
                "  {} -> {} (run: dalo approve skill {})",
                skill.slot_name, skill.source_ref, skill.source_ref
            );
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
                "  {source_ref} (run: dalo audit {source_ref}; accept only with an explicit --accept-risk reason)"
            );
        }
    }

    let blocked_operations = report
        .materialization
        .iter()
        .filter(|operation| operation.status == MaterializeOperationStatus::Blocked)
        .collect::<Vec<_>>();
    if !blocked_operations.is_empty() {
        println!("materialization blocks:");
        for operation in blocked_operations {
            let reason = operation.reason.as_deref().unwrap_or("blocked");
            println!("  {}: {reason}", operation.link_path.display());
        }
    }

    if !report.resolution.diagnostics.is_empty() {
        println!("resolution diagnostics:");
        for diagnostic in &report.resolution.diagnostics {
            println!(
                "  {}: {}",
                resolver::diagnostic_code_name(diagnostic.code),
                diagnostic.message
            );
        }
    }

    if !report.lock.drift.is_empty() {
        println!("lock drift:");
        for drift in &report.lock.drift {
            println!("  {} {}: {}", drift.code, drift.subject, drift.message);
        }
    }

    if !report.unmanaged_skills.is_empty() {
        println!("unmanaged skills:");
        for skill in &report.unmanaged_skills {
            let marker = if skill.protected { " protected" } else { "" };
            println!("  {} -> {}{}", skill.id, skill.path.display(), marker);
        }
    }

    if !report.inventory_warnings.is_empty() {
        println!("inventory warnings:");
        for warning in &report.inventory_warnings {
            println!(
                "  {} {}: {}",
                warning.code,
                warning.path.display(),
                warning.message
            );
        }
    }

    if !report.target_warnings.is_empty() {
        println!("target warnings:");
        for warning in &report.target_warnings {
            println!(
                "  {} {}: {}",
                warning.code.as_str(),
                warning.path.display(),
                warning.message
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
                drift.target.display(),
                drift.message
            );
        }
    }
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
            run.last_attempted_at_unix
        );
        if let Some(success) = run.last_successful_at_unix {
            println!("  last success: {success}");
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

/// Print a human-readable sync report.
pub fn print_sync_report(report: &SyncReport) {
    println!("dalo store: {}", report.store.display());
    if report.operations.is_empty() {
        if report.linked_targets == 0 && !report.resolution.active_skills.is_empty() {
            println!(
                "nothing materialized: {} skills resolved but no targets are linked; run `dalo target link <codex|claude|openclaw|hermes>`",
                report.resolution.active_skills.len()
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
        } else {
            println!("nothing materialized: resolution is incomplete");
        }
    } else {
        for operation in &report.operations {
            let desired = operation
                .desired_path
                .as_ref()
                .map_or(String::new(), |path| format!(" -> {}", path.display()));
            let reason = operation
                .reason
                .as_ref()
                .map_or(String::new(), |reason| format!(" ({reason})"));
            println!(
                "{:<8} {:<10} {}{}{}",
                operation.status.as_str(),
                operation.kind.as_str(),
                operation.link_path.display(),
                desired,
                reason
            );
        }
    }
    let prefix = if report.operations.is_empty() {
        "  "
    } else {
        ""
    };
    for skill in &report.resolution.pending_approval_skills {
        println!(
            "{prefix}pending approval: {} (run: dalo approve skill {})",
            skill.source_ref, skill.source_ref
        );
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
    for diagnostic in &report.resolution.diagnostics {
        println!(
            "{prefix}diagnostic: {}: {}",
            resolver::diagnostic_code_name(diagnostic.code),
            diagnostic.message
        );
    }
    println!(
        "{prefix}security preflight: deterministic checks and compatible cached findings only; sync did not run an agent reviewer; passing is not a safety guarantee"
    );
}

/// Print a human-readable source add report.
pub fn print_source_add_report(report: &SourceAddReport) {
    let verb = if report.dry_run { "would add" } else { "added" };
    println!(
        "{verb} source {} -> {}",
        report.source.id,
        report.source.path.display()
    );
    for audit in &report.audits {
        print_audit_report(audit);
    }
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
        println!("  checkout: removed {}", report.checkout_path.display());
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
    println!("  approvals removed: {}", report.removed_approvals);
    println!("  catalog lock removed: {}", report.removed_catalog_lock);
    if !report.deactivated_skills.is_empty() {
        println!("  deactivated skills:");
        for skill in &report.deactivated_skills {
            println!("    {skill}");
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
        println!(
            "{:<12} {:<7} priority={:<4} enabled={} {}{}",
            source.id,
            source.kind,
            source.priority,
            source.enabled,
            source.path.display(),
            managed
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
pub fn print_source_priority_report(report: &SourcePriorityReport) {
    let verb = if report.dry_run {
        "would update"
    } else {
        "updated"
    };
    println!(
        "{verb} source {} priority={}",
        report.source.id, report.source.priority
    );
}

/// Print a human-readable catalog add report.
pub fn print_catalog_add_report(source: &SourceConfig, dry_run: bool) {
    let verb = if dry_run { "would add" } else { "added" };
    println!(
        "{verb} catalog source {} -> {}",
        source.id,
        source.path.display()
    );
}

/// Print a human-readable catalog inspect report.
pub fn print_catalog_inspect_report(report: &CatalogInspectReport) {
    println!(
        "catalog {}: {} available skill(s)",
        report.source_id,
        report.candidates.len()
    );
    for candidate in &report.candidates {
        let marker = if candidate.selected { "*" } else { " " };
        let id = candidate.id.as_deref().unwrap_or("-");
        println!(
            "  {marker} {:<24} id={:<24} {}",
            candidate.slot_name, id, candidate.path
        );
    }
}

/// Print a human-readable catalog select report.
pub fn print_catalog_select_report(report: &CatalogSelectReport) {
    let verb = if report.dry_run {
        "would select"
    } else {
        "selected"
    };
    if report.selected.is_empty() {
        println!("catalog {}: no skills selected", report.source_id);
    } else {
        println!(
            "catalog {}: {verb} {}",
            report.source_id,
            report.selected.join(", ")
        );
    }
    for audit in &report.audits {
        print_audit_report(audit);
    }
    for warning in &report.migration_warnings {
        println!("warning: {warning}");
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
        report.pack_id,
        report.target.display()
    );
}

/// Print a human-readable catalog drift report.
pub fn print_catalog_drift_report(report: &CatalogDrift) {
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
    }
}

/// Print a reviewed catalog pin-advance plan or result.
pub fn print_catalog_advance_report(report: &CatalogAdvanceReport) {
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
    }
    for reason in &report.blocking_reasons {
        println!("  blocked: {reason}");
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

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// Print a human-readable adopt report.
pub fn print_adopt_report(report: &AdoptReport) {
    println!(
        "{} {} -> {}",
        report.copy.as_str(),
        report.source_path.display(),
        report.local_path.display()
    );
    println!("replacement: {}", report.replacement.as_str());
    if let Some(next_step) = report.next_step.as_deref() {
        println!("note: {next_step}");
    }
}

/// Print a human-readable resolve list report.
pub fn print_resolve_list_report(report: &ResolveListReport) {
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
            let marker = if skill.protected { " protected" } else { "" };
            println!("  {} -> {}{}", skill.id, skill.path.display(), marker);
        }
    }

    if !report.owned_skills.is_empty() {
        println!("owned symlinks:");
        for skill in &report.owned_skills {
            println!(
                "  {} -> {} ({})",
                skill.id,
                skill.link_path.display(),
                skill.store_path.display()
            );
        }
    }

    if !report.target_warnings.is_empty() {
        println!("target warnings:");
        for warning in &report.target_warnings {
            println!(
                "  {} {}: {}",
                warning.code.as_str(),
                warning.path.display(),
                warning.message
            );
        }
    }
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
}

/// Print a human-readable doctor report.
pub fn print_doctor_report(report: &DoctorReport) {
    println!("dalo store: {}", report.store.display());
    println!(
        "summary: errors={} warnings={} info={} ok={}",
        report.summary.errors, report.summary.warnings, report.summary.info, report.summary.ok
    );
    for finding in &report.findings {
        let next = finding
            .next_command
            .as_ref()
            .map_or(String::new(), |command| format!(" next={command}"));
        let severity = doctor_severity_label(finding.severity);
        let severity_padding = " ".repeat(7usize.saturating_sub(severity.len()));
        println!(
            "{}{} {}: {}{}",
            term::doctor_severity(severity),
            severity_padding,
            finding.code,
            finding.message,
            next
        );
    }
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

/// Print a human-readable target detection report.
pub fn print_target_detect_report(report: &TargetDetectReport) {
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
pub fn print_target_unlink_report(report: &TargetUnlinkReport) {
    println!("{} target {}", report.status.as_str(), report.target_id);
    if report.status == crate::target::TargetUnlinkStatus::Unlinked {
        println!("note: owned symlinks remain; run `dalo sync` to remove them");
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
        report.path.display()
    );
}

/// Print a parsed team manifest.
pub fn print_team_manifest_view(report: &TeamManifestView) {
    println!("team manifest: {}", report.path.display());
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
            catalog.id, catalog.version, skills, catalog.url
        );
    }
}

/// Print a reviewed team catalog pin update.
pub fn print_team_catalog_update(report: &TeamCatalogUpdateReport) {
    println!(
        "team catalog {}: {} -> {} (from {})",
        report.catalog_id,
        short_commit(&report.old_commit),
        short_commit(&report.candidate_commit),
        report.from_ref
    );
    if report.outcomes.is_empty() {
        println!("  inventory: unchanged");
    } else {
        println!("  inventory:");
        for outcome in &report.outcomes {
            println!("    {} {}", outcome.code.as_str(), outcome.message);
        }
    }
    if report.audits.is_empty() {
        println!("  audits: none");
    } else {
        println!("  audits:");
        for audit in &report.audits {
            let status = match audit.status {
                AuditStatus::Clean => "clean",
                AuditStatus::Review => "review",
                AuditStatus::Blocked => "blocked",
            };
            println!("    {} {status}", audit.source_ref);
        }
    }
    for reason in &report.blocking_reasons {
        println!("  blocked: {reason}");
    }
    let result = if !report.blocking_reasons.is_empty() {
        "not updated"
    } else if report.updated {
        "updated"
    } else if report.dry_run && report.old_version != report.candidate_commit {
        "would update"
    } else if report.old_version == report.candidate_commit {
        "already current"
    } else {
        "not updated"
    };
    println!("  result: {result} ({})", report.path.display());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

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
}
