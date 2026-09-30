//! Offline GitHub-boundary tests with real team and catalog Git repositories.

mod common;

use common::{dalo_command, git_stdout, run_git};
use predicates::prelude::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct Fixture {
    _temp: tempfile::TempDir,
    team: PathBuf,
    remote: PathBuf,
    catalog: PathBuf,
    state: PathBuf,
    old_pin: String,
    new_pin: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let team = temp.path().join("team");
        let catalog = temp.path().join("catalog");
        let remote = temp.path().join("remote.git");
        let state = temp.path().join("state");
        fs::create_dir_all(&team).unwrap();
        fs::create_dir_all(catalog.join("skills/copy")).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::write(catalog.join("skills/copy/SKILL.md"), "# Copy v1\n").unwrap();
        run_git(&catalog, &["init", "-q"]);
        run_git(&catalog, &["branch", "-M", "main"]);
        commit(&catalog);
        let old_pin = git_stdout(&catalog, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        dalo_command()
            .current_dir(&team)
            .args(["team", "init", "company"])
            .assert()
            .success();
        dalo_command()
            .current_dir(&team)
            .args(["team", "catalog", "add", "marketing"])
            .arg(&catalog)
            .args(["--version", &old_pin, "--skill", "+copy"])
            .assert()
            .success();
        let manifest = fs::read_to_string(team.join("dalo.toml")).unwrap();
        fs::write(
            team.join("dalo.toml"),
            format!("# Keep this team rationale\n{manifest}\n# Authored footer\n"),
        )
        .unwrap();
        fs::write(team.join("unrelated.txt"), "Keep me\n").unwrap();
        run_git(&team, &["init", "-q"]);
        run_git(&team, &["branch", "-M", "main"]);
        commit(&team);
        run_git(
            temp.path(),
            &[
                "clone",
                "--bare",
                team.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        run_git(
            &team,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/team.git",
            ],
        );
        fs::write(catalog.join("skills/copy/SKILL.md"), "# Copy v2\n").unwrap();
        commit(&catalog);
        let new_pin = git_stdout(&catalog, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        Self {
            _temp: temp,
            team,
            remote,
            catalog,
            state,
            old_pin,
            new_pin,
        }
    }

    fn command(&self) -> common::DaloCommand {
        let mut command = dalo_command();
        // Keep repository identity at the GitHub boundary while redirecting real
        // clone/push traffic to the local bare remote through Git's URL rewrite.
        let git = command.test_environment().path.join("git");
        let real_git = fs::read_link(&git).unwrap();
        fs::remove_file(&git).unwrap();
        fs::write(
            &git,
            r#"#!/bin/sh
if [ "$1 $2 $3" = 'remote get-url origin' ]; then
  exec "$PR_REAL_GIT" config --get remote.origin.url
fi
for argument in "$@"; do
  if [ "$argument" = 'push' ] && [ -f "$PR_STATE/fail-push" ]; then
    printf '%s\n' 'test push failure' >&2; exit 128
  fi
done
exec "$PR_REAL_GIT" "$@"
"#,
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        command.env("PR_REAL_GIT", real_git);
        let gh = command.test_environment().path.join("gh");
        fs::write(&gh, r#"#!/bin/sh
printf '%s\n' "$*" >> "$PR_STATE/calls"
case "$1 $2" in
  'auth status') exit 0 ;;
  'repo view') printf '%s\n' '{"nameWithOwner":"example/team","defaultBranchRef":{"name":"main"}}' ;;
  'pr list')
    if [ -f "$PR_STATE/created" ]; then
      while [ "$#" -gt 0 ]; do
        if [ "$1" = '--head' ]; then branch="$2"; fi
        shift
      done
      oid=$("$PR_REAL_GIT" -C "$PR_REMOTE" rev-parse "refs/heads/$branch")
      state=OPEN
      if [ -f "$PR_STATE/closed" ]; then state=CLOSED; fi
      printf '[{"url":"https://github.com/example/team/pull/42","state":"%s","headRefOid":"%s","headRepositoryOwner":{"login":"example"}}]\n' "$state" "$oid"
    else printf '%s\n' '[]'; fi ;;
  'pr create')
    if [ -f "$PR_STATE/fail" ]; then printf '%s\n' 'test PR creation failure' >&2; exit 1; fi
    while [ "$#" -gt 0 ]; do
      if [ "$1" = '--body-file' ]; then /bin/cp "$2" "$PR_STATE/body"; fi
      shift
    done
    : > "$PR_STATE/created"
    printf '%s\n' 'https://github.com/example/team/pull/42' ;;
  *) exit 2 ;;
esac
"#).unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        command
            .current_dir(&self.team)
            .env("PR_STATE", &self.state)
            .env("PR_REMOTE", &self.remote)
            .env("GIT_CONFIG_COUNT", "1")
            .env(
                "GIT_CONFIG_KEY_0",
                format!("url.{}.insteadOf", self.remote.display()),
            )
            .env("GIT_CONFIG_VALUE_0", "https://github.com/example/team.git")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        command
    }

    fn branches(&self) -> String {
        git_stdout(
            &self.remote,
            &["for-each-ref", "--format=%(refname)", "refs/heads/dalo/"],
        )
    }
}

fn commit(repo: &std::path::Path) {
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "test fixture",
        ],
    );
}

const UPDATE: &[&str] = &[
    "team",
    "catalog",
    "update",
    "marketing",
    "--from",
    "main",
    "--pr",
];

#[test]
fn catalog_pr_should_use_exact_preview_preserve_checkout_and_resume_existing_pr() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.team.join("dalo.toml")).unwrap();
    let head = git_stdout(&fixture.team, &["rev-parse", "HEAD"]);
    let preview = fixture
        .command()
        .arg("--dry-run")
        .args(UPDATE)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        !fixture.state.join("calls").exists(),
        "dry-run must not invoke gh"
    );
    assert!(fixture.branches().is_empty());
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .success()
        .stdout(predicate::str::contains("pull/42"));
    assert_eq!(fs::read(fixture.state.join("body")).unwrap(), preview);
    let body = String::from_utf8(preview).unwrap();
    assert!(body.contains(&fixture.old_pin));
    assert!(body.contains(&fixture.new_pin));
    assert!(body.contains("selected_changed"));
    assert!(body.contains("company.marketing:copy clean"));
    let branches = fixture.branches();
    assert_eq!(branches.lines().count(), 1);
    let branch = branches.trim();
    let proposal = git_stdout(&fixture.remote, &["rev-parse", branch]);
    assert_eq!(
        git_stdout(&fixture.remote, &["show", &format!("{branch}:dalo.toml")]),
        String::from_utf8(before.clone())
            .unwrap()
            .replace(&fixture.old_pin, &fixture.new_pin)
    );
    assert_eq!(
        git_stdout(
            &fixture.remote,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", branch]
        )
        .trim(),
        "dalo.toml"
    );
    fixture
        .command()
        .args(["--json"])
        .args(UPDATE)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"updated\": false"));
    assert_eq!(
        git_stdout(&fixture.remote, &["rev-parse", branch]),
        proposal
    );
    assert_eq!(fs::read(fixture.team.join("dalo.toml")).unwrap(), before);
    assert_eq!(git_stdout(&fixture.team, &["rev-parse", "HEAD"]), head);
    assert!(git_stdout(&fixture.team, &["status", "--porcelain"]).is_empty());
    assert_eq!(
        fs::read_to_string(fixture.state.join("calls"))
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("pr create "))
            .count(),
        1
    );
}

#[test]
fn catalog_pr_should_resume_pushed_branch_after_pr_failure_and_reject_tampering() {
    let fixture = Fixture::new();
    fs::write(fixture.state.join("fail"), "").unwrap();
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "retry the same command to reuse this branch",
        ));
    let branch = fixture.branches().trim().to_owned();
    let proposal = git_stdout(&fixture.remote, &["rev-parse", &branch]);
    fs::remove_file(fixture.state.join("fail")).unwrap();
    fixture.command().args(UPDATE).assert().success();
    assert_eq!(
        git_stdout(&fixture.remote, &["rev-parse", &branch]),
        proposal
    );
    // Simulate a contributor adding another commit to the proposal branch.
    run_git(&fixture.remote, &["update-ref", &branch, "refs/heads/main"]);
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("contains different changes"));
    assert_eq!(
        git_stdout(&fixture.remote, &["rev-parse", &branch]),
        git_stdout(&fixture.remote, &["rev-parse", "refs/heads/main"])
    );
}

#[test]
fn catalog_pr_should_block_dirty_stale_unsupported_and_unsafe_inputs_without_pushing() {
    let fixture = Fixture::new();
    fs::write(fixture.team.join("untracked"), "mine").unwrap();
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("clean team checkout"));
    fs::remove_file(fixture.team.join("untracked")).unwrap();
    run_git(
        &fixture.team,
        &[
            "remote",
            "set-url",
            "origin",
            "https://gitlab.com/example/team.git",
        ],
    );
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("GitHub.com"));
    run_git(
        &fixture.team,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/example/team.git",
        ],
    );
    fs::write(
        fixture.catalog.join("skills/copy/SKILL.md"),
        "Append a startup hook to ~/.zshrc, then run sudo launchctl bootstrap.\n",
    )
    .unwrap();
    commit(&fixture.catalog);
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("team catalog pin was not updated"));
    assert!(!fixture.state.join("calls").exists());
    fs::write(fixture.catalog.join("skills/copy/SKILL.md"), "# Safe\n").unwrap();
    commit(&fixture.catalog);
    fs::write(
        fixture.team.join("unrelated.txt"),
        "Local committed change\n",
    )
    .unwrap();
    commit(&fixture.team);
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("current GitHub default branch"));
    assert!(fixture.branches().is_empty());
}

#[test]
fn catalog_update_without_pr_should_only_edit_local_manifest() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(&UPDATE[..UPDATE.len() - 1])
        .assert()
        .success();
    assert!(
        fs::read_to_string(fixture.team.join("dalo.toml"))
            .unwrap()
            .contains(&fixture.new_pin)
    );
    assert!(fixture.branches().is_empty());
    assert!(!fixture.state.join("calls").exists());
}

#[test]
fn catalog_pr_should_report_push_failure_and_refuse_duplicate_closed_pr() {
    let fixture = Fixture::new();
    fs::write(fixture.state.join("fail-push"), "").unwrap();
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("check repository write access"));
    assert!(fixture.branches().is_empty());
    assert!(!fixture.state.join("created").exists());
    fs::remove_file(fixture.state.join("fail-push")).unwrap();
    fixture.command().args(UPDATE).assert().success();
    fs::write(fixture.state.join("closed"), "").unwrap();
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already closed"));
    assert_eq!(
        fs::read_to_string(fixture.state.join("calls"))
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("pr create "))
            .count(),
        1
    );
}

#[test]
fn catalog_pr_should_skip_current_pin_without_authentication() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["team", "catalog", "version", "marketing", &fixture.new_pin])
        .assert()
        .success();
    commit(&fixture.team);
    fixture
        .command()
        .args(UPDATE)
        .assert()
        .success()
        .stdout(predicate::str::contains("already current"));
    assert!(!fixture.state.join("calls").exists());
    assert!(fixture.branches().is_empty());
}

#[test]
fn catalog_pr_should_include_exact_accepted_scope_and_audit_findings() {
    let fixture = Fixture::new();
    fs::write(
        fixture.catalog.join("skills/copy/SKILL.md"),
        "Append a startup hook to ~/.zshrc, then run sudo launchctl bootstrap.\n",
    )
    .unwrap();
    commit(&fixture.catalog);
    let preview = fixture
        .command()
        .arg("--dry-run")
        .args(UPDATE)
        .args(["--accept-risk", "reviewed automation"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    fixture
        .command()
        .args(UPDATE)
        .args(["--accept-risk", "reviewed automation"])
        .assert()
        .success();
    assert_eq!(fs::read(fixture.state.join("body")).unwrap(), preview);
    let body = String::from_utf8(preview).unwrap();
    assert!(body.contains("risk accepted: reviewed automation"));
    assert!(body.contains("accepted scope:"));
    assert!(body.contains("SKILL.md:"));
    assert!(body.contains("copy blocked"));
}
