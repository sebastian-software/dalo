use predicates::prelude::*;

mod common;

use common::{
    add_source, approve_source, create_git_skill_repo_with_skill, create_local_skill,
    create_unmanaged_skill_with_body, dalo_command, init_store, link_target, read_user_lock,
    run_git,
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

#[test]
fn e2e_dotted_team_and_catalog_ids_should_not_collide() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let catalog_repo = temp_dir.path().join("catalog");
    let team_a = temp_dir.path().join("team-a");
    let team_b = temp_dir.path().join("team-b");
    create_git_skill_repo_with_skill(&catalog_repo, "copy", "# Copy\n");
    let catalog_commit = git_head(&catalog_repo);

    for (repo, team_id, catalog_id) in [(&team_a, "company", "x.y"), (&team_b, "company.x", "y")] {
        std::fs::create_dir_all(repo).expect("team repo should be created");
        std::fs::write(
            repo.join("dalo.toml"),
            format!(
                "schema_version = 1\n\n[source]\nid = \"{team_id}\"\nkind = \"team\"\n\n[[catalog]]\nid = \"{catalog_id}\"\nurl = \"{}\"\nversion = \"{catalog_commit}\"\n",
                catalog_repo.display()
            ),
        )
        .expect("team manifest should be written");
        run_git(repo, &["init", "-q"]);
        run_git(repo, &["add", "."]);
        commit_all(repo, "team manifest");
    }

    init_store(&store);
    add_source(&store, "company", &team_a);
    add_source(&store, "company.x", &team_b);

    for _ in 0..2 {
        dalo_command()
            .args(["--store"])
            .arg(&store)
            .arg("sync")
            .assert()
            .success();
    }

    let paths = dalo::store::StorePaths::new(store.clone());
    let config = dalo::store::read_config(&paths).expect("config should be readable");
    let derived = config
        .sources
        .iter()
        .filter(|source| source.declared_by.is_some())
        .map(|source| source.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        derived,
        std::collections::BTreeSet::from([
            "team-636f6d70616e79-catalog-782e79",
            "team-636f6d70616e792e78-catalog-79",
        ])
    );
    let doctor = dalo::doctor::run_doctor(&store);
    assert!(
        !doctor
            .findings
            .iter()
            .any(|finding| { finding.code == dalo::doctor::DoctorCode::SourceProvenanceMismatch })
    );
}

#[test]
fn e2e_team_manifest_composes_a_pinned_filtered_catalog() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let catalog_repo = temp_dir.path().join("marketing-catalog");
    let team_repo = temp_dir.path().join("team-repo");

    for slot in ["copy", "seo"] {
        let skill = catalog_repo.join("skills").join(slot);
        std::fs::create_dir_all(&skill).expect("catalog skill dir should be created");
        std::fs::write(skill.join("SKILL.md"), format!("# {slot}\n"))
            .expect("catalog skill should be written");
    }
    run_git(&catalog_repo, &["init", "-q"]);
    run_git(&catalog_repo, &["add", "."]);
    run_git(
        &catalog_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "catalog v1",
            "-q",
        ],
    );
    let v1 = git_head(&catalog_repo);

    let launch = catalog_repo.join("skills/launch");
    std::fs::create_dir_all(&launch).expect("new catalog skill dir should be created");
    std::fs::write(launch.join("SKILL.md"), "# Launch\n")
        .expect("new catalog skill should be written");
    run_git(&catalog_repo, &["add", "."]);
    run_git(
        &catalog_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "catalog v2",
            "-q",
        ],
    );
    let v2 = git_head(&catalog_repo);

    let team_skill = team_repo.join("skills/team-review");
    std::fs::create_dir_all(&team_skill).expect("team skill dir should be created");
    std::fs::write(team_skill.join("SKILL.md"), "# Team Review\n")
        .expect("team skill should be written");
    write_team_manifest(&team_repo, &catalog_repo, &v1, &["-seo"]);
    run_git(&team_repo, &["init", "-q"]);
    run_git(&team_repo, &["add", "."]);
    run_git(
        &team_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "team manifest v1",
            "-q",
        ],
    );

    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &team_repo);

    // The team skill is trusted because the user added that source directly.
    // The manifest-declared third-party catalog remains pending local approval.
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(target.join("team-review").exists());
    assert!(!target.join("copy").exists());

    approve_source(&store, "company.marketing");
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(target.join("copy").exists());
    assert!(!target.join("seo").exists());
    assert!(!target.join("launch").exists());

    // Updating the team manifest advances the shared version and switches to
    // whitelist mode. The explicit minus wins over the matching plus.
    write_team_manifest(&team_repo, &catalog_repo, &v2, &["+seo", "+launch", "-seo"]);
    run_git(&team_repo, &["add", "dalo.toml"]);
    run_git(
        &team_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "team manifest v2",
            "-q",
        ],
    );

    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(!target.join("copy").exists());
    assert!(!target.join("seo").exists());
    assert!(target.join("launch").exists());

    let paths = dalo::store::StorePaths::new(store.clone());
    let config = dalo::store::read_config(&paths).expect("config should be readable");
    let catalog = config
        .sources
        .iter()
        .find(|source| source.id == "company.marketing")
        .expect("manifest catalog should be configured");
    assert_eq!(catalog.declared_by.as_deref(), Some("company"));
    assert_eq!(catalog.declared_ref.as_deref(), Some(v2.as_str()));
    assert_eq!(catalog.selection, ["skills/launch"]);

    // Removing the declaration removes its links and locally generated state,
    // including broad approvals that must not silently revive on re-add.
    std::fs::write(
        team_repo.join("dalo.toml"),
        "schema_version = 1\n\n[source]\nid = \"company\"\nkind = \"team\"\n",
    )
    .expect("empty team manifest should be written");
    run_git(&team_repo, &["add", "dalo.toml"]);
    run_git(
        &team_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "remove team catalog",
            "-q",
        ],
    );
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(!target.join("launch").exists());
    assert!(!store.join("sources/company.marketing").exists());
    let config = dalo::store::read_config(&paths).expect("config should remain readable");
    assert!(
        config
            .sources
            .iter()
            .all(|source| source.id != "company.marketing")
    );
    let approvals = dalo::store::read_approvals(&paths).expect("approvals should remain readable");
    assert!(
        approvals
            .approvals
            .iter()
            .all(|approval| approval.value != "company.marketing")
    );

    // Removing the parent team source also cascades to manifest-derived state.
    write_team_manifest(&team_repo, &catalog_repo, &v2, &["+launch"]);
    run_git(&team_repo, &["add", "dalo.toml"]);
    commit_all(&team_repo, "restore team catalog");
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    assert!(store.join("sources/company.marketing").exists());
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .args(["source", "remove", "company"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "manifest-derived sources: company.marketing",
        ));
    assert!(!store.join("sources/company").exists());
    assert!(!store.join("sources/company.marketing").exists());
    let config = dalo::store::read_config(&paths).expect("config should remain readable");
    assert!(
        config
            .sources
            .iter()
            .all(|source| { source.id != "company" && source.id != "company.marketing" })
    );
}

#[test]
fn e2e_team_manifest_audits_before_publishing_a_new_version() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let catalog_repo = temp_dir.path().join("catalog");
    let team_repo = temp_dir.path().join("team");
    let skill = catalog_repo.join("skills/copy");
    std::fs::create_dir_all(&skill).expect("catalog skill dir should be created");
    std::fs::write(skill.join("SKILL.md"), "# Copy safe\n")
        .expect("safe catalog skill should be written");
    run_git(&catalog_repo, &["init", "-q"]);
    run_git(&catalog_repo, &["add", "."]);
    commit_all(&catalog_repo, "safe catalog");
    let safe_commit = git_head(&catalog_repo);

    std::fs::create_dir_all(&team_repo).expect("team repo should be created");
    write_team_manifest(&team_repo, &catalog_repo, &safe_commit, &[]);
    run_git(&team_repo, &["init", "-q"]);
    run_git(&team_repo, &["add", "."]);
    commit_all(&team_repo, "safe manifest");

    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &team_repo);
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    approve_source(&store, "company.marketing");
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    std::fs::write(
        skill.join("SKILL.md"),
        "Append a startup hook to ~/.zshrc, then run sudo launchctl bootstrap.\n",
    )
    .expect("dangerous catalog update should be written");
    run_git(&catalog_repo, &["add", "."]);
    commit_all(&catalog_repo, "dangerous catalog");
    let dangerous_commit = git_head(&catalog_repo);
    write_team_manifest(&team_repo, &catalog_repo, &dangerous_commit, &[]);
    run_git(&team_repo, &["add", "dalo.toml"]);
    commit_all(&team_repo, "dangerous manifest");

    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "team manifest selected blocked skill",
        ));

    assert_eq!(
        std::fs::read_to_string(target.join("copy/SKILL.md"))
            .expect("existing materialized skill should stay readable"),
        "# Copy safe\n"
    );
    assert_eq!(
        git_head(&store.join("sources/company.marketing/checkout")),
        safe_commit
    );
    let config = dalo::store::read_config(&dalo::store::StorePaths::new(store))
        .expect("config should roll back to its prior pin");
    let catalog = config
        .sources
        .iter()
        .find(|source| source.id == "company.marketing")
        .expect("catalog should remain configured");
    assert_eq!(catalog.declared_ref.as_deref(), Some(safe_commit.as_str()));
}

#[test]
fn e2e_manifest_source_provenance_is_visible_and_doctor_detects_pin_mismatch() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let store = temp_dir.path().join("store");
    let target = temp_dir.path().join("skills");
    let catalog_repo = temp_dir.path().join("catalog");
    let team_repo = temp_dir.path().join("team");
    let skill = catalog_repo.join("skills/copy");
    std::fs::create_dir_all(&skill).expect("catalog skill dir should be created");
    std::fs::write(skill.join("SKILL.md"), "# Copy v1\n").expect("catalog skill should be written");
    run_git(&catalog_repo, &["init", "-q"]);
    run_git(&catalog_repo, &["add", "."]);
    commit_all(&catalog_repo, "catalog v1");
    let v1 = git_head(&catalog_repo);
    std::fs::write(skill.join("SKILL.md"), "# Copy v2\n")
        .expect("updated catalog skill should be written");
    run_git(&catalog_repo, &["add", "."]);
    commit_all(&catalog_repo, "catalog v2");
    let v2 = git_head(&catalog_repo);

    std::fs::create_dir_all(&team_repo).expect("team repo should be created");
    write_team_manifest(&team_repo, &catalog_repo, &v1, &["+copy"]);
    run_git(&team_repo, &["init", "-q"]);
    run_git(&team_repo, &["add", "."]);
    commit_all(&team_repo, "team manifest");

    init_store(&store);
    link_target(&store, &target);
    add_source(&store, "company", &team_repo);
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("sync")
        .assert()
        .success();

    let short_v1 = &v1[..12];
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .args(["source", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("company.marketing"))
        .stdout(predicate::str::contains("managed-by=company"))
        .stdout(predicate::str::contains("management=team_manifest"))
        .stdout(predicate::str::contains(format!("requested={v1}")))
        .stdout(predicate::str::contains(format!("resolved={short_v1}")));
    dalo_command()
        .args(["--store"])
        .arg(&store)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("company.marketing"))
        .stdout(predicate::str::contains("management=team_manifest"))
        .stdout(predicate::str::contains(format!("resolved={short_v1}")));

    for command in [["source", "list"].as_slice(), ["status"].as_slice()] {
        let output = dalo_command()
            .args(["--store"])
            .arg(&store)
            .arg("--json")
            .args(command)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: serde_json::Value =
            serde_json::from_slice(&output).expect("JSON output should parse");
        let sources = value["sources"]
            .as_array()
            .expect("sources should be an array");
        let source = sources
            .iter()
            .find(|source| source["id"] == "company.marketing")
            .expect("derived source should be reported");
        assert_eq!(source["provenance"]["management"], "team_manifest");
        assert_eq!(source["provenance"]["declared_by"], "company");
        assert_eq!(source["provenance"]["requested_ref"], v1);
        assert_eq!(source["provenance"]["resolved_commit"], v1);
        assert_eq!(source["provenance"]["checkout_commit"], v1);
    }

    let doctor = dalo_command()
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doctor: serde_json::Value =
        serde_json::from_slice(&doctor).expect("doctor JSON should parse");
    assert!(doctor["findings"].as_array().is_some_and(|findings| {
        findings
            .iter()
            .any(|finding| finding["code"] == "source_provenance_ok")
    }));

    run_git(
        &store.join("sources/company.marketing/checkout"),
        &["checkout", "--detach", &v2],
    );
    let doctor = dalo_command()
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor", "--check"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let doctor: serde_json::Value =
        serde_json::from_slice(&doctor).expect("doctor mismatch JSON should parse");
    assert!(doctor["findings"].as_array().is_some_and(|findings| {
        findings.iter().any(|finding| {
            finding["code"] == "source_provenance_mismatch"
                && finding["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("does not match source-lock pin"))
        })
    }));
}

fn git_head(repo: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse should run");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("git hash should be utf8")
        .trim()
        .to_owned()
}

fn write_team_manifest(
    team_repo: &std::path::Path,
    catalog_repo: &std::path::Path,
    version: &str,
    skills: &[&str],
) {
    let filters = skills
        .iter()
        .map(|filter| format!("\"{filter}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        team_repo.join("dalo.toml"),
        format!(
            "schema_version = 1\n\n[[catalog]]\nid = \"marketing\"\nurl = \"{}\"\nversion = \"{version}\"\nskills = [{filters}]\n",
            catalog_repo.display()
        ),
    )
    .expect("team manifest should be written");
}

fn commit_all(repo: &std::path::Path, message: &str) {
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
            message,
            "-q",
        ],
    );
}
