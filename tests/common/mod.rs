#![allow(dead_code)]

use assert_cmd::Command;
use dalo::catalog::{self, SourceLock};
use dalo::config::UserConfig;
use dalo::lockfile::UserLock;
use dalo::store::{self, ApprovalRecord, StorePaths};
use dalo::{source, target};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn dalo_command() -> Command {
    let mut command = Command::cargo_bin("dalo").expect("binary should build");
    command.env_remove("DALO_STORE");
    command
}

pub fn setup_store_with_target(store: &Path, target: &Path) {
    init_store(store);
    link_target(store, target);
}

pub fn setup_store_with_skill_and_target(store: &Path, target: &Path) {
    setup_store_with_target(store, target);
    create_local_skill(store, "review", "# Review\n");
}

pub fn init_store(store: &Path) {
    store::init_store(store.to_path_buf(), false).expect("store should initialize");
}

pub fn link_target(store: &Path, target: &Path) {
    target::link_target(store, "generic", Some(target), false).expect("target should link");
}

pub fn add_source(store: &Path, source: &str, repo: &Path) {
    source::add_team_source(
        &StorePaths::new(store.to_path_buf()),
        source,
        repo.to_str().expect("repo path should be utf8"),
        false,
    )
    .expect("source should be added");
}

pub fn create_local_skill(store: &Path, slot_name: &str, body: &str) {
    let skill_dir = store.join("local/skills").join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("local skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("local skill should be written");
}

pub fn create_unmanaged_skill(target: &Path, slot_name: &str) {
    create_unmanaged_skill_with_body(target, slot_name, &format!("# {slot_name}\n"));
}

pub fn create_unmanaged_skill_with_body(target: &Path, slot_name: &str, body: &str) {
    let skill_dir = target.join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("unmanaged skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("unmanaged skill should be written");
}

pub fn create_git_skill_repo(repo: &Path) {
    create_git_skill_repo_with_skill(repo, "team", "# Team\n");
}

pub fn create_git_skill_repo_with_skill(repo: &Path, slot_name: &str, body: &str) {
    let skill_dir = repo.join("skills").join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("repo skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("repo skill should be written");
    init_git_repo(repo);
}

pub fn create_git_catalog_repo(repo: &Path) {
    for slot in ["copy-editing", "launch-copy"] {
        let skill_dir = repo.join("skills").join(slot);
        std::fs::create_dir_all(&skill_dir).expect("repo dirs created");
        std::fs::write(skill_dir.join("SKILL.md"), format!("# {slot}\n")).expect("skill written");
    }
    init_git_repo(repo);
}

pub fn create_git_catalog_repo_with_duplicate_slots(repo: &Path) {
    for folder in ["a", "b"] {
        let skill_dir = repo.join("skills").join(folder);
        std::fs::create_dir_all(&skill_dir).expect("repo dirs created");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: shared\n---\n# Shared\n",
        )
        .expect("skill written");
    }
    init_git_repo(repo);
}

fn init_git_repo(repo: &Path) {
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

pub fn approve_source(store: &Path, source: &str) {
    let paths = StorePaths::new(store.to_path_buf());
    let mut approvals = store::read_approvals(&paths).expect("approvals should be readable");
    approvals.approvals.push(ApprovalRecord {
        scope: "source".to_owned(),
        value: source.to_owned(),
    });
    store::write_approvals(&paths, &approvals).expect("source approval should be written");
}

pub fn set_source_untrusted(store: &Path, source_id: &str) {
    update_config(store, |config| {
        let source = config
            .sources
            .iter_mut()
            .find(|source| source.id == source_id)
            .expect("source should exist");
        source.trusted = false;
    });
}

pub fn remove_source_update_policy(store: &Path, source_id: &str) {
    update_config(store, |config| {
        let source = config
            .sources
            .iter_mut()
            .find(|source| source.id == source_id)
            .expect("source should exist");
        source.update_policy = None;
    });
}

pub fn write_local_only_config(store: &Path) {
    let paths = StorePaths::new(store.to_path_buf());
    let config = UserConfig::default_for_store(store);
    store::write_config(&paths, &config).expect("config should be written");
}

fn update_config(store: &Path, update: impl FnOnce(&mut UserConfig)) {
    let paths = StorePaths::new(store.to_path_buf());
    let mut config = store::read_config(&paths).expect("config should be readable");
    update(&mut config);
    store::write_config(&paths, &config).expect("config should be written");
}

pub fn read_user_lock(store: &Path) -> UserLock {
    store::read_user_lock(&StorePaths::new(store.to_path_buf()))
        .expect("user lock should be readable")
}

pub fn read_source_lock(store: &Path) -> SourceLock {
    catalog::read_source_lock(&StorePaths::new(store.to_path_buf()))
        .expect("source lock should be readable")
}

pub fn write_source_lock(store: &Path, lock: &SourceLock) {
    catalog::write_source_lock(&StorePaths::new(store.to_path_buf()), lock)
        .expect("source lock should be writable");
}

pub fn run_git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("git should run");
    assert!(status.success(), "git {args:?} should succeed");
}

pub fn git_command_succeeds(repo: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git should run")
        .success()
}

pub struct GitRevParseLogger {
    pub path_env: OsString,
    pub log: PathBuf,
    pub real_git: String,
}

pub fn git_rev_parse_logger(temp_dir: &Path) -> GitRevParseLogger {
    let real_git_output = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("real git should be discoverable");
    assert!(
        real_git_output.status.success(),
        "real git should be discoverable"
    );
    let real_git = String::from_utf8(real_git_output.stdout)
        .expect("git path should be utf8")
        .trim()
        .to_owned();

    let bin = temp_dir.join("git-wrapper-bin");
    std::fs::create_dir_all(&bin).expect("wrapper bin should be created");
    let wrapper = bin.join("git");
    std::fs::write(
        &wrapper,
        "#!/bin/sh\n\
         if [ \"$1\" = \"rev-parse\" ] && [ \"$2\" = \"HEAD\" ]; then\n\
         \tprintf '%s\\n' \"$PWD\" >> \"$DALO_GIT_REV_PARSE_LOG\"\n\
         fi\n\
         exec \"$DALO_REAL_GIT\" \"$@\"\n",
    )
    .expect("git wrapper should be written");
    let mut permissions = std::fs::metadata(&wrapper)
        .expect("git wrapper metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("git wrapper should be executable");

    let mut path_env = bin.into_os_string();
    path_env.push(":");
    path_env.push(std::env::var_os("PATH").unwrap_or_default());

    GitRevParseLogger {
        path_env,
        log: temp_dir.join("git-rev-parse.log"),
        real_git,
    }
}
