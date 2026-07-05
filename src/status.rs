//! Status model and renderable command output.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adopt::{
    AdoptReport, KeepReport, RemoveOwnedReport, ResolveListReport, TargetScanWarning,
    UnmanagedSkill,
};
use crate::catalog::{CatalogDrift, CatalogInspectReport, CatalogSelectReport};
use crate::doctor::{DoctorReport, DoctorSeverity};
use crate::error::DaloResult;
use crate::instructions::{
    self, DiscoveredPack, InstructionBlockDrift, InstructionPackReport, TopicOverlap,
};
use crate::inventory::InventoryWarning;
use crate::lockfile::{self, LockDrift, LockDriftCode};
use crate::materialize::SyncReport;
use crate::resolver::{self, Resolution};
use crate::source::{
    SourceAddReport, SourceConfig, SourceKind, SourceListReport, SourcePriorityReport,
};
use crate::store::{self, InitReport, StorePaths};
use crate::target::{TargetDetectReport, TargetLinkReport, TargetUnlinkReport};
use crate::term;

/// Full status report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    /// Store root.
    pub store: PathBuf,
    /// Source scan summaries.
    pub sources: Vec<SourceStatus>,
    /// Inventory warnings.
    pub inventory_warnings: Vec<InventoryWarning>,
    /// Resolution output.
    pub resolution: Resolution,
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
}

/// Build the current status report.
#[must_use = "the status report should be rendered or inspected"]
pub fn build_status_report(store_root: &Path) -> DaloResult<StatusReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let config = store::read_config(&paths)?;
    let approvals = store::read_approvals(&paths)?;
    let previous_lock = store::read_user_lock(&paths)?;

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

    let resolution = live.resolution;
    let live_lock = lockfile::build_user_lock(&config.sources, &resolution, None);
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

    Ok(StatusReport {
        store: store_root.to_path_buf(),
        sources,
        inventory_warnings,
        resolution,
        lock,
        unmanaged_skills: unmanaged_scan.unmanaged_skills,
        target_warnings: unmanaged_scan.warnings,
        instruction_packs,
        instruction_pack_overlaps,
        instruction_block_drifts,
    })
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
    println!("Store ready.");
    println!("Next steps:");
    println!("  1. dalo target link <codex|claude|openclaw|hermes>");
    println!("  2. dalo source add <id> <git-url>");
    println!("  3. dalo sync");
}

/// Print a human-readable status report.
pub fn print_status_report(report: &StatusReport) {
    println!("dalo store: {}", report.store.display());
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
            println!("  {} -> {}", skill.slot_name, skill.source_ref);
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

/// Print a human-readable sync report.
pub fn print_sync_report(report: &SyncReport) {
    println!("dalo store: {}", report.store.display());
    if report.operations.is_empty() {
        println!("nothing to sync: 0 skills materialized; store is up to date");
        return;
    }
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

/// Print a human-readable source add report.
pub fn print_source_add_report(report: &SourceAddReport) {
    let verb = if report.dry_run { "would add" } else { "added" };
    println!(
        "{verb} source {} -> {}",
        report.source.id,
        report.source.path.display()
    );
}

/// Print a human-readable source list report.
pub fn print_source_list_report(report: &SourceListReport) {
    if report.sources.is_empty() {
        println!("no sources configured");
        return;
    }
    for source in &report.sources {
        println!(
            "{:<12} {:<5} priority={:<4} enabled={} {}",
            source.id,
            source.kind,
            source.priority,
            source.enabled,
            source.path.display()
        );
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
