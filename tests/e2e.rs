use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn e2e_local_only_sync_quickstart() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    init_store(&store);
    link_target(&store, &target);
    create_local_skill(&store, "review", "# Review\n");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("applied"));

    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("skill should be linked")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn e2e_team_source_sync_from_local_git_repo() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo, "team", "# Team\n");
    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &repo);
    approve_source(&store, "company");

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
fn e2e_unmanaged_conflict_does_not_overwrite_real_folder() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    init_store(&store);
    link_target(&store, &target);
    create_local_skill(&store, "review", "# Managed Review\n");
    create_unmanaged_skill(&target, "review", "# Unmanaged Review\n");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("conflict"));

    assert_eq!(
        std::fs::read_to_string(target.join("review/SKILL.md"))
            .expect("unmanaged skill should remain readable"),
        "# Unmanaged Review\n"
    );
}

#[test]
fn e2e_adoption_flow_copies_then_replaces_on_yes() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    init_store(&store);
    link_target(&store, &target);
    create_unmanaged_skill(&target, "review", "# Review\n");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .args(["--yes", "adopt", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("replacement: replaced"));

    assert!(store.join("local/skills/review/SKILL.md").is_file());
    assert!(
        std::fs::symlink_metadata(target.join("review"))
            .expect("adopted target should be a symlink")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn e2e_multi_source_shadowing_is_recorded_in_lock() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo_a = temp_dir.path().join("team-a");
    let repo_b = temp_dir.path().join("team-b");
    create_git_skill_repo(&repo_a, "team", "# Team A\n");
    create_git_skill_repo(&repo_b, "team", "# Team B\n");
    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "a", &repo_a);
    add_source(&store, "b", &repo_b);
    approve_source(&store, "a");
    approve_source(&store, "b");

    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    let lock = std::fs::read_to_string(store.join("lock.toml")).expect("lock should be readable");
    assert!(lock.contains("source_ref = \"a:team\""));
    assert!(lock.contains("source_ref = \"b:team\""));
    assert!(lock.contains("reason = \"shadowed\""));
}

#[test]
fn e2e_dirty_team_source_blocks_sync() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo(&repo, "team", "# Team\n");
    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &repo);
    approve_source(&store, "company");
    std::fs::write(
        store.join("sources/company/checkout/skills/team/SKILL.md"),
        "# Dirty\n",
    )
    .expect("checkout should be dirtied");

    Command::cargo_bin("dalo")
        .expect("binary should build")
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

fn init_store(store: &std::path::Path) {
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .arg("init")
        .assert()
        .success();
}

fn link_target(store: &std::path::Path, target: &std::path::Path) {
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .args(["target", "link", "generic"])
        .arg(target)
        .assert()
        .success();
}

fn add_source(store: &std::path::Path, source: &str, repo: &std::path::Path) {
    Command::cargo_bin("dalo")
        .expect("binary should build")
        .args(["--store"])
        .arg(store)
        .args(["source", "add", source])
        .arg(repo)
        .assert()
        .success();
}

fn create_local_skill(store: &std::path::Path, slot_name: &str, body: &str) {
    let skill_dir = store.join("local/skills").join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("local skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("local skill should be written");
}

fn create_unmanaged_skill(target: &std::path::Path, slot_name: &str, body: &str) {
    let skill_dir = target.join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("unmanaged skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("unmanaged skill should be written");
}

fn create_git_skill_repo(repo: &std::path::Path, slot_name: &str, body: &str) {
    let skill_dir = repo.join("skills").join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("repo skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("repo skill should be written");
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
    let mut content = std::fs::read_to_string(store.join("approvals.toml"))
        .expect("approvals should be readable");
    content.push_str(&format!(
        "\n[[approvals]]\nscope = \"source\"\nvalue = \"{source}\"\n"
    ));
    std::fs::write(store.join("approvals.toml"), content)
        .expect("source approval should be written");
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("git should run");
    assert!(status.success());
}
