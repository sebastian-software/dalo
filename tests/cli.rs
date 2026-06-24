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
        .arg("doctor")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "command `doctor` is not implemented yet",
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

#[test]
fn sync_dry_run_should_not_create_symlink() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--dry-run", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("planned"));

    assert!(!target.join("review").exists());
}

#[test]
fn sync_should_create_directory_symlink() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("applied"));

    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("link should exist")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn sync_should_report_existing_on_second_run() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("existing"));
}

#[test]
fn sync_should_write_user_lock_with_active_and_unlinked_skills() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let local_skill_dir = store.join("local/skills/team");
    std::fs::create_dir_all(&local_skill_dir).expect("local skill dir should be created");
    std::fs::write(local_skill_dir.join("SKILL.md"), "# Local Team\n")
        .expect("local skill should be written");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    approve_source(&store, "company");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();

    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    let lock = std::fs::read_to_string(store.join("lock.toml")).expect("lock should be readable");
    assert!(lock.contains("source_ref = \"local:team\""));
    assert!(lock.contains("source_ref = \"company:team\""));
    assert!(lock.contains("reason = \"shadowed\""));
    assert!(lock.contains("status = \"applied\"") || lock.contains("status = \"existing\""));
}

#[test]
fn status_json_should_report_lock_drift_after_skill_removal() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    std::fs::remove_dir_all(store.join("local/skills/review"))
        .expect("local skill should be removed");
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"code\": \"active_removed\""))
        .stdout(predicate::str::contains("\"subject\": \"local:review\""));
}

#[test]
fn status_should_fail_on_unsupported_lock_schema() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(store.join("lock.toml"), "schema_version = 999\n")
        .expect("lock should be overwritten");
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("status")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "unsupported lock schema version 999",
        ));
}

#[test]
fn sync_should_remove_owned_symlink_after_source_is_removed_from_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
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
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    approve_source(&store, "company");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(
        std::fs::symlink_metadata(target.join("team"))
            .expect("team link should exist")
            .file_type()
            .is_symlink()
    );

    write_local_only_config(&store);
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    assert!(!target.join("team").exists());
}

#[test]
fn source_add_should_clone_team_source_into_store() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
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
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("added source company"));

    assert!(store.join("sources/company/checkout/.git").is_dir());
}

#[test]
fn source_priority_should_update_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
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
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["source", "priority", "company", "3"])
        .assert()
        .success()
        .stdout(predicate::str::contains("priority=3"));
}

#[test]
fn sync_should_block_dirty_team_source() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
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
        .args(["source", "add", "company"])
        .arg(&repo)
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
    std::fs::write(
        store.join("sources/company/checkout/skills/team/SKILL.md"),
        "# Dirty\n",
    )
    .expect("checkout should be dirtied");
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains(
            "source `company` has local changes",
        ));
}

fn setup_store_with_skill_and_target(store: &std::path::Path, target: &std::path::Path) {
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .arg("init")
        .assert()
        .success();
    let skill_dir = store.join("local/skills/review");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");
    Command::cargo_bin("skillmgr")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .args(["target", "link", "generic"])
        .arg(target)
        .assert()
        .success();
}

fn create_git_skill_repo(repo: &std::path::Path) {
    std::fs::create_dir_all(repo.join("skills/team")).expect("repo dirs should be created");
    std::fs::write(repo.join("skills/team/SKILL.md"), "# Team\n").expect("skill should be written");
    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "initial",
            "-q",
        ],
    );
}

fn approve_source(store: &std::path::Path, source: &str) {
    std::fs::write(
        store.join("approvals.toml"),
        format!("schema_version = 1\n\n[[approvals]]\nscope = \"source\"\nvalue = \"{source}\"\n"),
    )
    .expect("source approval should be written");
}

fn write_local_only_config(store: &std::path::Path) {
    std::fs::write(
        store.join("config.toml"),
        format!(
            "version = 1\n\n[settings]\nautosync = false\n\n[[sources]]\nid = \"local\"\nkind = \"local\"\npath = \"{}\"\npriority = 0\nenabled = true\ntrusted = true\n",
            store.join("local").display()
        ),
    )
    .expect("config should be written");
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("git should run");
    assert!(status.success());
}
