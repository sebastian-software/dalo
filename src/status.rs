//! Status model and renderable command output.

use crate::store::InitReport;
use crate::target::{TargetDetectReport, TargetLinkReport, TargetUnlinkReport};

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
