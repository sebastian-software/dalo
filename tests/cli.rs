use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_should_list_planned_top_level_commands() {
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("source"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("adopt"))
        .stdout(predicate::str::contains("resolve"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn stubbed_command_should_fail_with_clear_message() {
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .arg("status")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "command `status` is not implemented yet",
        ));
}

#[test]
fn init_dry_run_json_should_not_create_store() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "--dry-run", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\": true"))
        .stdout(predicate::str::contains("\"status\": \"planned\""));

    assert!(!store.exists());
}

#[test]
fn init_should_create_store_layout() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("created"));

    assert!(store.join("config.toml").is_file());
    assert!(store.join("lock.toml").is_file());
    assert!(store.join("state.toml").is_file());
    assert!(store.join("approvals.toml").is_file());
    assert!(store.join("local/.git").is_dir());
}
