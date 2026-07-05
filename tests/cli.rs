use assert_cmd::Command;
use dalo::store;
use predicates::prelude::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn help_should_list_planned_top_level_commands() {
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
fn help_should_render_implemented_command_groups() {
    for args in [
        vec!["target", "--help"],
        vec!["source", "--help"],
        vec!["resolve", "--help"],
        vec!["adopt", "--help"],
        vec!["status", "--help"],
        vec!["sync", "--help"],
        vec!["doctor", "--help"],
    ] {
        Command::cargo_bin("dalo")
            .expect("binary should build")
            .args(args)
            .assert()
            .success();
    }
}

#[test]
fn init_dry_run_json_should_not_create_store() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
fn init_should_use_dalo_store_environment_override() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store-from-env");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .env("DALO_STORE", &store)
        .arg("init")
        .assert()
        .success();

    assert!(store.join("config.toml").is_file());
}

#[test]
fn init_should_ignore_legacy_store_environment_override() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let home = temp_dir.path().join("home");
    let legacy_store = temp_dir.path().join("legacy-store");
    let legacy_store_env = ["SKILL", "MGR_STORE"].concat();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .env("HOME", &home)
        .env(legacy_store_env, &legacy_store)
        .arg("init")
        .assert()
        .success();

    assert!(home.join(".dalo/config.toml").is_file());
    assert!(!legacy_store.exists());
}

#[test]
fn doctor_json_should_report_missing_store_without_creating_it() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("missing-store");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"code\": \"store_missing\""));

    assert!(!store.exists());
}

#[test]
fn doctor_json_should_report_initialized_store() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"code\": \"store_exists\""))
        .stdout(predicate::str::contains("\"code\": \"config_ok\""))
        .stdout(predicate::str::contains("\"code\": \"lock_ok\""));
}

#[test]
fn relative_store_path_should_create_absolute_owned_symlink_and_clean_doctor() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .current_dir(temp_dir.path())
        .args(["--store", "store", "init"])
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .current_dir(temp_dir.path())
        .args(["--store", "store", "target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    create_unmanaged_skill(&target, "review");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .current_dir(temp_dir.path())
        .args(["--store", "store", "adopt", "--replace", "review"])
        .assert()
        .success();

    let link_target = std::fs::read_link(target.join("review")).expect("link should be readable");
    assert!(link_target.is_absolute());
    assert_eq!(
        store::comparable_path(&link_target),
        store::comparable_path(&store.join("local/skills/review"))
    );

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .current_dir(temp_dir.path())
        .args(["--store", "store", "--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"code\": \"foreign_owned_symlink\"").not());
}

#[test]
fn doctor_json_should_report_broken_owned_symlink() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    std::fs::remove_dir_all(store.join("local/skills/review"))
        .expect("local skill should be removed");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"code\": \"broken_owned_symlink\"",
        ));
}

#[test]
fn doctor_json_should_report_foreign_owned_symlink() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let foreign = temp_dir.path().join("foreign");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    std::fs::create_dir_all(&foreign).expect("foreign target should be created");
    std::fs::remove_file(target.join("review")).expect("owned symlink should be removed");
    std::os::unix::fs::symlink(&foreign, target.join("review"))
        .expect("foreign symlink should be created");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"code\": \"foreign_owned_symlink\"",
        ));
}

#[test]
fn doctor_json_should_report_unmanaged_same_name_blocker() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"code\": \"unmanaged_same_name_blocker\"",
        ));
}

#[test]
fn status_json_should_report_local_skill_as_active() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let skill_dir = store.join("local/skills/review");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
fn sync_yes_should_not_replace_unmanaged_real_directory() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--yes", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("conflict"));

    assert!(
        !std::fs::symlink_metadata(target.join("review"))
            .expect("unmanaged skill should remain")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(target.join("review/SKILL.md"))
            .expect("unmanaged content should remain"),
        "# review\n"
    );
}

#[test]
fn sync_should_report_existing_on_second_run() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    Command::cargo_bin("dalo")
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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    approve_source(&store, "company");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();

    Command::cargo_bin("dalo")
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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    std::fs::remove_dir_all(store.join("local/skills/review"))
        .expect("local skill should be removed");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(store.join("lock.toml"), "schema_version = 999\n")
        .expect("lock should be overwritten");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
fn status_json_should_report_unmanaged_target_skills() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"unmanaged_skills\""))
        .stdout(predicate::str::contains("\"id\": \"review\""));
}

#[test]
fn status_should_report_actionable_error_for_corrupt_state() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(store.join("state.toml"), "schema_version = ")
        .expect("state should be corrupted");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("status")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("run `dalo init`"));
}

#[test]
fn init_should_repair_corrupt_state_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(store.join("state.toml"), "schema_version = ")
        .expect("state should be corrupted");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("repaired"))
        .stdout(predicate::str::contains("state.toml"));

    assert!(
        std::fs::read_dir(&store)
            .expect("store dir should be readable")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("state.toml.corrupt-"))
    );
    let state =
        store::read_state(&store::StorePaths::new(store)).expect("state should be repaired");
    assert!(state.targets.is_empty());
}

#[test]
fn adopt_should_copy_unmanaged_skill_without_replacing_by_default() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("copied"))
        .stdout(predicate::str::contains("replacement: skipped"));

    assert!(store.join("local/skills/review/SKILL.md").is_file());
    assert!(
        !std::fs::symlink_metadata(target.join("review"))
            .expect("original should remain")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn adopt_should_resolve_slot_when_cwd_contains_same_named_decoy_directory() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let project = temp_dir.path().join("project");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    std::fs::create_dir_all(project.join("review")).expect("decoy dir should be created");
    std::fs::write(project.join("review/SKILL.md"), "# Decoy\n").expect("decoy should be written");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .current_dir(&project)
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("copied"));

    assert_eq!(
        std::fs::read_to_string(store.join("local/skills/review/SKILL.md"))
            .expect("adopted skill should be readable"),
        "# review\n"
    );
    assert_eq!(
        std::fs::read_to_string(project.join("review/SKILL.md")).expect("decoy should be readable"),
        "# Decoy\n"
    );
}

#[test]
fn adopt_should_accept_explicit_relative_path_selector() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .current_dir(&target)
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "./review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("copied"));

    assert!(store.join("local/skills/review/SKILL.md").is_file());
}

#[test]
fn adopt_yes_should_not_replace_original_without_replace() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--yes", "adopt", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: skipped"));

    assert!(store.join("local/skills/review/SKILL.md").is_file());
    assert!(
        !std::fs::symlink_metadata(target.join("review"))
            .expect("original should remain")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn adopt_replace_should_replace_original_with_owned_symlink_without_committing() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: replaced"));

    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("replacement should exist")
            .file_type()
            .is_symlink()
    );
    assert!(!git_command_succeeds(
        &store.join("local"),
        &["rev-parse", "HEAD"]
    ));
}

#[test]
fn adopt_then_adopt_replace_should_complete_the_two_step_replacement() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");

    // Step 1: copy only (no --replace).
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: skipped"));

    // Step 2: replace, reusing the copy from step 1 (previously failed).
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: replaced"));

    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("replacement should exist")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn adopt_replace_should_refuse_when_local_destination_is_an_unrelated_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    // A pre-existing, UNRELATED local skill with the same slot name (different body).
    let local = store.join("local/skills/review");
    std::fs::create_dir_all(&local).expect("local skill dir should be created");
    std::fs::write(local.join("SKILL.md"), "# pre-existing local\n")
        .expect("local skill should be written");
    // Unmanaged target skill with different content (create writes "# review\n").
    create_unmanaged_skill(&target, "review");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    // The unmanaged content must be preserved: still a real dir with its own body.
    assert!(
        !std::fs::symlink_metadata(target.join("review"))
            .expect("target should remain")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(target.join("review/SKILL.md")).expect("content remains"),
        "# review\n"
    );
}

#[test]
fn adopt_replace_should_not_replace_local_marker_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review.local");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review.local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: protected"));

    assert!(store.join("local/skills/review.local/SKILL.md").is_file());
    assert!(
        !std::fs::symlink_metadata(target.join("review.local"))
            .expect("local marker should remain real")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn adopt_replace_should_refuse_replacement_for_kept_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "keep", "review"])
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: protected"));
}

#[test]
fn adopt_replace_should_keep_kept_skill_directory_as_real_entry() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "keep", "review"])
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success();

    assert!(
        !std::fs::symlink_metadata(target.join("review"))
            .expect("kept skill should remain")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn adopt_replace_should_preserve_kept_skill_contents() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "keep", "review"])
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(target.join("review/SKILL.md"))
            .expect("kept skill should remain readable"),
        "# review\n"
    );
}

#[test]
fn adopt_replace_should_preserve_original_contents_via_symlink_when_not_protected() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(target.join("review/SKILL.md"))
            .expect("adopted skill should still resolve through the symlink"),
        "# review\n"
    );
}

#[test]
fn adopt_should_fail_for_path_outside_materialization_dirs() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let outside = temp_dir.path().join("outside");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&outside, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace"])
        .arg(outside.join("review"))
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("was not found"));
}

#[test]
fn adopt_should_not_touch_path_outside_materialization_dirs() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let outside = temp_dir.path().join("outside");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&outside, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace"])
        .arg(outside.join("review"))
        .assert()
        .failure();

    assert!(outside.join("review/SKILL.md").is_file());
}

#[test]
fn adopted_skill_should_show_as_local_override_over_team_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    setup_store_with_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    approve_source(&store, "company");
    create_unmanaged_skill(&target, "team");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "team"])
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"source_ref\": \"local:team\""))
        .stdout(predicate::str::contains("\"local_override\": true"));
}

#[test]
fn resolve_list_should_report_unmanaged_skills() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unmanaged skills:"))
        .stdout(predicate::str::contains("review"));
}

#[test]
fn resolve_adopt_yes_should_copy_only_until_replace_is_explicit() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--yes", "resolve", "adopt", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: skipped"));
    assert!(
        !std::fs::symlink_metadata(target.join("review"))
            .expect("original should remain after --yes")
            .file_type()
            .is_symlink()
    );

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "adopt", "--replace", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: replaced"));
    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("replacement should exist")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn resolve_keep_should_protect_unmanaged_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "keep", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("protected"));
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"protected\": true"));
}

#[test]
fn resolve_keep_should_resolve_slot_when_cwd_contains_same_named_decoy_directory() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let project = temp_dir.path().join("project");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    std::fs::create_dir_all(project.join("review")).expect("decoy dir should be created");
    std::fs::write(project.join("review/SKILL.md"), "# Decoy\n").expect("decoy should be written");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .current_dir(&project)
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "keep", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("protected"));

    let state =
        store::read_state(&store::StorePaths::new(store)).expect("state should be readable");
    assert_eq!(state.protected_skills.len(), 1);
    assert_eq!(
        store::comparable_path(&state.protected_skills[0].path),
        store::comparable_path(&target.join("review"))
    );
    assert_ne!(
        store::comparable_path(&state.protected_skills[0].path),
        store::comparable_path(&project.join("review"))
    );
}

#[test]
fn resolve_keep_dry_run_should_not_write_state() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    let state_before = std::fs::read(store.join("state.toml")).expect("state should be readable");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--dry-run", "resolve", "keep", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("planned"));

    assert_eq!(
        std::fs::read(store.join("state.toml")).expect("state should be readable"),
        state_before
    );
}

#[test]
fn resolve_remove_owned_should_remove_only_recorded_symlink() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "remove-owned", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    assert!(!target.join("review").exists());
}

#[test]
fn resolve_remove_owned_yes_should_not_remove_real_entry() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    create_unmanaged_skill(&target, "review");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
        .assert()
        .success();
    std::fs::remove_file(target.join("review")).expect("owned symlink should be removed");
    create_unmanaged_skill(&target, "review");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["--yes", "resolve", "remove-owned", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("blocked_real_entry"));

    assert!(target.join("review/SKILL.md").is_file());
}

#[test]
fn doctor_suggested_remove_owned_should_clear_real_entry_record() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    std::fs::remove_file(target.join("review")).expect("owned symlink should be removed");
    std::fs::create_dir_all(target.join("review")).expect("real entry should be created");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"code\": \"owned_path_real_entry\"",
        ))
        .stdout(predicate::str::contains(
            "\"next_command\": \"dalo resolve remove-owned review\"",
        ));

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["resolve", "remove-owned", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("blocked_real_entry"));

    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: DoctorReportSchema =
        serde_json::from_slice(&output).expect("doctor JSON should match the doctor schema");

    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.code != "owned_path_real_entry")
    );
    assert!(target.join("review").is_dir());
}

#[test]
fn sync_should_remove_owned_symlink_after_source_is_removed_from_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    approve_source(&store, "company");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    Command::cargo_bin("dalo")
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
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    assert!(!target.join("team").exists());
}

#[test]
fn sync_should_preserve_owned_symlink_when_source_scan_is_degraded() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_skill_and_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("review link should exist")
            .file_type()
            .is_symlink()
    );

    let local_source = store.join("local");
    let original_mode = std::fs::metadata(&local_source)
        .expect("local source metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    std::fs::set_permissions(&local_source, std::fs::Permissions::from_mode(0o000))
        .expect("local source permissions should be changed");

    let renamed_source = store.join("local-unavailable");
    let used_rename_fallback = if std::fs::read_dir(&local_source).is_ok() {
        std::fs::set_permissions(
            &local_source,
            std::fs::Permissions::from_mode(original_mode),
        )
        .expect("local source permissions should be restored before fallback");
        std::fs::rename(&local_source, &renamed_source)
            .expect("local source should be renamed for root-safe fallback");
        true
    } else {
        false
    };

    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("scan degraded"))
        .get_output()
        .stdout
        .clone();

    if used_rename_fallback {
        std::fs::rename(&renamed_source, &local_source)
            .expect("local source should be restored after fallback");
    } else {
        std::fs::set_permissions(
            &local_source,
            std::fs::Permissions::from_mode(original_mode),
        )
        .expect("local source permissions should be restored");
    }

    assert!(
        String::from_utf8(output)
            .expect("sync output should be utf8")
            .contains("preserving recorded owned link")
    );
    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("review link should survive degraded sync")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn source_add_should_clone_team_source_into_store() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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
fn source_add_dry_run_should_not_clone_or_write_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--dry-run", "source", "add", "company"])
        .arg(&repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("would add source company"));

    assert!(!store.join("sources/company/checkout").exists());
}

#[test]
fn source_add_should_approve_added_source() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    assert!(
        std::fs::symlink_metadata(target.join("team"))
            .expect("team skill should be linked")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn source_priority_should_update_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .args(["source", "priority", "company", "3"])
        .assert()
        .success()
        .stdout(predicate::str::contains("priority=3"));
}

#[test]
fn source_priority_should_refuse_to_move_local_source() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "priority", "local", "5"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("local source"));
}

#[test]
fn sync_should_block_dirty_team_source() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    Command::cargo_bin("dalo")
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
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

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

#[test]
fn sync_should_not_link_unapproved_team_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    setup_store_with_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    set_source_untrusted(&store, "company");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    assert!(!target.join("team").exists());
}

#[test]
fn sync_should_not_refresh_team_source_without_track_policy() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo);
    setup_store_with_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add", "company"])
        .arg(&repo)
        .assert()
        .success();
    remove_source_update_policy(&store, "company");
    std::fs::write(repo.join("skills/team/SKILL.md"), "# Team v2\n")
        .expect("upstream skill should be updated");
    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "update team",
            "-q",
        ],
    );

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(store.join("sources/company/checkout/skills/team/SKILL.md"))
            .expect("checkout skill should be readable"),
        "# Team\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("team/SKILL.md"))
            .expect("materialized skill should be readable"),
        "# Team\n"
    );
}

#[test]
fn status_should_show_all_pending_approval_candidates_for_same_slot() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo_a = temp_dir.path().join("team-a-repo");
    let repo_b = temp_dir.path().join("team-b-repo");
    create_git_skill_repo(&repo_a);
    create_git_skill_repo(&repo_b);
    setup_store_with_target(&store, &target);
    for (source_id, repo) in [("team-a", &repo_a), ("team-b", &repo_b)] {
        Command::cargo_bin("dalo")
            .expect("binary should build")
            .args(["--store"])
            .arg(&store)
            .args(["source", "add", source_id])
            .arg(repo)
            .assert()
            .success();
        set_source_untrusted(&store, source_id);
    }

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("pending approval:"))
        .stdout(predicate::str::contains("team -> team-a:team"))
        .stdout(predicate::str::contains("team -> team-b:team"));
}

#[test]
fn sync_should_not_block_on_dirty_local_source() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    setup_store_with_target(&store, &target);
    let local = store.join("local");
    let skill_dir = local.join("skills/review");
    std::fs::create_dir_all(&skill_dir).expect("local skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");
    run_git(&local, &["add", "."]);
    run_git(
        &local,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "add review",
            "-q",
        ],
    );
    // Modify the committed skill so the local source is dirty in the same Git sense
    // that blocks a Team source.
    std::fs::write(skill_dir.join("SKILL.md"), "# Review dirty\n")
        .expect("local skill should be dirtied");
    let mut command = Command::cargo_bin("dalo").expect("binary should build");

    command
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
}

#[test]
fn status_json_schema_should_model_instruction_packs_and_blocked_skills() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let agents = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    for (pack, body) in [
        ("style", "topics: formatting\n\nUse tabs.\n"),
        ("format", "topics: formatting\n\nWrap at 100.\n"),
    ] {
        std::fs::write(
            store.join("local/instructions").join(format!("{pack}.md")),
            body,
        )
        .expect("pack should be written");
        Command::cargo_bin("dalo")
            .expect("binary should build")
            .args(["--store"])
            .arg(&store)
            .args(["instructions", "enable", pack])
            .arg(&agents)
            .assert()
            .success();
    }

    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: StatusReportSchema =
        serde_json::from_slice(&output).expect("status JSON should match the status schema");

    assert_eq!(report.instruction_packs.len(), 2);
    assert!(report.instruction_packs.iter().all(|pack| pack.enabled));
    assert!(
        report
            .instruction_packs
            .iter()
            .any(|pack| pack.id == "style" && pack.source_id == "local")
    );
    assert_eq!(report.instruction_pack_overlaps.len(), 1);
    assert_eq!(
        report.instruction_pack_overlaps[0].topics,
        vec!["formatting".to_owned()]
    );
    assert!(
        report.instruction_pack_overlaps[0]
            .packs
            .contains(&"local:style".to_owned())
    );
    // blocked_skills is modeled (empty here); referencing its fields guards the schema.
    assert!(
        report
            .resolution
            .blocked_skills
            .iter()
            .all(|blocked| !blocked.requirement.is_empty() && !blocked.reason.is_empty())
    );
}

#[test]
fn source_inspect_json_should_model_catalog_candidates() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("catalog-repo");
    create_git_catalog_repo(&repo);
    setup_store_with_target(&store, &target);

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add-catalog", "marketing"])
        .arg(&repo)
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "select", "marketing", "copy-editing"])
        .assert()
        .success();

    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "source", "inspect", "marketing"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: CatalogInspectSchema =
        serde_json::from_slice(&output).expect("inspect JSON should match the catalog schema");

    assert_eq!(report.source_id, "marketing");
    assert!(
        report
            .candidates
            .iter()
            .any(|candidate| candidate.slot_name == "copy-editing" && candidate.selected)
    );
    assert!(
        report
            .candidates
            .iter()
            .any(|candidate| candidate.slot_name == "launch-copy" && !candidate.selected)
    );
}

// Mirror structs for the machine-output schema. They intentionally live in the test
// crate so production types are not forced to derive `Deserialize`. Deserialization
// fails if a named field is renamed, removed, or changes type, which is the schema
// guarantee the substring assertions could not provide. Only fields under test are
// modeled; serde ignores the rest of the payload.
#[derive(serde::Deserialize)]
struct StatusReportSchema {
    resolution: ResolutionSchema,
    lock: LockStatusSchema,
    instruction_packs: Vec<InstructionPackSchema>,
    instruction_pack_overlaps: Vec<TopicOverlapSchema>,
}

#[derive(serde::Deserialize)]
struct ResolutionSchema {
    active_skills: Vec<ActiveSkillSchema>,
    blocked_skills: Vec<BlockedSkillSchema>,
}

#[derive(serde::Deserialize)]
struct ActiveSkillSchema {
    source_ref: String,
}

#[derive(serde::Deserialize)]
struct BlockedSkillSchema {
    requirement: String,
    reason: String,
}

#[derive(serde::Deserialize)]
struct InstructionPackSchema {
    id: String,
    source_id: String,
    enabled: bool,
}

#[derive(serde::Deserialize)]
struct TopicOverlapSchema {
    packs: [String; 2],
    topics: Vec<String>,
}

#[derive(serde::Deserialize)]
struct CatalogInspectSchema {
    source_id: String,
    candidates: Vec<CatalogCandidateSchema>,
}

#[derive(serde::Deserialize)]
struct CatalogCandidateSchema {
    slot_name: String,
    selected: bool,
}

#[derive(serde::Deserialize)]
struct LockStatusSchema {
    schema_version: u32,
}

#[derive(serde::Deserialize)]
struct DoctorReportSchema {
    findings: Vec<DoctorFindingSchema>,
}

#[derive(serde::Deserialize)]
struct DoctorFindingSchema {
    severity: String,
    code: String,
}

#[test]
fn status_json_should_deserialize_into_status_schema_with_active_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let skill_dir = store.join("local/skills/review");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");
    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: StatusReportSchema =
        serde_json::from_slice(&output).expect("status JSON should match the status schema");

    assert_eq!(
        report.resolution.active_skills[0].source_ref,
        "local:review"
    );
}

#[test]
fn status_json_should_expose_lock_schema_version_field() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: StatusReportSchema =
        serde_json::from_slice(&output).expect("status JSON should match the status schema");

    assert_eq!(report.lock.schema_version, 1);
}

#[test]
fn doctor_json_should_deserialize_into_doctor_schema_with_store_exists_finding() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let output = Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: DoctorReportSchema =
        serde_json::from_slice(&output).expect("doctor JSON should match the doctor schema");

    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "store_exists" && finding.severity == "ok")
    );
}

fn setup_store_with_skill_and_target(store: &std::path::Path, target: &std::path::Path) {
    setup_store_with_target(store, target);
    let skill_dir = store.join("local/skills/review");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").expect("skill should be written");
}

#[test]
fn catalog_select_should_materialize_only_selected_skills() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("catalog-repo");
    create_git_catalog_repo(&repo);
    setup_store_with_target(&store, &target);

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add-catalog", "marketing"])
        .arg(&repo)
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "inspect", "marketing"])
        .assert()
        .success()
        .stdout(predicate::str::contains("copy-editing"))
        .stdout(predicate::str::contains("launch-copy"));

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "select", "marketing", "copy-editing"])
        .assert()
        .success();
    let source_lock =
        std::fs::read_to_string(store.join("source-lock.toml")).expect("source lock readable");
    assert!(source_lock.contains("selected = [\"skills/copy-editing\"]"));
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    // Only the selected catalog skill is materialized; the unselected one is not.
    assert!(
        std::fs::symlink_metadata(target.join("copy-editing"))
            .expect("selected skill should be linked")
            .file_type()
            .is_symlink()
    );
    assert!(!target.join("launch-copy").exists());
}

#[test]
fn catalog_select_dry_run_should_not_write_config_or_source_lock() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("catalog-repo");
    create_git_catalog_repo(&repo);
    setup_store_with_target(&store, &target);
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add-catalog", "marketing"])
        .arg(&repo)
        .assert()
        .success();
    let config_before = std::fs::read(store.join("config.toml")).expect("config readable");
    let source_lock_before =
        std::fs::read(store.join("source-lock.toml")).expect("source lock readable");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--dry-run", "source", "select", "marketing", "copy-editing"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would select"));

    assert_eq!(
        std::fs::read(store.join("config.toml")).expect("config readable"),
        config_before
    );
    assert_eq!(
        std::fs::read(store.join("source-lock.toml")).expect("source lock readable"),
        source_lock_before
    );
}

#[test]
fn catalog_select_should_support_path_fallback_for_duplicate_slots() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("catalog-repo");
    create_git_catalog_repo_with_duplicate_slots(&repo);
    setup_store_with_target(&store, &target);

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add-catalog", "catalog"])
        .arg(&repo)
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "select", "catalog", "shared"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous"));
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "select", "catalog", "skills/a"])
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    let linked = std::fs::read_link(target.join("shared")).expect("selected skill should link");
    assert!(linked.ends_with("sources/catalog/checkout/skills/a"));
    let source_lock =
        std::fs::read_to_string(store.join("source-lock.toml")).expect("source lock readable");
    assert!(source_lock.contains("selected = [\"skills/a\"]"));
}

#[test]
fn catalog_refresh_check_should_report_upstream_drift() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("catalog-repo");
    create_git_catalog_repo(&repo);
    setup_store_with_target(&store, &target);

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "add-catalog", "marketing"])
        .arg(&repo)
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "select", "marketing", "copy-editing"])
        .assert()
        .success();

    // Upstream drift: change the selected skill and add a new unselected one.
    std::fs::write(
        repo.join("skills/copy-editing/SKILL.md"),
        "# copy-editing v2\n",
    )
    .expect("skill rewritten");
    std::fs::create_dir_all(repo.join("skills/seo")).expect("dir created");
    std::fs::write(repo.join("skills/seo/SKILL.md"), "# seo\n").expect("skill written");
    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "update",
            "-q",
        ],
    );

    // The read-only check reports the changed selection and the new offering
    // without advancing the pin.
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["source", "refresh", "marketing", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("selected_changed"))
        .stdout(predicate::str::contains("new_available"));
}

#[test]
fn instructions_enable_disable_should_manage_block_idempotently() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target_file = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();

    // Author a local instruction pack and seed the target with user content.
    std::fs::write(
        store.join("local/instructions/house-style.md"),
        "version: 1.0\n\nUse tabs, not spaces.\n",
    )
    .expect("pack should be written");
    std::fs::write(&target_file, "# Project\n\nUser notes.\n").expect("target should be written");

    let enable = || {
        Command::cargo_bin("dalo")
            .expect("binary should build")
            .args(["--store"])
            .arg(&store)
            .args(["instructions", "enable", "house-style"])
            .arg(&target_file)
            .assert()
            .success();
    };

    enable();
    let after_enable = std::fs::read_to_string(&target_file).expect("target readable");
    assert!(after_enable.contains("# Project"));
    assert!(after_enable.contains("User notes."));
    assert!(after_enable.contains("Use tabs, not spaces."));
    assert!(after_enable.contains("<!-- dalo:start house-style -->"));

    // Enabling again is idempotent.
    enable();
    let after_second = std::fs::read_to_string(&target_file).expect("target readable");
    assert_eq!(after_enable, after_second);

    // Disabling removes exactly the block and keeps user content.
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["instructions", "disable", "house-style"])
        .arg(&target_file)
        .assert()
        .success();
    let after_disable = std::fs::read_to_string(&target_file).expect("target readable");
    assert!(after_disable.contains("# Project"));
    assert!(after_disable.contains("User notes."));
    assert!(!after_disable.contains("dalo:start"));
}

#[test]
fn instructions_enable_dry_run_should_not_write_target_or_lock() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target_file = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(
        store.join("local/instructions/house-style.md"),
        "version: 1.0\n\nUse tabs.\n",
    )
    .expect("pack should be written");
    std::fs::write(&target_file, "# Project\n").expect("target should be written");
    let target_before = std::fs::read(&target_file).expect("target should be readable");
    let lock_before = std::fs::read(store.join("lock.toml")).expect("lock should be readable");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--dry-run", "instructions", "enable", "house-style"])
        .arg(&target_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("would enable"));

    assert_eq!(
        std::fs::read(&target_file).expect("target should be readable"),
        target_before
    );
    assert_eq!(
        std::fs::read(store.join("lock.toml")).expect("lock should be readable"),
        lock_before
    );
}

#[test]
fn instructions_disable_dry_run_should_not_write_target_or_lock() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target_file = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(
        store.join("local/instructions/house-style.md"),
        "version: 1.0\n\nUse tabs.\n",
    )
    .expect("pack should be written");
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["instructions", "enable", "house-style"])
        .arg(&target_file)
        .assert()
        .success();
    let target_before = std::fs::read(&target_file).expect("target should be readable");
    let lock_before = std::fs::read(store.join("lock.toml")).expect("lock should be readable");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--dry-run", "instructions", "disable", "house-style"])
        .arg(&target_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("would disable"));

    assert_eq!(
        std::fs::read(&target_file).expect("target should be readable"),
        target_before
    );
    assert_eq!(
        std::fs::read(store.join("lock.toml")).expect("lock should be readable"),
        lock_before
    );
}

#[test]
fn instructions_enable_should_reject_malformed_existing_block() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target_file = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(
        store.join("local/instructions/house-style.md"),
        "version: 1.0\n\nUse tabs.\n",
    )
    .expect("pack should be written");
    let malformed = "# Project\n\n<!-- dalo:start house-style -->\nmissing end\n";
    std::fs::write(&target_file, malformed).expect("target should be written");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["instructions", "enable", "house-style"])
        .arg(&target_file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("malformed instruction block"));

    assert_eq!(
        std::fs::read_to_string(&target_file).expect("target readable"),
        malformed
    );
}

#[test]
fn instructions_enable_should_fail_on_non_utf8_target_without_rewriting() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target_file = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    std::fs::write(
        store.join("local/instructions/house-style.md"),
        "version: 1.0\n\nUse tabs.\n",
    )
    .expect("pack should be written");
    let original = b"# Project\n\nLatin-1 byte: \x96\n";
    std::fs::write(&target_file, original).expect("target should be written");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["instructions", "enable", "house-style"])
        .arg(&target_file)
        .assert()
        .failure()
        .code(4);

    assert_eq!(
        std::fs::read(&target_file).expect("target bytes should be readable"),
        original
    );
}

#[test]
fn status_json_should_report_instruction_pack_topic_overlap() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let agents = temp_dir.path().join("AGENTS.md");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("init")
        .assert()
        .success();

    // Two local packs declaring a shared topic.
    std::fs::write(
        store.join("local/instructions/style.md"),
        "topics: formatting\n\nUse tabs.\n",
    )
    .expect("pack should be written");
    std::fs::write(
        store.join("local/instructions/format.md"),
        "topics: formatting\n\nWrap at 100.\n",
    )
    .expect("pack should be written");

    for pack in ["style", "format"] {
        Command::cargo_bin("dalo")
            .expect("binary should build")
            .args(["--store"])
            .arg(&store)
            .args(["instructions", "enable", pack])
            .arg(&agents)
            .assert()
            .success();
    }

    // status --json surfaces the advisory overlap naming both pack refs.
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("instruction_pack_overlaps"))
        .stdout(predicate::str::contains("local:style"))
        .stdout(predicate::str::contains("local:format"));
}

fn setup_store_with_target(store: &std::path::Path, target: &std::path::Path) {
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .args(["target", "link", "generic"])
        .arg(target)
        .assert()
        .success();
}

fn create_unmanaged_skill(target: &std::path::Path, slot_name: &str) {
    let skill_dir = target.join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("unmanaged skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), format!("# {slot_name}\n"))
        .expect("unmanaged skill should be written");
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

fn set_source_untrusted(store: &std::path::Path, source_id: &str) {
    let config_path = store.join("config.toml");
    let content = std::fs::read_to_string(&config_path).expect("config should be readable");
    let mut in_source = false;
    let mut out = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[sources]]" {
            in_source = false;
        } else if let Some(rest) = trimmed.strip_prefix("id = ") {
            in_source = rest.trim().trim_matches('"') == source_id;
        }
        if in_source && trimmed == "trusted = true" {
            out.push_str("trusted = false\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    std::fs::write(&config_path, out).expect("config should be written");
}

fn remove_source_update_policy(store: &std::path::Path, source_id: &str) {
    let config_path = store.join("config.toml");
    let content = std::fs::read_to_string(&config_path).expect("config should be readable");
    let mut in_source = false;
    let mut out = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[sources]]" {
            in_source = false;
        } else if let Some(rest) = trimmed.strip_prefix("id = ") {
            in_source = rest.trim().trim_matches('"') == source_id;
        }
        if in_source && trimmed.starts_with("update_policy = ") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    std::fs::write(&config_path, out).expect("config should be written");
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

fn create_git_catalog_repo(repo: &std::path::Path) {
    for slot in ["copy-editing", "launch-copy"] {
        std::fs::create_dir_all(repo.join("skills").join(slot)).expect("repo dirs created");
        std::fs::write(
            repo.join("skills").join(slot).join("SKILL.md"),
            format!("# {slot}\n"),
        )
        .expect("skill written");
    }
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

fn create_git_catalog_repo_with_duplicate_slots(repo: &std::path::Path) {
    for folder in ["a", "b"] {
        std::fs::create_dir_all(repo.join("skills").join(folder)).expect("repo dirs created");
        std::fs::write(
            repo.join("skills").join(folder).join("SKILL.md"),
            "---\nname: shared\n---\n# Shared\n",
        )
        .expect("skill written");
    }
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

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("git should run");
    assert!(status.success());
}

fn git_command_succeeds(repo: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git should run")
        .success()
}
