use predicates::prelude::*;
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

#[test]
fn nearest_project_is_discovered_from_subdirectories_and_global_bypasses_it() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    git(&project, &["init"]);
    dalo_command()
        .arg("--project")
        .arg(&project)
        .arg("init")
        .assert()
        .success();
    let nested = project.join("src/deep");
    fs::create_dir_all(&nested).unwrap();
    let preview = dalo_command()
        .current_dir(&nested)
        .args(["install", "--dry-run", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: serde_json::Value = serde_json::from_slice(&preview).unwrap();
    assert_eq!(
        preview["project"],
        fs::canonicalize(&project).unwrap().to_str().unwrap()
    );
    assert!(!project.join(".dalo").exists());
    // Global explicitly means the home store, even with an environment override.
    let mut cmd = dalo_command();
    let home_store = cmd.test_environment().home.join(".dalo");
    let custom_store = temp.path().join("custom");
    cmd.current_dir(&nested)
        .env("DALO_STORE", &custom_store)
        .args(["init", "-g"])
        .assert()
        .success();
    assert!(home_store.join("config.toml").exists());
    assert!(!custom_store.exists());
    assert!(!project.join(".dalo").exists());
    dalo_command()
        .current_dir(&nested)
        .env("DALO_STORE", &custom_store)
        .arg("init")
        .assert()
        .success();
    assert!(custom_store.join("config.toml").exists());
}

#[test]
fn discovery_stops_at_nested_git_boundaries_and_nearest_manifest_wins() {
    let temp = tempfile::tempdir().unwrap();
    dalo_command()
        .arg("--project")
        .arg(temp.path())
        .arg("init")
        .assert()
        .success();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init"]);
    // The outer definition must not affect another repository.
    dalo_command()
        .current_dir(&repo)
        .args(["install", "--dry-run"])
        .assert()
        .failure();
    dalo_command()
        .arg("--project")
        .arg(&repo)
        .arg("init")
        .assert()
        .success();
    let nested = repo.join("nested");
    fs::create_dir(&nested).unwrap();
    dalo_command()
        .arg("--project")
        .arg(&nested)
        .arg("init")
        .assert()
        .success();
    let preview = dalo_command()
        .current_dir(&nested)
        .args(["install", "--json", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: serde_json::Value = serde_json::from_slice(&preview).unwrap();
    assert_eq!(
        preview["project"],
        fs::canonicalize(&nested).unwrap().to_str().unwrap()
    );
    fs::remove_file(nested.join("dalo-project.toml")).unwrap();
    // Worktrees and submodules use a .git file rather than a directory.
    fs::write(nested.join(".git"), "gitdir: ../elsewhere\n").unwrap();
    dalo_command()
        .current_dir(&nested)
        .args(["install", "--dry-run"])
        .assert()
        .failure();
}

#[test]
fn malformed_discovered_definition_never_falls_back_to_global() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("dalo-project.toml"), "invalid = [").unwrap();
    let mut cmd = dalo_command();
    let home_store = cmd.test_environment().home.join(".dalo");
    cmd.current_dir(temp.path()).arg("init").assert().failure();
    assert!(!home_store.exists());
    let mut cmd = dalo_command();
    let home_store = cmd.test_environment().home.join(".dalo");
    cmd.current_dir(temp.path())
        .args(["init", "--global"])
        .assert()
        .success();
    assert!(home_store.join("config.toml").exists());
}

#[test]
fn scope_flags_conflict_and_store_independent_commands_stay_available() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("dalo-project.toml"), "invalid = [").unwrap();
    dalo_command()
        .current_dir(temp.path())
        .args(["completions", "bash"])
        .assert()
        .success();
    dalo_command()
        .current_dir(temp.path())
        .args(["team", "show"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("dalo-project").not());
    dalo_command()
        .args(["--global", "--project", ".", "init"])
        .assert()
        .failure();
    dalo_command()
        .args(["--global", "--store", "temp", "init"])
        .assert()
        .failure();
}

fn init_in_terminal(
    root: &std::path::Path,
    home_store: &mut std::path::PathBuf,
    arguments: &[&str],
    answer: &str,
    environment: &[(&str, &str)],
) -> (std::process::Output, bool) {
    use std::io::{Read, Write};
    use std::process::Stdio;
    let isolated = dalo_command();
    *home_store = isolated.test_environment().home.join(".dalo");
    let executable = assert_cmd::cargo::cargo_bin!("dalo");
    let mut command = std::process::Command::new("/usr/bin/script");
    isolated.test_environment().apply_to(&mut command);
    command
        .current_dir(root)
        .env_remove("CI")
        .env("DALO_ASSISTANT_CHECK", "never");
    for (key, value) in environment {
        command.env(key, value);
    }
    #[cfg(target_os = "macos")]
    command
        .args(["-q", "/dev/null"])
        .arg(executable)
        .args(arguments);
    #[cfg(not(target_os = "macos"))]
    {
        let quote = |value: &std::ffi::OsStr| {
            format!("'{}'", value.to_string_lossy().replace('\'', "'\"'\"'"))
        };
        let mut parts = vec![quote(std::path::Path::new(executable).as_os_str())];
        parts.extend(
            arguments
                .iter()
                .map(|argument| quote(std::ffi::OsStr::new(argument))),
        );
        command
            .args(["-q", "-e", "-c"])
            .arg(parts.join(" "))
            .arg("/dev/null");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let answer = answer.to_owned();
    let reader = std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut byte = [0];
        let mut answered = false;
        while output.read(&mut byte).unwrap() > 0 {
            captured.push(byte[0]);
            if !answered && captured.ends_with(b"[p/g, Enter cancels] ") {
                input.write_all(answer.as_bytes()).unwrap();
                input.flush().unwrap();
                answered = true;
            }
        }
        captured
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("terminal command timed out: {arguments:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let stdout = reader.join().unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    let global_created = home_store.join("config.toml").exists();
    (
        std::process::Output {
            status,
            stdout,
            stderr,
        },
        global_created,
    )
}

#[test]
fn interactive_init_asks_once_and_cancellation_writes_nothing() {
    for answer in ["p\n", "g\n", "\n"] {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init"]);
        let mut home_store = std::path::PathBuf::new();
        let (output, global_created) =
            init_in_terminal(root.path(), &mut home_store, &["init"], answer, &[]);
        assert!(String::from_utf8_lossy(&output.stdout).contains("[p/g, Enter cancels]"));
        assert_eq!(
            root.path().join("dalo-project.toml").exists(),
            answer == "p\n"
        );
        assert_eq!(global_created, answer == "g\n");
        assert_eq!(output.status.success(), answer != "\n");
        assert!(!root.path().join(".dalo").exists());
    }
}

#[test]
fn interactive_ci_and_dry_run_do_not_ask_for_scope() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    let mut home_store = std::path::PathBuf::new();
    let (output, created) = init_in_terminal(
        root.path(),
        &mut home_store,
        &["init", "--dry-run"],
        "",
        &[],
    );
    assert!(output.status.success());
    assert!(!created);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("[p/g, Enter cancels]"));
    let (output, created) = init_in_terminal(
        root.path(),
        &mut home_store,
        &["init", "--json"],
        "",
        &[("CI", "true")],
    );
    assert!(output.status.success());
    assert!(created);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("[p/g, Enter cancels]"));
    assert!(!root.path().join("dalo-project.toml").exists());
}
