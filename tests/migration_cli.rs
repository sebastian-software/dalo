use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

mod common;
use common::dalo_command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn fixture(root: &Path) -> String {
    let upstream = root.join("upstream");
    let project = root.join("project");
    fs::create_dir_all(upstream.join("skills/review")).unwrap();
    let content = "---\nname: review\ndescription: Review documentation.\n---\nRead the docs.\n";
    fs::write(upstream.join("skills/review/SKILL.md"), content).unwrap();
    git(&upstream, &["init"]);
    git(&upstream, &["add", "."]);
    git(
        &upstream,
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
    fs::create_dir_all(project.join(".agents/skills/review")).unwrap();
    fs::create_dir_all(project.join(".claude/skills")).unwrap();
    fs::write(project.join(".agents/skills/review/SKILL.md"), content).unwrap();
    symlink(
        "../../.agents/skills/review",
        project.join(".claude/skills/review"),
    )
    .unwrap();
    let lock = serde_json::json!({"version":1,"skills":{"review":{
        "source":upstream,"sourceType":"git","skillPath":"skills/review/SKILL.md","computedHash":"a".repeat(64)
    }}});
    fs::write(
        project.join("skills-lock.json"),
        serde_json::to_vec(&lock).unwrap(),
    )
    .unwrap();
    git(&upstream, &["rev-parse", "HEAD"])
}

#[test]
fn verified_handover_preserves_originals_and_requires_fresh_approval() {
    let temp = tempfile::tempdir().unwrap();
    let commit = fixture(temp.path());
    let root = temp.path().join("project");
    fs::create_dir_all(root.join(".agents/skills/personal")).unwrap();
    fs::write(root.join(".agents/skills/personal/note"), "mine").unwrap();
    for args in [
        vec!["migrate", "skills-sh", "--json"],
        vec!["migrate", "skills-sh", "--apply", "--dry-run", "--json"],
    ] {
        let result = dalo_command()
            .current_dir(&root)
            .args(args)
            .assert()
            .success();
        let report: serde_json::Value =
            serde_json::from_slice(&result.get_output().stdout).unwrap();
        assert_eq!(report["skills"][0]["commit"], commit);
        assert_eq!(report["applied"], false);
        assert!(!root.join("dalo-project.toml").exists());
        assert!(root.join(".claude/skills/review/SKILL.md").exists());
    }
    dalo_command()
        .current_dir(&root)
        .args(["migrate", "skills-sh", "--apply"])
        .assert()
        .success();
    let manifest = fs::read_to_string(root.join("dalo-project.toml")).unwrap();
    assert!(manifest.contains(&commit));
    assert!(!root.join("skills-lock.json").exists());
    assert!(!root.join(".dalo").exists());
    assert!(
        root.join(".dalo-migration-backup/.agents/skills/review/SKILL.md")
            .exists()
    );
    assert_eq!(
        fs::read_link(root.join(".dalo-migration-backup/.claude/skills/review")).unwrap(),
        Path::new("../../.agents/skills/review")
    );
    dalo_command()
        .current_dir(&root)
        .arg("install")
        .assert()
        .failure();
    dalo_command()
        .current_dir(&root)
        .args(["approve", "skill", "imported-1:review"])
        .assert()
        .success();
    dalo_command()
        .current_dir(&root)
        .arg("install")
        .assert()
        .success();
    assert!(root.join(".claude/skills/review/SKILL.md").exists());
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/personal/note")).unwrap(),
        "mine"
    );
}

#[test]
fn ambiguous_or_modified_installations_block_the_entire_handover() {
    for case in [
        "modified",
        "upstream",
        "external-link",
        "nested-link",
        "unsupported",
        "version",
        "existing",
        "backup",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path());
        let root = temp.path().join("project");
        match case {
            "modified" => {
                fs::write(root.join(".agents/skills/review/SKILL.md"), "my edits").unwrap()
            }
            "upstream" => {
                let repo = temp.path().join("upstream");
                fs::write(
                    repo.join("skills/review/SKILL.md"),
                    "---\nname: review\ndescription: Changed.\n---\nNew content",
                )
                .unwrap();
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
                        "change",
                    ],
                );
            }
            "external-link" => {
                fs::remove_file(root.join(".claude/skills/review")).unwrap();
                symlink(
                    temp.path().join("upstream/skills/review"),
                    root.join(".claude/skills/review"),
                )
                .unwrap();
            }
            "nested-link" => symlink("SKILL.md", root.join(".agents/skills/review/alias")).unwrap(),
            "unsupported" | "version" => {
                let path = root.join("skills-lock.json");
                let mut lock: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                if case == "version" {
                    lock["version"] = 99.into();
                } else {
                    lock["skills"]["other"] = serde_json::json!({"source":"local","sourceType":"local","computedHash":"b".repeat(64)});
                }
                fs::write(path, serde_json::to_vec(&lock).unwrap()).unwrap();
            }
            "existing" => fs::write(root.join("dalo-project.toml"), "existing").unwrap(),
            "backup" => fs::create_dir(root.join(".dalo-migration-backup")).unwrap(),
            _ => unreachable!(),
        }
        let lock = fs::read(root.join("skills-lock.json")).unwrap();
        let content = fs::read(root.join(".agents/skills/review/SKILL.md")).unwrap();
        dalo_command()
            .current_dir(&root)
            .args(["migrate", "skills-sh", "--apply"])
            .assert()
            .failure();
        assert_eq!(
            fs::read(root.join("skills-lock.json")).unwrap(),
            lock,
            "{case}"
        );
        assert_eq!(
            fs::read(root.join(".agents/skills/review/SKILL.md")).unwrap(),
            content,
            "{case}"
        );
        assert!(!root.join(".dalo").exists());
        if case != "existing" {
            assert!(!root.join("dalo-project.toml").exists());
        }
    }
}

#[test]
fn discovery_respects_git_boundaries_and_scope_overrides() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    let root = temp.path().join("project");
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    dalo_command()
        .current_dir(&nested)
        .args(["migrate", "skills-sh"])
        .assert()
        .success();
    git(&nested, &["init"]);
    dalo_command()
        .current_dir(&nested)
        .args(["migrate", "skills-sh"])
        .assert()
        .failure();
    dalo_command()
        .current_dir(&root)
        .args(["migrate", "skills-sh", "--global"])
        .assert()
        .failure();
    assert!(root.join("skills-lock.json").exists());
}

#[test]
fn recorded_ref_preserves_content_when_default_branch_has_advanced() {
    let temp = tempfile::tempdir().unwrap();
    let commit = fixture(temp.path());
    let root = temp.path().join("project");
    let repo = temp.path().join("upstream");
    git(&repo, &["tag", "installed-version"]);
    fs::write(repo.join("skills/review/SKILL.md"), "changed upstream").unwrap();
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
    let path = root.join("skills-lock.json");
    let mut lock: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    lock["skills"]["review"]["ref"] = "installed-version".into();
    fs::write(path, serde_json::to_vec(&lock).unwrap()).unwrap();
    let result = dalo_command()
        .current_dir(&root)
        .args(["migrate", "skills-sh", "--json"])
        .assert()
        .success();
    let report: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(report["skills"][0]["commit"], commit);
}

#[test]
fn blocked_json_keeps_a_parseable_report_on_stdout() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    let root = temp.path().join("project");
    fs::write(root.join(".agents/skills/review/SKILL.md"), "local edit").unwrap();
    let result = dalo_command()
        .current_dir(&root)
        .args(["migrate", "skills-sh", "--apply", "--json"])
        .assert()
        .failure();
    let report: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(report["ready"], false);
    assert!(
        report["skills"][0]["blocked"]
            .as_str()
            .unwrap()
            .contains("differs")
    );
    assert!(!root.join(".dalo-migration-backup").exists());
}
