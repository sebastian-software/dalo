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
        .arg("sync")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "command `sync` is not implemented yet",
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

#[test]
fn status_json_should_report_local_skill_as_active() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let skill_dir = store.join("local/skills/review");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"source_ref\": \"local:review\""))
        .stdout(predicate::str::contains("\"active_skills\""));
}

#[test]
fn target_detect_should_report_known_targets() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "target", "detect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"codex\""))
        .stdout(predicate::str::contains("\"id\": \"hermes\""))
        .stdout(predicate::str::contains("\"id\": \"opencode\""));
}

#[test]
fn target_link_generic_should_create_directory_and_update_state() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success()
        .stdout(predicate::str::contains("linked target generic"));

    assert!(target.is_dir());
    assert!(
        std::fs::read_to_string(store.join("state.toml"))
            .expect("state should be readable")
            .contains("generic")
    );
}

#[test]
fn target_unlink_should_keep_target_directory() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["target", "unlink", "generic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unlinked target generic"));

    assert!(target.is_dir());
}
