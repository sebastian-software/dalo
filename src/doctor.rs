//! Diagnostics for store, target, Git, and lockfile health.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::adopt;
use crate::config::UserConfig;
use crate::git;
use crate::resolver;
use crate::source::SourceKind;
use crate::store::{self, ApprovalsFile, StateFile, StorePaths};

/// Doctor report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    /// Store root.
    pub store: PathBuf,
    /// Diagnostic findings.
    pub findings: Vec<DoctorFinding>,
    /// Summary counts by severity.
    pub summary: DoctorSummary,
}

/// Count of findings by severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DoctorSummary {
    /// Error count.
    pub errors: usize,
    /// Warning count.
    pub warnings: usize,
    /// Info count.
    pub info: usize,
    /// OK count.
    pub ok: usize,
}

/// One diagnostic finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorFinding {
    /// Severity.
    pub severity: DoctorSeverity,
    /// Machine-readable code.
    pub code: DoctorCode,
    /// Human-readable message.
    pub message: String,
    /// Suggested next command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    /// Blocks normal operation.
    Error,
    /// May block a subset of workflows or deserves attention.
    Warning,
    /// Useful context.
    Info,
    /// Check passed.
    Ok,
}

/// Diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCode {
    /// Store root exists.
    StoreExists,
    /// Store root is missing.
    StoreMissing,
    /// Expected store layout exists.
    StoreLayoutOk,
    /// Expected store path is missing.
    StoreLayoutMissing,
    /// Config parses.
    ConfigOk,
    /// Config cannot be parsed.
    ConfigInvalid,
    /// State parses.
    StateOk,
    /// State cannot be parsed.
    StateInvalid,
    /// Lock parses.
    LockOk,
    /// Lock cannot be parsed.
    LockInvalid,
    /// Git executable is available.
    GitAvailable,
    /// Git executable is missing.
    GitMissing,
    /// GitHub CLI is available.
    GhAvailable,
    /// GitHub CLI is missing.
    GhMissing,
    /// GitHub CLI is authenticated.
    GhAuthenticated,
    /// GitHub CLI is not authenticated.
    GhUnauthenticated,
    /// Local source has a Git repository.
    LocalGitOk,
    /// Local source Git repository is missing.
    LocalGitMissing,
    /// Configured target exists.
    TargetExists,
    /// Configured target is missing.
    TargetMissing,
    /// Multiple logical targets share a directory.
    DuplicateTargetDirectory,
    /// Owned symlink is valid.
    OwnedSymlinkOk,
    /// Owned symlink is missing.
    MissingOwnedSymlink,
    /// Owned symlink points to a missing store path.
    BrokenOwnedSymlink,
    /// Recorded owned slot is a real entry.
    OwnedPathRealEntry,
    /// Recorded owned symlink points outside the store.
    ForeignOwnedSymlink,
    /// Unmanaged target skill blocks the same managed slot.
    UnmanagedSameNameBlocker,
    /// Source is clean.
    SourceClean,
    /// Source has local changes.
    DirtySource,
    /// A skill is pending approval.
    PendingApproval,
    /// A skill is blocked because its required closure is not linkable.
    RequiredClosureBlocked,
    /// Target path looks cloud-synced.
    CloudSyncedTarget,
}

/// Run read-only diagnostics.
pub fn run_doctor(store_root: &Path) -> DoctorReport {
    let paths = StorePaths::new(store_root.to_path_buf());
    let mut findings = Vec::new();

    check_commands(&mut findings);
    check_store_layout(&paths, &mut findings);

    let config = read_config(&paths, &mut findings);
    let state = read_state(&paths, &mut findings);
    let lock_ok = read_lock(&paths, &mut findings);

    if paths.local_dir.join(".git").is_dir() {
        findings.push(ok(
            DoctorCode::LocalGitOk,
            "local source Git repository exists",
        ));
    } else if paths.root.exists() {
        findings.push(finding_error(
            DoctorCode::LocalGitMissing,
            "local source Git repository is missing",
            Some("dalo init".to_owned()),
        ));
    }

    if let Some(state) = state.as_ref() {
        check_targets(state, &mut findings);
        check_owned_symlinks(&paths, state, &mut findings);
    }

    if let Some(config) = config.as_ref() {
        check_sources(config, &mut findings);
    }

    if let (Some(config), Some(_), true) = (config.as_ref(), state.as_ref(), lock_ok) {
        check_resolution(&paths, config, &mut findings);
    }

    findings.sort_by(|left, right| {
        severity_name(left.severity)
            .cmp(severity_name(right.severity))
            .then_with(|| code_name(left.code).cmp(code_name(right.code)))
            .then_with(|| left.message.cmp(&right.message))
    });
    let summary = summarize(&findings);

    DoctorReport {
        store: store_root.to_path_buf(),
        findings,
        summary,
    }
}

fn check_commands(findings: &mut Vec<DoctorFinding>) {
    if command_succeeds("git", &["--version"]) {
        findings.push(ok(DoctorCode::GitAvailable, "git is available"));
    } else {
        findings.push(finding_error(
            DoctorCode::GitMissing,
            "git is not available on PATH",
            None,
        ));
    }

    if command_succeeds("gh", &["--version"]) {
        findings.push(ok(DoctorCode::GhAvailable, "gh is available"));
        if command_succeeds("gh", &["auth", "status"]) {
            findings.push(ok(DoctorCode::GhAuthenticated, "gh is authenticated"));
        } else {
            findings.push(finding_warning(
                DoctorCode::GhUnauthenticated,
                "gh is not authenticated; PR flows will not work",
                Some("gh auth login".to_owned()),
            ));
        }
    } else {
        findings.push(finding_warning(
            DoctorCode::GhMissing,
            "gh is not available; normal sync works, but PR flows will not",
            None,
        ));
    }
}

fn check_store_layout(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) {
    if paths.root.is_dir() {
        findings.push(ok(DoctorCode::StoreExists, "store root exists"));
    } else {
        findings.push(finding_error(
            DoctorCode::StoreMissing,
            format!("store root `{}` does not exist", paths.root.display()),
            Some("dalo init".to_owned()),
        ));
        return;
    }

    for path in [
        &paths.config_file,
        &paths.lock_file,
        &paths.state_file,
        &paths.approvals_file,
        &paths.local_dir,
        &paths.local_skills_dir,
        &paths.sources_dir,
    ] {
        if path.exists() {
            findings.push(ok(
                DoctorCode::StoreLayoutOk,
                format!("expected store path exists: `{}`", path.display()),
            ));
        } else {
            findings.push(finding_error(
                DoctorCode::StoreLayoutMissing,
                format!("expected store path is missing: `{}`", path.display()),
                Some("dalo init".to_owned()),
            ));
        }
    }
}

fn read_config(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> Option<UserConfig> {
    match store::read_config(paths) {
        Ok(config) => {
            findings.push(ok(DoctorCode::ConfigOk, "config parses"));
            Some(config)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::ConfigInvalid,
                format!("config could not be read: {error}"),
                Some("dalo init".to_owned()),
            ));
            None
        }
    }
}

fn read_state(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> Option<StateFile> {
    match store::read_state(paths) {
        Ok(state) => {
            findings.push(ok(DoctorCode::StateOk, "state parses"));
            Some(state)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::StateInvalid,
                format!("state could not be read: {error}"),
                Some("dalo init".to_owned()),
            ));
            None
        }
    }
}

fn read_lock(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> bool {
    match store::read_user_lock(paths) {
        Ok(_) => {
            findings.push(ok(DoctorCode::LockOk, "user lock parses"));
            true
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::LockInvalid,
                format!("user lock could not be read: {error}"),
                Some("dalo sync".to_owned()),
            ));
            false
        }
    }
}

fn check_targets(state: &StateFile, findings: &mut Vec<DoctorFinding>) {
    for target in state.targets.iter().filter(|target| target.enabled) {
        if target.path.is_dir() {
            findings.push(ok(
                DoctorCode::TargetExists,
                format!(
                    "target `{}` exists at `{}`",
                    target.id,
                    target.path.display()
                ),
            ));
        } else {
            findings.push(finding_warning(
                DoctorCode::TargetMissing,
                format!(
                    "target `{}` is configured but `{}` is missing",
                    target.id,
                    target.path.display()
                ),
                Some(format!("dalo target link {}", target.id)),
            ));
        }

        if looks_cloud_synced(&target.path) {
            findings.push(finding_warning(
                DoctorCode::CloudSyncedTarget,
                format!(
                    "target `{}` appears to be inside a cloud-synced folder: `{}`",
                    target.id,
                    target.path.display()
                ),
                None,
            ));
        }
    }

    for dir in &state.materialization_dirs {
        if dir.logical_targets.len() > 1 {
            findings.push(info(
                DoctorCode::DuplicateTargetDirectory,
                format!(
                    "targets share `{}`: {}",
                    dir.path.display(),
                    dir.logical_targets.join(", ")
                ),
            ));
        }
    }
}

fn check_owned_symlinks(paths: &StorePaths, state: &StateFile, findings: &mut Vec<DoctorFinding>) {
    for owned in &state.owned_skills {
        match fs::symlink_metadata(&owned.link_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                match fs::read_link(&owned.link_path) {
                    Ok(target) if !target.starts_with(&paths.root) => {
                        findings.push(finding_error(
                            DoctorCode::ForeignOwnedSymlink,
                            format!(
                                "owned symlink `{}` points outside the store to `{}`",
                                owned.link_path.display(),
                                target.display()
                            ),
                            Some(format!("dalo resolve remove-owned {}", owned.slot_name)),
                        ));
                    }
                    Ok(target) if !target.exists() => {
                        findings.push(finding_error(
                            DoctorCode::BrokenOwnedSymlink,
                            format!(
                                "owned symlink `{}` points to missing `{}`",
                                owned.link_path.display(),
                                target.display()
                            ),
                            Some(format!("dalo resolve remove-owned {}", owned.slot_name)),
                        ));
                    }
                    Ok(_) => findings.push(ok(
                        DoctorCode::OwnedSymlinkOk,
                        format!("owned symlink `{}` is valid", owned.link_path.display()),
                    )),
                    Err(error) => findings.push(finding_error(
                        DoctorCode::BrokenOwnedSymlink,
                        format!(
                            "owned symlink `{}` could not be read: {error}",
                            owned.link_path.display()
                        ),
                        Some(format!("dalo resolve remove-owned {}", owned.slot_name)),
                    )),
                }
            }
            Ok(_) => findings.push(finding_error(
                DoctorCode::OwnedPathRealEntry,
                format!(
                    "recorded owned path `{}` is a real entry, not a symlink",
                    owned.link_path.display()
                ),
                Some(format!("dalo resolve remove-owned {}", owned.slot_name)),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                findings.push(finding_warning(
                    DoctorCode::MissingOwnedSymlink,
                    format!(
                        "recorded owned symlink `{}` is missing",
                        owned.link_path.display()
                    ),
                    Some(format!("dalo resolve remove-owned {}", owned.slot_name)),
                ));
            }
            Err(error) => findings.push(finding_error(
                DoctorCode::BrokenOwnedSymlink,
                format!(
                    "recorded owned symlink `{}` could not be inspected: {error}",
                    owned.link_path.display()
                ),
                Some(format!("dalo resolve remove-owned {}", owned.slot_name)),
            )),
        }
    }
}

fn check_sources(config: &UserConfig, findings: &mut Vec<DoctorFinding>) {
    for source in config.sources.iter().filter(|source| source.enabled) {
        match git::is_dirty(&source.path) {
            Ok(true) => {
                let severity = if source.kind == SourceKind::Team {
                    DoctorSeverity::Error
                } else {
                    DoctorSeverity::Warning
                };
                findings.push(DoctorFinding {
                    severity,
                    code: DoctorCode::DirtySource,
                    message: format!("source `{}` has local changes", source.id),
                    next_command: Some("git status".to_owned()),
                });
            }
            Ok(false) => findings.push(ok(
                DoctorCode::SourceClean,
                format!("source `{}` is clean", source.id),
            )),
            Err(error) => findings.push(finding_warning(
                DoctorCode::DirtySource,
                format!(
                    "source `{}` dirty state could not be checked: {error}",
                    source.id
                ),
                None,
            )),
        }
    }
}

fn check_resolution(paths: &StorePaths, config: &UserConfig, findings: &mut Vec<DoctorFinding>) {
    let approvals = store::read_approvals(paths).unwrap_or_else(|_| ApprovalsFile::empty());
    let resolution = resolver::resolve_from_config(config, approvals.approvals).resolution;

    for skill in &resolution.pending_approval_skills {
        findings.push(finding_warning(
            DoctorCode::PendingApproval,
            format!("skill `{}` is pending approval", skill.source_ref),
            Some("dalo status".to_owned()),
        ));
    }

    for blocked in &resolution.blocked_skills {
        findings.push(finding_warning(
            DoctorCode::RequiredClosureBlocked,
            format!(
                "skill `{}` is blocked: requirement `{}` is {}",
                blocked.skill.source_ref,
                blocked.requirement,
                resolver::closure_block_reason_name(blocked.reason)
            ),
            Some("dalo status".to_owned()),
        ));
    }

    let active_slots = resolution
        .active_skills
        .iter()
        .map(|skill| (skill.slot_name.as_str(), skill.source_ref.as_str()))
        .collect::<BTreeMap<_, _>>();
    if let Ok(unmanaged_skills) = adopt::discover_unmanaged_skills(paths) {
        for unmanaged in unmanaged_skills {
            if let Some(source_ref) = active_slots.get(unmanaged.slot_name.as_str()) {
                findings.push(finding_error(
                    DoctorCode::UnmanagedSameNameBlocker,
                    format!(
                        "unmanaged skill `{}` blocks managed `{}`",
                        unmanaged.path.display(),
                        source_ref
                    ),
                    Some(format!("dalo adopt {}", unmanaged.id)),
                ));
            }
        }
    }
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn looks_cloud_synced(path: &Path) -> bool {
    let value = path.to_string_lossy();
    ["Dropbox", "Google Drive", "iCloud Drive", "OneDrive"]
        .iter()
        .any(|marker| value.contains(marker))
}

fn summarize(findings: &[DoctorFinding]) -> DoctorSummary {
    let mut summary = DoctorSummary::default();
    for finding in findings {
        match finding.severity {
            DoctorSeverity::Error => summary.errors += 1,
            DoctorSeverity::Warning => summary.warnings += 1,
            DoctorSeverity::Info => summary.info += 1,
            DoctorSeverity::Ok => summary.ok += 1,
        }
    }
    summary
}

fn ok(code: DoctorCode, message: impl Into<String>) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Ok,
        code,
        message: message.into(),
        next_command: None,
    }
}

fn info(code: DoctorCode, message: impl Into<String>) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Info,
        code,
        message: message.into(),
        next_command: None,
    }
}

fn finding_warning(
    code: DoctorCode,
    message: impl Into<String>,
    next_command: Option<String>,
) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code,
        message: message.into(),
        next_command,
    }
}

fn finding_error(
    code: DoctorCode,
    message: impl Into<String>,
    next_command: Option<String>,
) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Error,
        code,
        message: message.into(),
        next_command,
    }
}

fn severity_name(severity: DoctorSeverity) -> &'static str {
    match severity {
        DoctorSeverity::Error => "0_error",
        DoctorSeverity::Warning => "1_warning",
        DoctorSeverity::Info => "2_info",
        DoctorSeverity::Ok => "3_ok",
    }
}

fn code_name(code: DoctorCode) -> &'static str {
    match code {
        DoctorCode::StoreExists => "store_exists",
        DoctorCode::StoreMissing => "store_missing",
        DoctorCode::StoreLayoutOk => "store_layout_ok",
        DoctorCode::StoreLayoutMissing => "store_layout_missing",
        DoctorCode::ConfigOk => "config_ok",
        DoctorCode::ConfigInvalid => "config_invalid",
        DoctorCode::StateOk => "state_ok",
        DoctorCode::StateInvalid => "state_invalid",
        DoctorCode::LockOk => "lock_ok",
        DoctorCode::LockInvalid => "lock_invalid",
        DoctorCode::GitAvailable => "git_available",
        DoctorCode::GitMissing => "git_missing",
        DoctorCode::GhAvailable => "gh_available",
        DoctorCode::GhMissing => "gh_missing",
        DoctorCode::GhAuthenticated => "gh_authenticated",
        DoctorCode::GhUnauthenticated => "gh_unauthenticated",
        DoctorCode::LocalGitOk => "local_git_ok",
        DoctorCode::LocalGitMissing => "local_git_missing",
        DoctorCode::TargetExists => "target_exists",
        DoctorCode::TargetMissing => "target_missing",
        DoctorCode::DuplicateTargetDirectory => "duplicate_target_directory",
        DoctorCode::OwnedSymlinkOk => "owned_symlink_ok",
        DoctorCode::MissingOwnedSymlink => "missing_owned_symlink",
        DoctorCode::BrokenOwnedSymlink => "broken_owned_symlink",
        DoctorCode::OwnedPathRealEntry => "owned_path_real_entry",
        DoctorCode::ForeignOwnedSymlink => "foreign_owned_symlink",
        DoctorCode::UnmanagedSameNameBlocker => "unmanaged_same_name_blocker",
        DoctorCode::SourceClean => "source_clean",
        DoctorCode::DirtySource => "dirty_source",
        DoctorCode::PendingApproval => "pending_approval",
        DoctorCode::RequiredClosureBlocked => "required_closure_blocked",
        DoctorCode::CloudSyncedTarget => "cloud_synced_target",
    }
}

impl std::fmt::Display for DoctorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(code_name(*self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MaterializationDirState, OwnedSkillState, TargetState};

    #[test]
    fn run_doctor_should_report_missing_store_without_creating_it() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("missing-store");

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::StoreMissing)
        );
        assert!(!store.exists());
    }

    #[test]
    fn run_doctor_should_report_broken_owned_symlink() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        store::init_store(store.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target).expect("target should be created");
        let link = target.join("review");
        std::os::unix::fs::symlink(store.join("local/skills/missing"), &link)
            .expect("broken symlink should be created");
        write_state(&store, &target, &link, &store.join("local/skills/missing"));

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::BrokenOwnedSymlink)
        );
    }

    #[test]
    fn check_sources_should_rate_dirty_team_source_as_error() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let team_repo = temp_dir.path().join("team-repo");
        create_dirty_git_repo(&team_repo);
        write_config_with_dirty_sources(&store, &team_repo, None);

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::DirtySource
                && finding.severity == DoctorSeverity::Error
                && finding.message.contains("`team`")
        }));
    }

    #[test]
    fn check_sources_should_rate_dirty_local_source_as_warning() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let team_repo = temp_dir.path().join("team-repo");
        let local_repo = temp_dir.path().join("local-repo");
        create_dirty_git_repo(&team_repo);
        create_dirty_git_repo(&local_repo);
        write_config_with_dirty_sources(&store, &team_repo, Some(&local_repo));

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::DirtySource
                && finding.severity == DoctorSeverity::Warning
                && finding.message.contains("`workspace`")
        }));
    }

    fn create_dirty_git_repo(repo: &Path) {
        fs::create_dir_all(repo).expect("repo dir should be created");
        run_git(repo, &["init", "-q"]);
        fs::write(repo.join("README.md"), "tracked\n").expect("tracked file should be written");
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
        fs::write(repo.join("README.md"), "dirty\n").expect("repo should be dirtied");
    }

    fn write_config_with_dirty_sources(store: &Path, team_repo: &Path, local_repo: Option<&Path>) {
        use crate::config::{Settings, UserConfig};
        use crate::source::{SourceConfig, SourceKind};

        let paths = StorePaths::new(store.to_path_buf());
        let mut sources = vec![SourceConfig {
            id: "team".to_owned(),
            kind: SourceKind::Team,
            path: team_repo.to_path_buf(),
            priority: 10,
            enabled: true,
            trusted: true,
            url: None,
            branch: None,
            update_policy: None,
            selection: Vec::new(),
        }];
        if let Some(local_repo) = local_repo {
            sources.push(SourceConfig {
                id: "workspace".to_owned(),
                kind: SourceKind::Local,
                path: local_repo.to_path_buf(),
                priority: 0,
                enabled: true,
                trusted: true,
                url: None,
                branch: None,
                update_policy: None,
                selection: Vec::new(),
            });
        }
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources,
        };
        store::write_config(&paths, &config).expect("config should be written");
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git should run");
        assert!(status.success());
    }

    fn write_state(store: &Path, target: &Path, link: &Path, store_path: &Path) {
        let paths = StorePaths::new(store.to_path_buf());
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.targets = vec![TargetState {
            id: "generic".to_owned(),
            path: target.to_path_buf(),
            canonical_path: target.to_path_buf(),
            enabled: true,
        }];
        state.materialization_dirs = vec![MaterializationDirState {
            path: target.to_path_buf(),
            logical_targets: vec!["generic".to_owned()],
        }];
        state.owned_skills = vec![OwnedSkillState {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link.to_path_buf(),
            store_path: store_path.to_path_buf(),
        }];
        store::write_state(&paths, &state).expect("state should be written");
    }
}
