//! Read-only reference validator for the experimental portable package profile.
//! Run with `cargo run --example validate_package -- <source-root>`.

use std::path::PathBuf;
use std::process::ExitCode;

use dalo::plugin;
use serde_json::json;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: validate_package <source-root>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: validate_package <source-root>");
        return ExitCode::from(2);
    }
    let root = PathBuf::from(root);
    if !root.is_dir() {
        eprintln!("source root must be an existing directory");
        return ExitCode::from(2);
    }
    let inventory = plugin::scan_source_plugins("validation", &root);
    let valid = !inventory.plugins.is_empty() && inventory.warnings.is_empty();
    println!(
        "{}",
        json!({
            "profile": "portable-agent-packages/0.1",
            "valid": valid,
            "packages": inventory.plugins,
            "warnings": inventory.warnings,
            "checks": "package structure, local tool files, hook contracts and bindings",
            "not_checked": ["member/dependency resolution", "provider installation",
                "runtime availability", "handler behavior", "trust or activation"],
        })
    );
    if valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
