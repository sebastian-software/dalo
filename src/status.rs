//! Status model and renderable command output.

use crate::store::InitReport;

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
