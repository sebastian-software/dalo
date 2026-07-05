use predicates::prelude::*;

mod common;

use common::{
    add_source, approve_source, create_git_skill_repo_with_skill, create_local_skill,
    create_unmanaged_skill_with_body, dalo_command, init_store, link_target, read_user_lock,
};

#[test]
fn e2e_local_only_sync_quickstart() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    init_store(&store);
    link_target(&store, &target);
    create_local_skill(&store, "review", "# Review\n");

    dalo_command()
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
    create_git_skill_repo_with_skill(&repo, "team", "# Team\n");
    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &repo);
    approve_source(&store, "company");

    dalo_command()
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
    create_unmanaged_skill_with_body(&target, "review", "# Unmanaged Review\n");

    dalo_command()
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
fn e2e_adoption_flow_copies_then_replaces_on_replace() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    init_store(&store);
    link_target(&store, &target);
    create_unmanaged_skill_with_body(&target, "review", "# Review\n");

    dalo_command()
        .args(["--store"])
        .arg(&store)
        .args(["adopt", "--replace", "review"])
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
    create_git_skill_repo_with_skill(&repo_a, "team", "# Team A\n");
    create_git_skill_repo_with_skill(&repo_b, "team", "# Team B\n");
    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "a", &repo_a);
    add_source(&store, "b", &repo_b);
    approve_source(&store, "a");
    approve_source(&store, "b");

    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    let lock = read_user_lock(&store);
    assert!(
        lock.active_skills
            .iter()
            .any(|skill| skill.source_ref == "a:team")
    );
    assert!(lock.unlinked_skills.iter().any(|skill| {
        skill.source_ref == "b:team" && skill.reason.as_deref() == Some("shadowed")
    }));
}

#[test]
fn e2e_dirty_team_source_blocks_sync() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let repo = temp_dir.path().join("team-repo");
    create_git_skill_repo_with_skill(&repo, "team", "# Team\n");
    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &repo);
    approve_source(&store, "company");
    std::fs::write(
        store.join("sources/company/checkout/skills/team/SKILL.md"),
        "# Dirty\n",
    )
    .expect("checkout should be dirtied");

    dalo_command()
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
