//! Instruction adoption through the real CLI, local Git sources and target files.
mod common;

use common::{add_source, create_git_skill_repo, dalo_command, read_user_lock, run_git};
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    _temp: tempfile::TempDir,
    store: PathBuf,
    repo: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = temp.path().join("store");
        let repo = temp.path().join("team");
        let target = temp.path().join("CLAUDE.md");
        dalo_command()
            .args(["--store"])
            .arg(&store)
            .arg("init")
            .assert()
            .success();
        fs::create_dir_all(repo.join("instructions")).unwrap();
        fs::write(
            repo.join("instructions/style.md"),
            "version: 2\ntopics: style\n\nTeam body.\n",
        )
        .unwrap();
        create_git_skill_repo(&repo);
        add_source(&store, "team", &repo);
        fs::write(&target, "# User heading\r\n\r\nUser footer.\r\n").unwrap();
        let fixture = Self {
            _temp: temp,
            store,
            repo,
            target,
        };
        fixture.enable(&fixture.target);
        fixture
    }
    fn command(&self) -> common::DaloCommand {
        let mut command = dalo_command();
        command.args(["--store"]).arg(&self.store);
        command
    }
    fn enable(&self, target: &Path) {
        self.command()
            .args(["instructions", "enable", "team:style"])
            .arg(target)
            .assert()
            .success();
    }
    fn edit(&self) -> String {
        let original = fs::read_to_string(&self.target).unwrap();
        let edited = original.replace(
            "Team body.",
            "Local variant: ä, 日本語.\r\nKeep my convention.",
        );
        fs::write(&self.target, &edited).unwrap();
        edited
    }
    fn pack(&self) -> PathBuf {
        self.store.join("local/instructions/style.md")
    }
    fn checkout(&self) -> PathBuf {
        self.store.join("sources/team/checkout")
    }
}

#[test]
fn instructions_adopt_should_preview_replace_only_markers_and_survive_team_refresh() {
    let fixture = Fixture::new();
    let other = fixture._temp.path().join("AGENTS.md");
    fixture.enable(&other);
    let edited = fixture.edit();
    let lock_before = fs::read(fixture.store.join("lock.toml")).unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("several files"));
    let preview = fixture
        .command()
        .args(["--json", "--dry-run", "instructions", "adopt", "team:style"])
        .arg(&fixture.target)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: serde_json::Value = serde_json::from_slice(&preview).unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(
        preview["body"],
        "Local variant: ä, 日本語.\r\nKeep my convention."
    );
    fixture
        .command()
        .args(["--dry-run", "instructions", "adopt", "team:style"])
        .arg(&fixture.target)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Local variant: ä, 日本語.\nKeep my convention.",
        ));
    assert!(!fixture.pack().exists());
    assert_eq!(fs::read_to_string(&fixture.target).unwrap(), edited);
    assert_eq!(
        fs::read(fixture.store.join("lock.toml")).unwrap(),
        lock_before
    );
    let original_source = fs::read(fixture.checkout().join("instructions/style.md")).unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .arg(&fixture.target)
        .assert()
        .success()
        .stdout(predicate::str::contains("local:style"));
    assert_eq!(
        fs::read_to_string(&fixture.target).unwrap(),
        edited.replace("team:style", "style")
    );
    assert!(
        fs::read_to_string(fixture.pack())
            .unwrap()
            .contains("Local variant: ä, 日本語.")
    );
    assert_eq!(
        fs::read(fixture.checkout().join("instructions/style.md")).unwrap(),
        original_source
    );
    let lock = read_user_lock(&fixture.store);
    assert_eq!(lock.active_instruction_packs.len(), 2);
    assert!(
        lock.active_instruction_packs
            .iter()
            .any(|entry| entry.source_id == "local" && entry.commit.is_none())
    );
    assert!(
        lock.active_instruction_packs
            .iter()
            .any(|entry| entry.source_id == "team")
    );
    fixture.command().arg("sync").assert().success();
    fs::write(
        fixture.repo.join("instructions/style.md"),
        "version: 3\n\nNew team body.\n",
    )
    .unwrap();
    run_git(&fixture.repo, &["add", "."]);
    run_git(
        &fixture.repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "update instructions",
        ],
    );
    fixture.command().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(&fixture.target).unwrap(),
        edited.replace("team:style", "style")
    );
    assert!(
        fs::read_to_string(other)
            .unwrap()
            .contains("New team body.")
    );
    fixture
        .command()
        .args(["instructions", "disable", "style"])
        .arg(&fixture.target)
        .assert()
        .success();
    assert!(
        fixture.pack().is_file(),
        "disabling never deletes the local pack"
    );
}

#[test]
fn instructions_adopt_should_infer_unique_target_and_keep_local_pack_uncommitted() {
    let fixture = Fixture::new();
    fixture.edit();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .success();
    assert_eq!(
        read_user_lock(&fixture.store).active_instruction_packs[0].source_id,
        "local"
    );
    let status = common::git_stdout(&fixture.store.join("local"), &["status", "--porcelain"]);
    assert!(status.contains("instructions/"));
}

#[test]
fn instructions_adopt_should_preserve_files_for_unedited_missing_malformed_and_nested_blocks() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no edits"));
    let original = fs::read_to_string(&fixture.target).unwrap();
    let lock_before = fs::read(fixture.store.join("lock.toml")).unwrap();
    for content in [
        "Unmanaged file".to_owned(),
        original.replace("<!-- dalo:end team:style -->", ""),
        original.replace(
            "Team body.",
            "<!-- dalo:start nested -->\r\nCustom\r\n<!-- dalo:end nested -->",
        ),
    ] {
        fs::write(&fixture.target, &content).unwrap();
        fixture
            .command()
            .args(["instructions", "adopt", "team:style"])
            .assert()
            .failure();
        assert_eq!(fs::read_to_string(&fixture.target).unwrap(), content);
        assert_eq!(
            fs::read(fixture.store.join("lock.toml")).unwrap(),
            lock_before
        );
        assert!(!fixture.pack().exists());
    }
}

#[test]
fn instructions_adopt_should_never_overwrite_local_content_or_redirected_directories() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let edited = fixture.edit();
    let outside = fixture._temp.path().join("outside.md");
    fs::write(&outside, "Foreign content").unwrap();
    symlink(&outside, fixture.pack()).unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
    assert_eq!(fs::read_to_string(&outside).unwrap(), "Foreign content");
    fs::remove_file(fixture.pack()).unwrap();
    fs::write(fixture.pack(), "My existing pack").unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(fixture.pack()).unwrap(),
        "My existing pack"
    );
    fs::remove_file(fixture.pack()).unwrap();
    let directory = fixture.store.join("local/instructions");
    fs::remove_dir(&directory).unwrap();
    let redirected = fixture._temp.path().join("redirected");
    fs::create_dir(&redirected).unwrap();
    symlink(&redirected, &directory).unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("non-symlink directories"));
    assert!(!redirected.join("style.md").exists());
    assert_eq!(fs::read_to_string(&fixture.target).unwrap(), edited);
}

#[test]
fn instructions_adopt_should_block_dirty_source_and_lossy_metadata_conversion() {
    let fixture = Fixture::new();
    let edited = fixture.edit();
    let source_pack = fixture.checkout().join("instructions/style.md");
    let original = fs::read(&source_pack).unwrap();
    fs::write(&source_pack, "Dirty source").unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("local changes"));
    fs::write(source_pack, original).unwrap();
    fs::write(
        &fixture.target,
        edited.replace("Local variant: ä, 日本語.", "topics: user-authored body"),
    )
    .unwrap();
    fixture
        .command()
        .args(["instructions", "adopt", "team:style"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("without changing content"));
    assert!(!fixture.pack().exists());
}
