#![allow(dead_code)]

use assert_cmd::Command;
use dalo::catalog::{self, SourceLock};
use dalo::config::UserConfig;
use dalo::lockfile::UserLock;
use dalo::store::{self, ApprovalRecord, StorePaths};
use dalo::{source, target};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A command whose provider-related environment is private to this invocation.
///
/// `TempDir` is retained for as long as the command exists, so parallel tests
/// never share mutable provider configuration and cleanup remains automatic.
pub struct DaloCommand {
    command: Command,
    environment: TestEnvironment,
}

impl Deref for DaloCommand {
    type Target = Command;

    fn deref(&self) -> &Self::Target {
        &self.command
    }
}

impl DerefMut for DaloCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.command
    }
}

impl DaloCommand {
    pub fn test_environment(&self) -> &TestEnvironment {
        &self.environment
    }
}

/// Paths which isolate a test command from provider configuration on the host.
pub struct TestEnvironment {
    _root: tempfile::TempDir,
    pub home: PathBuf,
    pub codex_home: PathBuf,
    pub claude_config_dir: PathBuf,
    pub opencode_config_dir: PathBuf,
    pub path: PathBuf,
}

impl TestEnvironment {
    fn create() -> Self {
        let search_path = std::env::var_os("PATH").unwrap_or_default();
        Self::create_with_git_search_path(&search_path)
    }

    fn create_with_git_search_path(git_search_path: &OsStr) -> Self {
        let root = tempfile::Builder::new()
            .prefix("dalo-test-env-")
            .tempdir()
            .expect("test environment should be created");
        let home = root.path().join("home");
        let codex_home = root.path().join("codex");
        let claude_config_dir = root.path().join("claude");
        let opencode_config_dir = root.path().join("opencode");
        let path = root.path().join("bin");
        for directory in [
            &home,
            &codex_home,
            &claude_config_dir,
            &opencode_config_dir,
            &path,
            &root.path().join("xdg/config"),
            &root.path().join("xdg/data"),
            &root.path().join("xdg/cache"),
        ] {
            std::fs::create_dir_all(directory)
                .expect("test environment directory should be created");
        }
        let programs = [
            ("bash", executable_from_path("bash")),
            ("sh", executable_from_path("sh")),
        ];
        for (name, program) in programs {
            std::os::unix::fs::symlink(program, path.join(name))
                .expect("controlled test program should be linked");
        }
        link_working_git(git_search_path, &path);
        Self {
            _root: root,
            home,
            codex_home,
            claude_config_dir,
            opencode_config_dir,
            path,
        }
    }

    fn xdg_config_home(&self) -> PathBuf {
        self.home
            .parent()
            .expect("test home has a parent")
            .join("xdg/config")
    }

    fn xdg_data_home(&self) -> PathBuf {
        self.home
            .parent()
            .expect("test home has a parent")
            .join("xdg/data")
    }

    fn xdg_cache_home(&self) -> PathBuf {
        self.home
            .parent()
            .expect("test home has a parent")
            .join("xdg/cache")
    }
}

fn executable_from_path(program: &str) -> PathBuf {
    let search_path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&search_path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{program} should be available for test fixtures"))
}

fn link_working_git(search_path: &OsStr, controlled_path: &Path) {
    let link = controlled_path.join("git");
    for candidate in std::env::split_paths(search_path)
        .map(|directory| directory.join("git"))
        .filter(|candidate| candidate.is_file() && !uses_path_selected_interpreter(candidate))
    {
        std::os::unix::fs::symlink(&candidate, &link)
            .expect("controlled git fixture should be linked");
        let works = std::process::Command::new(&link)
            .arg("--version")
            .env_clear()
            .env("PATH", controlled_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if works {
            return;
        }
        std::fs::remove_file(&link).expect("unusable git fixture should be removed");
    }
    panic!("a git executable that works with the controlled PATH should be available");
}

fn uses_path_selected_interpreter(candidate: &Path) -> bool {
    let mut prefix = [0; 128];
    std::fs::File::open(candidate)
        .and_then(|mut file| file.read(&mut prefix))
        .is_ok_and(|read| {
            let contents = &prefix[..read];
            let shebang = contents
                .split(|byte| *byte == b'\n')
                .next()
                .and_then(|line| line.strip_prefix(b"#!"))
                .map(|line| line.trim_ascii());
            shebang.is_some_and(|interpreter| interpreter.starts_with(b"/usr/bin/env"))
        })
}

pub fn dalo_command() -> DaloCommand {
    let environment = TestEnvironment::create();
    dalo_command_with_environment(environment)
}

pub fn dalo_command_with_git_search_path(git_search_path: &OsStr) -> DaloCommand {
    let environment = TestEnvironment::create_with_git_search_path(git_search_path);
    dalo_command_with_environment(environment)
}

fn dalo_command_with_environment(environment: TestEnvironment) -> DaloCommand {
    let mut command = Command::cargo_bin("dalo").expect("binary should build");
    command
        .env_remove("DALO_STORE")
        .env("HOME", &environment.home)
        .env("CODEX_HOME", &environment.codex_home)
        .env("CLAUDE_CONFIG_DIR", &environment.claude_config_dir)
        .env("OPENCODE_CONFIG_DIR", &environment.opencode_config_dir)
        .env("XDG_CONFIG_HOME", environment.xdg_config_home())
        .env("XDG_DATA_HOME", environment.xdg_data_home())
        .env("XDG_CACHE_HOME", environment.xdg_cache_home())
        .env("PATH", &environment.path);
    DaloCommand {
        command,
        environment,
    }
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
        None,
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

pub fn create_git_repo_without_skills(repo: &Path) {
    std::fs::create_dir_all(repo).expect("repo should be created");
    std::fs::write(repo.join("README.md"), "# Empty skill repository\n")
        .expect("repo readme should be written");
    init_git_repo(repo);
}

pub fn create_git_skill_repo_with_skill(repo: &Path, slot_name: &str, body: &str) {
    let skill_dir = repo.join("skills").join(slot_name);
    std::fs::create_dir_all(&skill_dir).expect("repo skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("repo skill should be written");
    init_git_repo(repo);
}

pub fn create_git_skill_repo_with_required_pair(repo: &Path) {
    for (slot_name, body) in [
        (
            "alpha",
            "---\nname: alpha\nrequires:\n  - beta\n---\n# Alpha\n",
        ),
        ("beta", "---\nname: beta\n---\n# Beta\n"),
    ] {
        let skill_dir = repo.join("skills").join(slot_name);
        std::fs::create_dir_all(&skill_dir).expect("repo skill dir should be created");
        std::fs::write(skill_dir.join("SKILL.md"), body).expect("repo skill should be written");
    }
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

    GitRevParseLogger {
        path_env: bin.into_os_string(),
        log: temp_dir.join("git-rev-parse.log"),
        real_git,
    }
}
