//! Focused command-line contract tests for store-independent package validation.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packages/source")
}

#[test]
fn human_validation_separates_provider_capability_from_authorization() {
    let output = Command::cargo_bin("dalo")
        .unwrap()
        .args(["plugin", "validate"])
        .arg(source_root())
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("hooks: supported"));
    assert!(text.contains("execution: not authorized by validation"));
}

#[test]
fn validate_json_works_without_an_initialized_store() {
    let output = Command::cargo_bin("dalo")
        .unwrap()
        .args(["--json", "plugin", "validate"])
        .arg(source_root())
        .env("DALO_STORE", "/path/that/does/not/exist")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["package_contract"]["valid"], true);
    assert_eq!(report["execution_authorization"]["status"], "never_granted");
    assert_eq!(report["execution_authorization"]["check"], "not_applicable");
    assert_eq!(report["providers"][0]["status"], "supported");
    assert_eq!(report["providers"][1]["status"], "supported");
}

#[test]
fn validate_is_read_only_and_does_not_run_handlers() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/example");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("PLUGIN.toml"),
        "schema_version = 1\n[plugin]\nname = \"example\"\ndescription = \"Example\"\n\n[[tool]]\nschema_version = 1\nid = \"handler\"\nentry = \"handler.mjs\"\nruntime = \"node\"\nargv = []\ncwd = \"tool_root\"\navailability = \"required\"\n",
    )
    .unwrap();
    fs::write(
        package.join("handler.mjs"),
        "require('fs').writeFileSync('executed', 'bad');\n",
    )
    .unwrap();

    let output = Command::cargo_bin("dalo")
        .unwrap()
        .args(["--json", "plugin", "validate"])
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!package.join("executed").exists());
    assert!(!temp.path().join("lock.toml").exists());
}

#[test]
fn validate_reports_invalid_packages_on_stdout_and_returns_failure() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/broken");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("PLUGIN.toml"), "schema_version = 99\n").unwrap();

    let output = Command::cargo_bin("dalo")
        .unwrap()
        .args(["--json", "plugin", "validate"])
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], false);
    assert_eq!(report["package_contract"]["valid"], false);
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| { finding["code"] == "invalid_plugin_package" })
    );
}
