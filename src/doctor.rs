//! Diagnostics for store, target, Git, and lockfile health.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::adopt;
use crate::autosync::{self, AutosyncRunOutcome};
use crate::catalog::{self, SourceLock};
use crate::config::UserConfig;
use crate::error::shell_quote_path;
use crate::git;
use crate::instructions;
use crate::resolver;
use crate::source::{self, SourceConfig, SourceKind};
use crate::store::{self, ApprovalsFile, OwnedSkillState, StateFile, StorePaths};

const COMMAND_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_CHECK_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
    /// Catalog source lock is present and parses.
    SourceLockOk,
    /// Catalog source lock cannot be parsed.
    SourceLockInvalid,
    /// Approvals file parses.
    ApprovalsOk,
    /// Approvals file cannot be parsed.
    ApprovalsInvalid,
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
    /// A protected target slot is intentionally kept unmanaged.
    ProtectedSkillKept,
    /// A protection record no longer maps to an existing target slot.
    StaleProtectedSkill,
    /// A linked target directory could not be scanned for unmanaged skills.
    UnreadableTargetDirectory,
    /// Source is clean.
    SourceClean,
    /// Source has local changes.
    DirtySource,
    /// Manifest-derived source provenance is internally consistent.
    SourceProvenanceOk,
    /// Manifest declaration, checkout, or source lock disagree.
    SourceProvenanceMismatch,
    /// Store contains checkout or staging content not owned by config.
    SourceStoreDebris,
    /// A skill is pending approval.
    PendingApproval,
    /// A skill is blocked because its required closure is not linkable.
    RequiredClosureBlocked,
    /// Two active instruction packs declare overlapping topics.
    InstructionPackTopicOverlap,
    /// An active instruction pack's rendered block is missing, malformed, or stale.
    InstructionBlockDrift,
    /// Target path looks cloud-synced.
    CloudSyncedTarget,
    /// Scheduled synchronization is installed and enabled.
    AutosyncInstalled,
    /// Scheduled synchronization is not installed.
    AutosyncNotInstalled,
    /// Scheduler metadata exists but the native job is disabled.
    AutosyncDisabled,
    /// The executable recorded in scheduler metadata is unavailable.
    AutosyncExecutableMissing,
    /// The latest scheduled synchronization was blocked.
    AutosyncRunBlocked,
    /// Autosync metadata or native scheduler state could not be inspected.
    AutosyncStateInvalid,
}

/// Run read-only diagnostics.
pub fn run_doctor(store_root: &Path) -> DoctorReport {
    let paths = StorePaths::new(store_root.to_path_buf());
    let mut findings = Vec::new();

    check_store_layout(&paths, &mut findings);
    if !paths.root.is_dir() {
        return finish_report(store_root, findings);
    }
    check_commands(&mut findings);

    let config = read_config(&paths, &mut findings);
    let state = read_state(&paths, &mut findings);
    let lock_ok = read_lock(&paths, &mut findings);
    let source_lock = read_source_lock(&paths, &mut findings);
    let approvals = read_approvals(&paths, &mut findings);

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
        check_protected_skills(state, &mut findings);
    }

    if let Some(config) = config.as_ref() {
        check_sources(config, &source_lock, &mut findings);
        check_source_store_debris(&paths, config, &mut findings);
    }

    if let (Some(config), Some(_), true) = (config.as_ref(), state.as_ref(), lock_ok) {
        check_resolution(&paths, config, approvals.as_ref(), &mut findings);
    }
    check_autosync(&paths, &mut findings);

    finish_report(store_root, findings)
}

fn finish_report(store_root: &Path, mut findings: Vec<DoctorFinding>) -> DoctorReport {
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

fn check_autosync(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) {
    match autosync::status(paths) {
        Ok(status) => {
            if let Some(error) = &status.scheduler_error {
                findings.push(finding_error(
                    DoctorCode::AutosyncStateInvalid,
                    format!("native autosync scheduler could not be inspected: {error}"),
                    Some("dalo autosync status".to_owned()),
                ));
            }
            if !status.installed && status.configured {
                findings.push(finding_warning(
                    DoctorCode::AutosyncDisabled,
                    "config enables autosync, but scheduler installation metadata is missing",
                    Some("dalo autosync install".to_owned()),
                ));
            } else if !status.installed {
                findings.push(info(
                    DoctorCode::AutosyncNotInstalled,
                    "scheduled synchronization is not installed",
                ));
            } else if let Some(executable) = status
                .executable
                .as_ref()
                .filter(|path| !autosync::executable_available(path))
            {
                findings.push(finding_warning(
                    DoctorCode::AutosyncExecutableMissing,
                    format!(
                        "recorded autosync executable `{}` is missing or not executable",
                        executable.display()
                    ),
                    Some("dalo autosync install".to_owned()),
                ));
            } else if status.enabled && status.configured {
                findings.push(ok(
                    DoctorCode::AutosyncInstalled,
                    format!(
                        "scheduled synchronization is enabled via {} ({})",
                        status.backend.map_or("unknown", |backend| backend.as_str()),
                        status
                            .schedule
                            .map_or("unknown", |schedule| schedule.as_str())
                    ),
                ));
            } else {
                findings.push(finding_warning(
                    DoctorCode::AutosyncDisabled,
                    "autosync config, metadata, and native scheduler state are inconsistent",
                    Some("dalo autosync install".to_owned()),
                ));
            }
            if let Some(run) = status.last_run
                && run.outcome == AutosyncRunOutcome::Blocked
            {
                findings.push(finding_warning(
                    DoctorCode::AutosyncRunBlocked,
                    format!(
                        "latest scheduled synchronization was blocked: {}",
                        run.reason.as_deref().unwrap_or("no reason recorded")
                    ),
                    Some("dalo autosync status".to_owned()),
                ));
            }
        }
        Err(error) => findings.push(finding_error(
            DoctorCode::AutosyncStateInvalid,
            format!("autosync state could not be inspected: {error}"),
            Some("dalo autosync uninstall".to_owned()),
        )),
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
                Some(format!("$EDITOR {}", shell_quote_path(&paths.config_file))),
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

fn read_approvals(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> Option<ApprovalsFile> {
    match store::read_approvals(paths) {
        Ok(approvals) => {
            findings.push(ok(DoctorCode::ApprovalsOk, "approvals parse"));
            Some(approvals)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::ApprovalsInvalid,
                format!("approvals could not be read: {error}"),
                Some("inspect or restore approvals.toml".to_owned()),
            ));
            None
        }
    }
}

enum SourceLockRead {
    Missing,
    Readable(SourceLock),
    Invalid,
}

impl SourceLockRead {
    fn lock(&self) -> Option<&SourceLock> {
        match self {
            Self::Readable(lock) => Some(lock),
            Self::Missing | Self::Invalid => None,
        }
    }

    fn can_check_provenance(&self) -> bool {
        !matches!(self, Self::Invalid)
    }
}

fn read_source_lock(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> SourceLockRead {
    if !paths.source_lock_file.exists() {
        return SourceLockRead::Missing;
    }

    match catalog::read_source_lock(paths) {
        Ok(lock) => {
            findings.push(ok(
                DoctorCode::SourceLockOk,
                "catalog source lock is present and readable",
            ));
            SourceLockRead::Readable(lock)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::SourceLockInvalid,
                format!("catalog source lock could not be read: {error}"),
                Some("inspect source-lock.toml before syncing".to_owned()),
            ));
            SourceLockRead::Invalid
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
                    Ok(target)
                        if !store::path_is_same_or_descendant(
                            &store::resolve_link_target(&owned.link_path, &target),
                            &paths.root,
                        ) =>
                    {
                        findings.push(finding_error(
                            DoctorCode::ForeignOwnedSymlink,
                            format!(
                                "owned symlink `{}` points outside the store to `{}`",
                                owned.link_path.display(),
                                target.display()
                            ),
                            Some(format!(
                                "dalo resolve remove-owned {}",
                                owned_selector(owned)
                            )),
                        ));
                    }
                    Ok(target)
                        if !store::resolve_link_target(&owned.link_path, &target).exists() =>
                    {
                        findings.push(finding_error(
                            DoctorCode::BrokenOwnedSymlink,
                            format!(
                                "owned symlink `{}` points to missing `{}`",
                                owned.link_path.display(),
                                target.display()
                            ),
                            Some(format!(
                                "dalo resolve remove-owned {}",
                                owned_selector(owned)
                            )),
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
                        Some(format!(
                            "dalo resolve remove-owned {}",
                            owned_selector(owned)
                        )),
                    )),
                }
            }
            Ok(_) => findings.push(finding_error(
                DoctorCode::OwnedPathRealEntry,
                format!(
                    "recorded owned path `{}` is a real entry, not a symlink",
                    owned.link_path.display()
                ),
                Some(format!(
                    "dalo resolve remove-owned {}",
                    owned_selector(owned)
                )),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                findings.push(finding_warning(
                    DoctorCode::MissingOwnedSymlink,
                    format!(
                        "recorded owned symlink `{}` is missing",
                        owned.link_path.display()
                    ),
                    Some(format!(
                        "dalo resolve remove-owned {}",
                        owned_selector(owned)
                    )),
                ));
            }
            Err(error) => findings.push(finding_error(
                DoctorCode::BrokenOwnedSymlink,
                format!(
                    "recorded owned symlink `{}` could not be inspected: {error}",
                    owned.link_path.display()
                ),
                Some(format!(
                    "dalo resolve remove-owned {}",
                    owned_selector(owned)
                )),
            )),
        }
    }
}

fn check_protected_skills(state: &StateFile, findings: &mut Vec<DoctorFinding>) {
    for protected in &state.protected_skills {
        let target = state
            .targets
            .iter()
            .find(|target| target.id == protected.target_id && target.enabled);
        let path = target
            .map(|target| target.canonical_path.join(&protected.slot_name))
            .or_else(|| protected.path.clone());
        let selector = if protected.target_id.is_empty() {
            protected.slot_name.clone()
        } else {
            format!("{}:{}", protected.target_id, protected.slot_name)
        };
        if target.is_none() || path.as_ref().is_none_or(|path| !path.is_dir()) {
            findings.push(finding_warning(
                DoctorCode::StaleProtectedSkill,
                format!(
                    "protected slot `{selector}` no longer maps to an existing target directory{}",
                    path.as_ref()
                        .map_or_else(String::new, |path| format!(" at `{}`", path.display()))
                ),
                Some(format!("dalo resolve unkeep {selector}")),
            ));
        } else {
            findings.push(info(
                DoctorCode::ProtectedSkillKept,
                format!(
                    "protected slot `{selector}` is kept at `{}`",
                    path.expect("existing protected path should be present")
                        .display()
                ),
            ));
        }
    }
}

fn owned_selector(owned: &OwnedSkillState) -> String {
    format!("{}:{}", owned.target_id, owned.slot_name)
}

fn check_sources(
    config: &UserConfig,
    source_lock: &SourceLockRead,
    findings: &mut Vec<DoctorFinding>,
) {
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
                    message: if source.kind == SourceKind::Local {
                        format!(
                            "source `{}` at `{}` has local changes; adopted skills must be committed before syncing",
                            source.id,
                            source.path.display()
                        )
                    } else {
                        format!(
                            "source `{}` at `{}` has local changes; resolve or commit them before syncing",
                            source.id,
                            source.path.display()
                        )
                    },
                    next_command: Some(format!(
                        "git -C {} status",
                        shell_quote_path(&source.path)
                    )),
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
        if source.declared_by.is_some() && source_lock.can_check_provenance() {
            check_manifest_source_provenance(source, config, source_lock.lock(), findings);
        }
    }
}

fn check_manifest_source_provenance(
    source: &SourceConfig,
    config: &UserConfig,
    source_lock: Option<&SourceLock>,
    findings: &mut Vec<DoctorFinding>,
) {
    let Some(team_id) = source.declared_by.as_deref() else {
        return;
    };
    let mut mismatches = Vec::new();
    let lock_commit = source_lock
        .and_then(|lock| lock.catalog(&source.id))
        .map(|entry| entry.commit.as_str());
    let checkout_commit = git::rev_parse_head(&source.path).ok();
    match (lock_commit, checkout_commit.as_deref()) {
        (None, _) => mismatches.push("source-lock.toml has no catalog pin".to_owned()),
        (Some(pin), Some(checkout)) if pin != checkout => mismatches.push(format!(
            "checkout {} does not match source-lock pin {}",
            short_commit(checkout),
            short_commit(pin)
        )),
        (Some(_), None) => mismatches.push("checkout commit could not be read".to_owned()),
        _ => {}
    }

    let team = config
        .sources
        .iter()
        .find(|candidate| candidate.id == team_id);
    if let Some(team) = team {
        match crate::team_manifest::load_team_manifest(&team.path, team_id) {
            Ok(manifest) => {
                let declaration = manifest.catalogs.iter().find(|catalog| {
                    crate::team_manifest::source_id_matches_declaration(
                        &source.id,
                        team_id,
                        &catalog.id,
                    )
                });
                if let Some(declaration) = declaration {
                    let expected_url =
                        source::resolve_source_location(&declaration.url, &team.path);
                    if source.url.as_deref() != Some(expected_url.as_str()) {
                        mismatches
                            .push("manifest origin does not match configured origin".to_owned());
                    }
                    if source.declared_ref.as_deref() != Some(declaration.version.as_str()) {
                        mismatches
                            .push("manifest version does not match configured version".to_owned());
                    }
                } else {
                    mismatches.push("declaring team manifest has no matching catalog".to_owned());
                }
            }
            Err(error) => mismatches.push(format!("declaring team manifest is invalid: {error}")),
        }
    } else {
        mismatches.push(format!("declaring team source `{team_id}` is missing"));
    }

    if mismatches.is_empty() {
        let provenance = source::source_provenance(source, source_lock);
        let origin = provenance.origin_url.as_deref().unwrap_or("<unknown>");
        let requested = provenance.requested_ref.as_deref().unwrap_or("<unknown>");
        let resolved = provenance
            .resolved_commit
            .as_deref()
            .map(short_commit)
            .unwrap_or("<missing>");
        findings.push(ok(
            DoctorCode::SourceProvenanceOk,
            format!(
                "manifest-derived source `{}` from `{origin}` requested `{requested}` and resolved `{resolved}`",
                source.id
            ),
        ));
    } else {
        findings.push(finding_error(
            DoctorCode::SourceProvenanceMismatch,
            format!(
                "manifest-derived source `{}` has provenance mismatch: {}",
                source.id,
                mismatches.join("; ")
            ),
            Some("dalo sync".to_owned()),
        ));
    }
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn check_source_store_debris(
    paths: &StorePaths,
    config: &UserConfig,
    findings: &mut Vec<DoctorFinding>,
) {
    let configured = config
        .sources
        .iter()
        .filter(|source| source.kind != SourceKind::Local)
        .map(|source| source.id.as_str())
        .collect::<BTreeSet<_>>();
    let Ok(source_dirs) = fs::read_dir(&paths.sources_dir) else {
        return;
    };

    for entry in source_dirs.flatten() {
        let source_id = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !configured.contains(source_id.as_str()) {
            findings.push(finding_warning(
                DoctorCode::SourceStoreDebris,
                format!("unconfigured source content exists at `{}`", path.display()),
                Some(format!("inspect or remove {}", path.display())),
            ));
            continue;
        }

        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let name = child.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".checkout-tmp-") || name == "checkout.dalo-removing" {
                findings.push(finding_warning(
                    DoctorCode::SourceStoreDebris,
                    format!(
                        "interrupted source-operation debris exists at `{}`",
                        child.path().display()
                    ),
                    Some(format!("inspect or remove {}", child.path().display())),
                ));
            }
        }
    }
}

fn check_resolution(
    paths: &StorePaths,
    config: &UserConfig,
    approvals: Option<&ApprovalsFile>,
    findings: &mut Vec<DoctorFinding>,
) {
    let approval_records = approvals
        .map(|approvals| approvals.approvals.clone())
        .unwrap_or_default();
    let resolution = resolver::resolve_from_config(config, approval_records).resolution;

    if approvals.is_some() {
        for diagnostic in &resolution.diagnostics {
            if diagnostic.code == resolver::ResolutionDiagnosticCode::LegacyBareApproval {
                findings.push(finding_warning(
                    DoctorCode::PendingApproval,
                    diagnostic.message.clone(),
                    Some("dalo status".to_owned()),
                ));
            }
        }
        for skill in &resolution.pending_approval_skills {
            findings.push(finding_warning(
                DoctorCode::PendingApproval,
                format!("skill `{}` is pending approval", skill.source_ref),
                Some(format!("dalo approve skill {}", skill.source_ref)),
            ));
        }
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

    let lock = store::read_user_lock(paths).unwrap_or_default();
    let discovered =
        instructions::discover_packs(paths, &config.sources, &lock.active_instruction_packs);
    let active_packs = discovered
        .into_iter()
        .filter(|pack| pack.enabled)
        .collect::<Vec<_>>();
    for overlap in instructions::topic_overlaps(&active_packs) {
        findings.push(finding_warning(
            DoctorCode::InstructionPackTopicOverlap,
            format!(
                "instruction packs `{}` and `{}` overlap on topics: {}",
                overlap.packs[0],
                overlap.packs[1],
                overlap.topics.join(", ")
            ),
            Some("dalo status".to_owned()),
        ));
    }
    for drift in instructions::instruction_block_drifts(
        paths,
        &config.sources,
        &lock.active_instruction_packs,
    ) {
        findings.push(finding_warning(
            DoctorCode::InstructionBlockDrift,
            format!(
                "instruction pack `{}:{}` is {} at `{}`: {}",
                drift.source_id,
                drift.pack_id,
                instruction_block_drift_kind_name(drift.kind),
                drift.target.display(),
                drift.message
            ),
            Some(format!(
                "dalo instructions enable {} {}",
                drift.pack_id,
                drift.target.display()
            )),
        ));
    }

    let active_slots = resolution
        .active_skills
        .iter()
        .map(|skill| (skill.slot_name.as_str(), skill.source_ref.as_str()))
        .collect::<BTreeMap<_, _>>();
    if let Ok(unmanaged_scan) = adopt::discover_unmanaged_skill_scan(paths) {
        for warning in unmanaged_scan.warnings {
            findings.push(finding_warning(
                DoctorCode::UnreadableTargetDirectory,
                format!(
                    "target path `{}` could not be scanned: {}",
                    warning.path.display(),
                    warning.message
                ),
                None,
            ));
        }
        for unmanaged in unmanaged_scan.unmanaged_skills {
            if unmanaged.protected {
                continue;
            }
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
    command_succeeds_with_timeout(program, args, COMMAND_CHECK_TIMEOUT)
}

fn command_succeeds_with_timeout(program: &str, args: &[&str], timeout: Duration) -> bool {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {}
            Err(_) => return false,
        }

        let elapsed = start.elapsed();
        if elapsed >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }

        thread::sleep(COMMAND_CHECK_POLL_INTERVAL.min(timeout - elapsed));
    }
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
        DoctorCode::SourceLockOk => "source_lock_ok",
        DoctorCode::SourceLockInvalid => "source_lock_invalid",
        DoctorCode::ApprovalsOk => "approvals_ok",
        DoctorCode::ApprovalsInvalid => "approvals_invalid",
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
        DoctorCode::ProtectedSkillKept => "protected_skill_kept",
        DoctorCode::StaleProtectedSkill => "stale_protected_skill",
        DoctorCode::UnreadableTargetDirectory => "unreadable_target_directory",
        DoctorCode::SourceClean => "source_clean",
        DoctorCode::DirtySource => "dirty_source",
        DoctorCode::SourceProvenanceOk => "source_provenance_ok",
        DoctorCode::SourceProvenanceMismatch => "source_provenance_mismatch",
        DoctorCode::SourceStoreDebris => "source_store_debris",
        DoctorCode::PendingApproval => "pending_approval",
        DoctorCode::RequiredClosureBlocked => "required_closure_blocked",
        DoctorCode::InstructionPackTopicOverlap => "instruction_pack_topic_overlap",
        DoctorCode::InstructionBlockDrift => "instruction_block_drift",
        DoctorCode::CloudSyncedTarget => "cloud_synced_target",
        DoctorCode::AutosyncInstalled => "autosync_installed",
        DoctorCode::AutosyncNotInstalled => "autosync_not_installed",
        DoctorCode::AutosyncDisabled => "autosync_disabled",
        DoctorCode::AutosyncExecutableMissing => "autosync_executable_missing",
        DoctorCode::AutosyncRunBlocked => "autosync_run_blocked",
        DoctorCode::AutosyncStateInvalid => "autosync_state_invalid",
    }
}

fn instruction_block_drift_kind_name(
    kind: instructions::InstructionBlockDriftKind,
) -> &'static str {
    match kind {
        instructions::InstructionBlockDriftKind::Missing => "missing",
        instructions::InstructionBlockDriftKind::Malformed => "malformed",
        instructions::InstructionBlockDriftKind::Stale => "stale",
        instructions::InstructionBlockDriftKind::SourceMissing => "source-missing",
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
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn run_doctor_should_report_missing_store_without_creating_it() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("missing-store");

        let report = run_doctor(&store);

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].code, DoctorCode::StoreMissing);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Error);
        assert_eq!(
            report.findings[0].next_command.as_deref(),
            Some("dalo init")
        );
        assert_eq!(
            report.summary,
            DoctorSummary {
                errors: 1,
                warnings: 0,
                info: 0,
                ok: 0,
            }
        );
        assert!(!store.exists());
    }

    #[test]
    fn read_source_lock_should_not_report_a_missing_file_as_readable() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let paths = StorePaths::new(temp_dir.path().join("store"));
        let mut findings = Vec::new();

        let source_lock = read_source_lock(&paths, &mut findings);

        assert!(matches!(source_lock, SourceLockRead::Missing));
        assert!(!findings.iter().any(|finding| {
            matches!(
                finding.code,
                DoctorCode::SourceLockOk | DoctorCode::SourceLockInvalid
            )
        }));
    }

    #[test]
    fn doctor_should_point_invalid_autosync_state_to_recovery_uninstall() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        fs::write(store.join("autosync.toml"), "not = [valid toml")
            .expect("autosync state should be corrupted");

        let report = run_doctor(&store);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == DoctorCode::AutosyncStateInvalid)
            .expect("invalid autosync state should be reported");

        assert_eq!(
            finding.next_command.as_deref(),
            Some("dalo autosync uninstall")
        );
    }

    #[test]
    fn doctor_should_name_the_missing_recorded_autosync_executable() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let paths = StorePaths::new(store);
        let missing_executable = temp_dir.path().join("removed/dalo");
        let state = crate::autosync::AutosyncInstallState {
            schema_version: 1,
            backend: crate::autosync::SchedulerBackend::Cron,
            schedule: crate::autosync::AutosyncSchedule::Daily,
            executable: missing_executable.clone(),
            store: paths.root.clone(),
            identifier: "dalo-autosync-test".to_owned(),
            artifacts: vec!["crontab".to_owned()],
            installed_at_unix: 1,
        };
        fs::write(
            &paths.autosync_file,
            toml::to_string(&state).expect("autosync state should serialize"),
        )
        .expect("autosync state should be written");
        let mut config = store::read_config(&paths).expect("config should parse");
        config.settings.autosync = true;
        config.settings.sync_interval = Some("daily".to_owned());
        store::write_config(&paths, &config).expect("config should be written");
        let mut findings = Vec::new();

        check_autosync(&paths, &mut findings);

        let finding = findings
            .iter()
            .find(|finding| finding.code == DoctorCode::AutosyncExecutableMissing)
            .expect("missing executable should have a dedicated finding");
        assert_eq!(finding.severity, DoctorSeverity::Warning);
        assert!(
            finding
                .message
                .contains(&missing_executable.display().to_string())
        );
        assert_eq!(
            finding.next_command.as_deref(),
            Some("dalo autosync install")
        );
        assert!(
            !findings
                .iter()
                .any(|finding| finding.code == DoctorCode::AutosyncDisabled)
        );
    }

    #[test]
    fn run_doctor_should_not_compare_provenance_when_source_lock_is_invalid() {
        use crate::config::Settings;

        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let catalog_repo = temp_dir.path().join("catalog");
        store::init_store(store.clone(), false).expect("init should succeed");
        create_git_skill_repo(&catalog_repo);
        let paths = StorePaths::new(store.clone());
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "team.marketing".to_owned(),
                kind: SourceKind::Catalog,
                path: catalog_repo,
                priority: 11,
                enabled: true,
                trusted: false,
                url: Some("https://example.com/marketing.git".to_owned()),
                branch: None,
                update_policy: Some("manifest".to_owned()),
                selection: Vec::new(),
                declared_by: Some("team".to_owned()),
                declared_ref: Some("main".to_owned()),
            }],
        };
        store::write_config(&paths, &config).expect("config should be written");
        fs::write(&paths.source_lock_file, "schema_version = ")
            .expect("source lock should be corrupted");

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::SourceLockInvalid)
        );
        assert!(!report.findings.iter().any(|finding| {
            matches!(
                finding.code,
                DoctorCode::SourceLockOk
                    | DoctorCode::SourceProvenanceOk
                    | DoctorCode::SourceProvenanceMismatch
            )
        }));
    }

    #[test]
    fn run_doctor_should_point_invalid_config_to_the_editor() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store with $(shell)");
        store::init_store(store.clone(), false).expect("init should succeed");
        let config_file = store.join("config.toml");
        fs::write(&config_file, "version = ").expect("config should be corrupted");

        let report = run_doctor(&store);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == DoctorCode::ConfigInvalid)
            .expect("invalid config should be reported");

        assert!(finding.message.contains("line 1"));
        assert_eq!(
            finding.next_command.as_deref(),
            Some(format!("$EDITOR '{}'", config_file.display()).as_str())
        );
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
    fn run_doctor_should_accept_owned_symlink_to_store_equivalent_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let store_alias = temp_dir.path().join("store-alias");
        let target = temp_dir.path().join("target");
        store::init_store(store.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target).expect("target should be created");
        let store_skill = store.join("local/skills/review");
        fs::create_dir_all(&store_skill).expect("skill should be created");
        std::os::unix::fs::symlink(&store, &store_alias).expect("store alias should be created");
        let link = target.join("review");
        std::os::unix::fs::symlink(store_alias.join("local/skills/review"), &link)
            .expect("owned symlink should be created");
        write_state(&store, &target, &link, &store_skill);

        let report = run_doctor(&store);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::ForeignOwnedSymlink)
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::OwnedSymlinkOk)
        );
    }

    #[test]
    fn command_succeeds_with_timeout_should_stop_hung_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let command = temp_dir.path().join("hang");
        fs::write(&command, "#!/bin/sh\nwhile :; do :; done\n").expect("script should be written");
        let mut permissions = fs::metadata(&command)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command, permissions).expect("script should be executable");

        assert!(!command_succeeds_with_timeout(
            command.to_str().expect("script path should be utf-8"),
            &[],
            Duration::from_millis(10),
        ));
    }

    #[test]
    fn run_doctor_should_report_missing_instruction_block() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let (store, target) = setup_enabled_instruction_pack(temp_dir.path(), "Body v1\n");
        fs::write(&target, "user-owned content\n").expect("target should be rewritten");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::InstructionBlockDrift
                && finding.message.contains("missing")
                && finding
                    .next_command
                    .as_deref()
                    .is_some_and(|command| command.contains("dalo instructions enable house-style"))
        }));
    }

    #[test]
    fn run_doctor_should_report_stale_instruction_block() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let (store, _target) = setup_enabled_instruction_pack(temp_dir.path(), "Body v1\n");
        fs::write(store.join("local/instructions/house-style.md"), "Body v2\n")
            .expect("pack should be updated");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::InstructionBlockDrift && finding.message.contains("stale")
        }));
    }

    #[test]
    fn run_doctor_should_report_invalid_approvals_without_pending_warnings() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let team_repo = temp_dir.path().join("team-repo");
        store::init_store(store.clone(), false).expect("init should succeed");
        create_git_skill_repo(&team_repo);
        write_config_with_team_source(&store, &team_repo, false);
        fs::write(
            StorePaths::new(store.clone()).approvals_file,
            "schema_version = ",
        )
        .expect("approvals should be corrupted");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::ApprovalsInvalid
                && finding.severity == DoctorSeverity::Error
                && finding.next_command.as_deref() == Some("inspect or restore approvals.toml")
        }));
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::PendingApproval)
        );
    }

    #[test]
    fn check_sources_should_rate_dirty_team_source_as_error() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let team_repo = temp_dir.path().join("team repo $(shell)");
        create_dirty_git_repo(&team_repo);
        write_config_with_dirty_sources(&store, &team_repo, None);

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::DirtySource
                && finding.severity == DoctorSeverity::Error
                && finding.message.contains("`team`")
                && finding.next_command.as_deref()
                    == Some(format!("git -C '{}' status", team_repo.display()).as_str())
        }));
    }

    #[test]
    fn check_sources_should_rate_dirty_local_source_as_warning() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let team_repo = temp_dir.path().join("team-repo");
        let local_repo = temp_dir.path().join("local repo; echo unsafe");
        create_dirty_git_repo(&team_repo);
        create_dirty_git_repo(&local_repo);
        write_config_with_dirty_sources(&store, &team_repo, Some(&local_repo));

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::DirtySource
                && finding.severity == DoctorSeverity::Warning
                && finding.message.contains("`workspace`")
                && finding.message.contains("adopted skills must be committed")
                && finding.next_command.as_deref()
                    == Some(format!("git -C '{}' status", local_repo.display()).as_str())
        }));
    }

    #[test]
    fn doctor_should_report_unconfigured_source_operation_debris() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let debris = store.join("sources/orphan/checkout.dalo-removing");
        fs::create_dir_all(&debris).expect("source debris should be created");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::SourceStoreDebris
                && finding.severity == DoctorSeverity::Warning
                && finding
                    .message
                    .contains(debris.parent().unwrap().to_string_lossy().as_ref())
        }));
    }

    fn setup_enabled_instruction_pack(root: &Path, body: &str) -> (PathBuf, PathBuf) {
        let store = root.join("store");
        let target = root.join("AGENTS.md");
        store::init_store(store.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store.clone());
        fs::write(paths.local_instructions_dir.join("house-style.md"), body)
            .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");
        crate::instructions::enable_pack(&paths, "house-style", &target, false)
            .expect("pack should be enabled");
        (store, target)
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

    fn create_git_skill_repo(repo: &Path) {
        fs::create_dir_all(repo.join("skills/review")).expect("repo skill dir should be created");
        fs::write(repo.join("skills/review/SKILL.md"), "# Review\n")
            .expect("skill should be written");
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

    fn write_config_with_team_source(store: &Path, team_repo: &Path, trusted: bool) {
        use crate::config::{Settings, UserConfig};
        use crate::source::{SourceConfig, SourceKind};

        let paths = StorePaths::new(store.to_path_buf());
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "team".to_owned(),
                kind: SourceKind::Team,
                path: team_repo.to_path_buf(),
                priority: 10,
                enabled: true,
                trusted,
                url: None,
                branch: None,
                update_policy: None,
                selection: Vec::new(),
                declared_by: None,
                declared_ref: None,
            }],
        };
        store::write_config(&paths, &config).expect("config should be written");
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
            declared_by: None,
            declared_ref: None,
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
                declared_by: None,
                declared_ref: None,
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
            extra: Default::default(),
        }];
        state.materialization_dirs = vec![MaterializationDirState {
            path: target.to_path_buf(),
            logical_targets: vec!["generic".to_owned()],
            extra: Default::default(),
        }];
        state.owned_skills = vec![OwnedSkillState {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link.to_path_buf(),
            store_path: store_path.to_path_buf(),
            extra: Default::default(),
        }];
        store::write_state(&paths, &state).expect("state should be written");
    }
}
