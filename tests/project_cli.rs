use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

mod common;
use common::dalo_command;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn upstream(root: &Path) -> String {
    fs::create_dir_all(root.join("skills/review")).unwrap();
    fs::write(root.join("skills/review/SKILL.md"), "---\nname: review\ndescription: Review the project documentation.\n---\n\nRead the documentation and summarize it.\n").unwrap();
    git(root, &["init"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "fixture",
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

fn manifest(project: &Path, url: &Path, commit: &str) {
    fs::write(project.join("dalo-project.toml"), format!(
        "schema_version = 1\ntargets = [\"claude\"]\n[[source]]\nid = \"shared\"\nurl = {:?}\ncommit = {:?}\nskills = [\"review\"]\n", url.to_str().unwrap(), commit)).unwrap();
}

#[test]
fn project_scope_is_explicit_and_does_not_replace_environment_store() {
    let temp = tempfile::tempdir().unwrap();
    let global = temp.path().join("global");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    dalo_command()
        .env("DALO_STORE", &global)
        .arg("--project")
        .arg(&project)
        .arg("init")
        .assert()
        .success();
    assert!(project.join("dalo-project.toml").exists());
    assert!(!project.join(".dalo").exists());
    assert!(!global.exists());
    dalo_command()
        .env("DALO_STORE", &global)
        .current_dir(&project)
        .arg("init")
        .assert()
        .success();
    assert!(global.join("config.toml").exists());
    assert!(!project.join(".dalo").exists());
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("--store")
        .arg(&global)
        .arg("init")
        .assert()
        .failure();
    dalo_command()
        .env("DALO_STORE", &global)
        .arg("install")
        .assert()
        .failure();
}

#[test]
fn install_restores_exact_commits_in_fresh_projects_and_requires_local_approval() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("upstream");
    let commit = upstream(&repo);
    // An upstream change must not move the declared pin on either machine.
    fs::write(repo.join("new.txt"), "new upstream state").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "next",
        ],
    );
    for name in ["first", "second"] {
        let project = temp.path().join(name);
        fs::create_dir(&project).unwrap();
        manifest(&project, &repo, &commit);
        let global = temp.path().join("global");
        let first = dalo_command()
            .env("DALO_STORE", &global)
            .arg("--project")
            .arg(&project)
            .arg("install")
            .assert()
            .failure();
        assert!(String::from_utf8_lossy(&first.get_output().stdout).contains("pending"));
        assert!(!project.join(".claude/skills/review").exists());
        assert!(!global.exists());
        assert_eq!(
            git(
                &project.join(".dalo/sources/shared/checkout"),
                &["rev-parse", "HEAD"]
            ),
            commit
        );
        dalo_command()
            .arg("--project")
            .arg(&project)
            .args(["approve", "skill", "shared:review"])
            .assert()
            .success();
        dalo_command()
            .arg("--project")
            .arg(&project)
            .args(["--json", "install"])
            .assert()
            .success();
        let link = project.join(".claude/skills/review");
        assert!(link.is_symlink());
        let lock = fs::read(project.join(".dalo/source-lock.toml")).unwrap();
        dalo_command()
            .arg("--project")
            .arg(&project)
            .arg("install")
            .assert()
            .success();
        assert_eq!(
            fs::read(project.join(".dalo/source-lock.toml")).unwrap(),
            lock
        );
        fs::write(link.join("SKILL.md"), "My local changes").unwrap();
        dalo_command()
            .arg("--project")
            .arg(&project)
            .arg("install")
            .assert()
            .failure();
        assert_eq!(
            fs::read_to_string(link.join("SKILL.md")).unwrap(),
            "My local changes"
        );
    }
}

#[test]
fn preview_and_invalid_declarations_do_not_create_state() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path();
    dalo_command()
        .arg("--project")
        .arg(project)
        .args(["--dry-run", "init"])
        .assert()
        .success();
    assert!(!project.join("dalo-project.toml").exists());
    manifest(
        project,
        Path::new("https://example.invalid/skills.git"),
        &"a".repeat(40),
    );
    dalo_command()
        .arg("--project")
        .arg(project)
        .args(["--json", "--dry-run", "install"])
        .assert()
        .success();
    assert!(!project.join(".dalo").exists());
    manifest(
        project,
        Path::new("https://example.invalid/skills.git"),
        "main",
    );
    dalo_command()
        .arg("--project")
        .arg(project)
        .arg("install")
        .assert()
        .failure();
    assert!(!project.join(".dalo").exists());
}

#[test]
fn redirected_paths_and_foreign_store_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let elsewhere = temp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, project.join(".dalo")).unwrap();
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("init")
        .assert()
        .failure();
    fs::remove_file(project.join(".dalo")).unwrap();
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("init")
        .assert()
        .success();
    symlink(&elsewhere, project.join(".claude")).unwrap();
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("install")
        .assert()
        .failure();
    assert!(!elsewhere.join("skills").exists());
    fs::remove_file(project.join(".claude")).unwrap();
    fs::create_dir(project.join(".dalo")).unwrap();
    fs::write(project.join(".dalo/important"), "preserve").unwrap();
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("install")
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(project.join(".dalo/important")).unwrap(),
        "preserve"
    );
}

#[test]
fn tracked_store_state_cannot_supply_shared_approvals() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init"]);
    fs::create_dir(temp.path().join(".dalo")).unwrap();
    fs::write(temp.path().join(".dalo/project-owner"), "dalo-project-v1\n").unwrap();
    git(temp.path(), &["add", ".dalo"]);
    dalo_command()
        .arg("--project")
        .arg(temp.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(predicates::str::contains("must not be tracked"));
    assert!(!temp.path().join("dalo-project.toml").exists());
}

#[test]
fn unmanaged_skill_and_changed_pin_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("upstream");
    let commit = upstream(&repo);
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    manifest(&project, &repo, &commit);
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("install")
        .assert()
        .failure();
    dalo_command()
        .arg("--project")
        .arg(&project)
        .args(["approve", "skill", "shared:review"])
        .assert()
        .success();
    let slot = project.join(".claude/skills/review");
    fs::create_dir(&slot).unwrap();
    fs::write(slot.join("SKILL.md"), "Project-authored skill").unwrap();
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("install")
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(slot.join("SKILL.md")).unwrap(),
        "Project-authored skill"
    );
    manifest(&project, &repo, &"a".repeat(40));
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("install")
        .assert()
        .failure();
    assert_eq!(
        git(
            &project.join(".dalo/sources/shared/checkout"),
            &["rev-parse", "HEAD"]
        ),
        commit
    );
}
