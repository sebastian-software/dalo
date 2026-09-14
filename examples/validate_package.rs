//! Read-only reference validator for the experimental portable package profile.
//! Run with `cargo run --example validate_package -- <source-root>`.

use std::path::PathBuf;
use std::process::ExitCode;

use dalo::package_validation;

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
    match package_validation::validate(&root) {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if report.valid {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("validation failed: {error}");
            ExitCode::FAILURE
        }
    }
}
