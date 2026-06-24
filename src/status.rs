//! Status model and renderable command output.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adopt::{AdoptReport, KeepReport, RemoveOwnedReport, ResolveListReport, UnmanagedSkill};
use crate::error::SkillmgrResult;
use crate::inventory::{self, InventoryWarning};
use crate::lockfile::{self, LockDrift};
use crate::materialize::SyncReport;
use crate::resolver::{self, Resolution, ResolutionInput};
use crate::source::{SourceAddReport, SourceKind, SourceListReport, SourcePriorityReport};
use crate::store::{self, InitReport, StorePaths};
use crate::target::{TargetDetectReport, TargetLinkReport, TargetUnlinkReport};

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
pub fn build_status_report(store_root: &Path) -> SkillmgrResult<StatusReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let config = store::read_config(&paths)?;
    let approvals = store::read_approvals(&paths)?;
    let previous_lock = store::read_user_lock(&paths)?;
    let mut sources = Vec::new();
    let mut inventories = Vec::new();
    let mut inventory_warnings = Vec::new();

    for source in &config.sources {
        if !source.enabled {
            sources.push(SourceStatus {
                id: source.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                priority: source.priority,
                enabled: false,
                exists: source.path.exists(),
                skill_count: 0,
                error: None,
            });
            continue;
        }

        if !source.path.exists() {
            sources.push(SourceStatus {
                id: source.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                priority: source.priority,
                enabled: true,
                exists: false,
                skill_count: 0,
                error: Some("source path does not exist".to_owned()),
            });
            continue;
        }

        match inventory::scan_source(&source.id, &source.path) {
            Ok(inventory) => {
                let skill_count = inventory.skills.len();
                inventory_warnings.extend(inventory.warnings.clone());
                inventories.push(inventory);
                sources.push(SourceStatus {
                    id: source.id.clone(),
                    kind: source.kind,
                    path: source.path.clone(),
                    priority: source.priority,
                    enabled: true,
                    exists: true,
                    skill_count,
                    error: None,
                });
            }
            Err(error) => {
                sources.push(SourceStatus {
                    id: source.id.clone(),
                    kind: source.kind,
                    path: source.path.clone(),
                    priority: source.priority,
                    enabled: true,
                    exists: true,
                    skill_count: 0,
                    error: Some(error.to_string()),
                });
            }
        }
    }

    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    inventory_warnings.sort_by(|left, right| left.path.cmp(&right.path));

    let resolution = resolver::resolve(&ResolutionInput {
        sources: config.sources.clone(),
        inventories,
        approvals: approvals.approvals,
        previous_lock: Some(previous_lock.clone()),
    });
    let live_lock = lockfile::build_user_lock(&config.sources, &resolution, None);
    let lock = LockStatus {
        path: paths.lock_file.clone(),
        schema_version: previous_lock.schema_version,
        drift: lockfile::compare_user_lock(&previous_lock, &live_lock),
    };
    let unmanaged_skills = crate::adopt::discover_unmanaged_skills(&paths)?;

    Ok(StatusReport {
        store: store_root.to_path_buf(),
        sources,
        inventory_warnings,
        resolution,
        lock,
        unmanaged_skills,
    })
}

/// Print a human-readable init report.
pub fn print_init_report(report: &InitReport) {
    println!("skillmgr store: {}", report.store.display());

    for operation in &report.operations {
        println!(
            "{:<8} {:<12} {}",
            operation.status.as_str(),
            operation.action.as_str(),
            operation.path.display()
        );
    }
}

/// Print a human-readable status report.
pub fn print_status_report(report: &StatusReport) {
    println!("skillmgr store: {}", report.store.display());
    println!("sources:");
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
            "  {:<12} {:<5?} priority={:<4} skills={:<3} {}{}",
            source.id, source.kind, source.priority, source.skill_count, state, error
        );
    }

    println!("active skills:");
    for skill in &report.resolution.active_skills {
        let marker = if skill.local_override {
            " local_override"
        } else {
            ""
        };
        println!("  {} -> {}{}", skill.slot_name, skill.source_ref, marker);
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

    if !report.lock.drift.is_empty() {
        println!("lock drift:");
        for drift in &report.lock.drift {
            println!("  {:?} {}: {}", drift.code, drift.subject, drift.message);
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
                "  {:?} {}: {}",
                warning.code,
                warning.path.display(),
                warning.message
            );
        }
    }
}

/// Print a human-readable sync report.
pub fn print_sync_report(report: &SyncReport) {
    println!("skillmgr store: {}", report.store.display());
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
    println!(
        "added source {} -> {}",
        report.source.id,
        report.source.path.display()
    );
}

/// Print a human-readable source list report.
pub fn print_source_list_report(report: &SourceListReport) {
    for source in &report.sources {
        println!(
            "{:<12} {:<5?} priority={:<4} enabled={} {}",
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
    println!(
        "updated source {} priority={}",
        report.source.id, report.source.priority
    );
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
}

/// Print a human-readable keep report.
pub fn print_keep_report(report: &KeepReport) {
    let status = if report.existing {
        "existing"
    } else {
        "protected"
    };
    println!("{status} {}", report.skill.path.display());
}

/// Print a human-readable remove-owned report.
pub fn print_remove_owned_report(report: &RemoveOwnedReport) {
    println!("{} {}", report.status.as_str(), report.link_path.display());
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
}
