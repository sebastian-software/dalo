//! Installable scheduled synchronization and durable run diagnostics.

#[cfg(test)]
use std::cell::RefCell;
use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};
use crate::store::{self, StorePaths};

const AUTOSYNC_SCHEMA_VERSION: u32 = 1;
const CRON_BEGIN_PREFIX: &str = "# BEGIN dalo autosync ";
const CRON_END_PREFIX: &str = "# END dalo autosync ";

/// Supported user-facing autosync schedules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AutosyncSchedule {
    /// Run once per hour.
    Hourly,
    /// Run once per day.
    Daily,
    /// Run once per week.
    Weekly,
}

impl AutosyncSchedule {
    /// Stable config and output label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }
}

/// Native scheduler selected for this installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerBackend {
    /// macOS per-user launchd agent.
    Launchd,
    /// Linux systemd user timer.
    Systemd,
    /// Per-user crontab fallback.
    Cron,
}

impl SchedulerBackend {
    /// Stable output label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Launchd => "launchd",
            Self::Systemd => "systemd",
            Self::Cron => "cron",
        }
    }
}

/// Persisted scheduler installation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutosyncInstallState {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Native scheduler backend.
    pub backend: SchedulerBackend,
    /// Configured schedule.
    pub schedule: AutosyncSchedule,
    /// Absolute Dalo executable path installed into the scheduler.
    pub executable: PathBuf,
    /// Absolute store path installed into the scheduler.
    pub store: PathBuf,
    /// Scheduler label, timer stem, or cron marker ID.
    pub identifier: String,
    /// Native artifact paths, or `crontab` for the cron fallback.
    pub artifacts: Vec<String>,
    /// Installation timestamp.
    pub installed_at_unix: u64,
}

/// Durable outcome of a scheduled synchronization attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutosyncRunOutcome {
    /// The runner started but has not recorded a terminal outcome yet.
    Running,
    /// Synchronization completed safely.
    Succeeded,
    /// The store was busy and this attempt intentionally did no work.
    Skipped,
    /// A fail-closed condition prevented completion.
    Blocked,
}

impl AutosyncRunOutcome {
    /// Stable output label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Skipped => "skipped",
            Self::Blocked => "blocked",
        }
    }
}

/// Persisted status of the latest scheduled attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutosyncRunState {
    /// Persisted schema version.
    pub schema_version: u32,
    /// Unix timestamp of the latest attempt.
    pub last_attempted_at_unix: u64,
    /// Unix timestamp of the latest successful attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful_at_unix: Option<u64>,
    /// Latest terminal or in-progress outcome.
    pub outcome: AutosyncRunOutcome,
    /// Actionable detail for skipped or blocked runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Full autosync status rendered by the CLI and embedded in `dalo status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutosyncStatusReport {
    /// Whether config.toml declares autosync enabled.
    pub configured: bool,
    /// Whether Dalo installation metadata exists.
    pub installed: bool,
    /// Whether the native scheduler currently reports the job enabled.
    pub enabled: bool,
    /// Installed scheduler backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<SchedulerBackend>,
    /// Installed schedule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<AutosyncSchedule>,
    /// Installed executable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    /// Installed store path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<PathBuf>,
    /// Scheduler label or marker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Native scheduler artifacts.
    pub artifacts: Vec<String>,
    /// Native scheduler inspection failure, when durable metadata remains readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduler_error: Option<String>,
    /// Durable status from the latest scheduled attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<AutosyncRunState>,
}

/// Install or uninstall command result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutosyncMutationReport {
    /// `installed`, `updated`, `unchanged`, `uninstalled`, or `would_*`.
    pub action: String,
    /// Whether no state was mutated.
    pub dry_run: bool,
    /// Resulting or planned status.
    pub status: AutosyncStatusReport,
}

#[derive(Debug)]
struct CommandResult {
    success: bool,
    stdout: String,
    stderr: String,
    status: String,
}

trait CommandRunner {
    fn run(&self, program: &str, args: &[String], input: Option<&str>)
    -> DaloResult<CommandResult>;
}

struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        input: Option<&str>,
    ) -> DaloResult<CommandResult> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if input.is_some() {
            command.stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::null());
        }
        let mut child = command.spawn().map_err(|error| {
            DaloError::Io(std::io::Error::new(
                error.kind(),
                format!("could not run `{program}`: {error}"),
            ))
        })?;
        if let Some(input) = input
            && let Some(mut stdin) = child.stdin.take()
        {
            stdin.write_all(input.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        Ok(CommandResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: output.status.to_string(),
        })
    }
}

/// Install or update the current user's native autosync job.
pub fn install(
    paths: &StorePaths,
    schedule: Option<AutosyncSchedule>,
    dry_run: bool,
) -> DaloResult<AutosyncMutationReport> {
    let schedule = schedule
        .or(read_install_state(paths)?.map(|state| state.schedule))
        .or_else(|| {
            store::read_config(paths)
                .ok()
                .and_then(|config| config.settings.sync_interval)
                .and_then(|value| schedule_from_str(&value))
        })
        .unwrap_or(AutosyncSchedule::Daily);
    let runner = SystemCommandRunner;
    let backend = detect_backend(&runner)?;
    let executable = env::current_exe()?.canonicalize()?;
    let home = user_home()?;
    install_with(
        paths,
        schedule,
        dry_run,
        backend,
        &executable,
        &home,
        &runner,
    )
}

fn schedule_from_str(value: &str) -> Option<AutosyncSchedule> {
    match value {
        "hourly" => Some(AutosyncSchedule::Hourly),
        "daily" => Some(AutosyncSchedule::Daily),
        "weekly" => Some(AutosyncSchedule::Weekly),
        _ => None,
    }
}

/// Remove the installed native autosync job idempotently.
pub fn uninstall(paths: &StorePaths, dry_run: bool) -> DaloResult<AutosyncMutationReport> {
    uninstall_with(paths, dry_run, &SystemCommandRunner)
}

/// Inspect scheduler installation and durable run state.
pub fn status(paths: &StorePaths) -> DaloResult<AutosyncStatusReport> {
    status_with(paths, &SystemCommandRunner)
}

/// Mark a scheduled run as started and return its attempt timestamp.
pub fn begin_run(paths: &StorePaths) -> DaloResult<u64> {
    let attempted = now_unix();
    let previous = read_run_state(paths)?;
    write_run_state(
        paths,
        &AutosyncRunState {
            schema_version: AUTOSYNC_SCHEMA_VERSION,
            last_attempted_at_unix: attempted,
            last_successful_at_unix: previous.and_then(|state| state.last_successful_at_unix),
            outcome: AutosyncRunOutcome::Running,
            reason: None,
        },
    )?;
    Ok(attempted)
}

/// Persist the terminal outcome for a scheduled run.
pub fn finish_run(
    paths: &StorePaths,
    attempted: u64,
    outcome: AutosyncRunOutcome,
    reason: Option<String>,
) -> DaloResult<()> {
    let previous = read_run_state(paths)?;
    let last_successful_at_unix = if outcome == AutosyncRunOutcome::Succeeded {
        Some(now_unix())
    } else {
        previous.and_then(|state| state.last_successful_at_unix)
    };
    write_run_state(
        paths,
        &AutosyncRunState {
            schema_version: AUTOSYNC_SCHEMA_VERSION,
            last_attempted_at_unix: attempted,
            last_successful_at_unix,
            outcome,
            reason,
        },
    )
}

fn install_with(
    paths: &StorePaths,
    schedule: AutosyncSchedule,
    dry_run: bool,
    backend: SchedulerBackend,
    executable: &Path,
    home: &Path,
    runner: &dyn CommandRunner,
) -> DaloResult<AutosyncMutationReport> {
    if !executable.is_file() {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "autosync requires a stable executable file, but `{}` is unavailable",
                executable.display()
            ),
        });
    }
    if fs::metadata(executable)?.permissions().mode() & 0o111 == 0 {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "autosync executable `{}` is not executable",
                executable.display()
            ),
        });
    }
    for path in [
        executable,
        &paths.root,
        home,
        &paths.autosync_log_file,
        &paths.autosync_error_log_file,
    ] {
        let Some(path_text) = path.to_str() else {
            return Err(DaloError::InvalidStorePath {
                path: path.to_path_buf(),
                reason: "scheduler paths must be valid UTF-8 so native artifacts can preserve them exactly"
                    .to_owned(),
            });
        };
        if path_text.chars().any(char::is_control) {
            return Err(DaloError::InvalidStorePath {
                path: path.to_path_buf(),
                reason: "scheduler paths must not contain control characters".to_owned(),
            });
        }
        if backend == SchedulerBackend::Cron && path_text.contains('\\') {
            return Err(DaloError::InvalidStorePath {
                path: path.to_path_buf(),
                reason: "cron fallback cannot safely encode paths containing backslashes; install a systemd user manager or move the path"
                    .to_owned(),
            });
        }
    }
    let config = store::read_config(paths)?;
    let previous = read_install_state(paths)?;
    let mut state = planned_state(paths, backend, schedule, executable, home);
    if let Some(previous) = &previous
        && same_install(previous, &state)
    {
        state.installed_at_unix = previous.installed_at_unix;
    }
    let config_matches = config.settings.autosync
        && config.settings.sync_interval.as_deref() == Some(schedule.as_str());
    let action = if previous
        .as_ref()
        .is_some_and(|old| same_install(old, &state))
        && config_matches
    {
        "unchanged"
    } else if previous.is_some() {
        "updated"
    } else {
        "installed"
    };

    if dry_run {
        let planned_action = match action {
            "installed" => "would_install",
            "updated" => "would_update",
            _ => "unchanged",
        };
        return Ok(AutosyncMutationReport {
            action: planned_action.to_owned(),
            dry_run: true,
            status: report_from_state(Some(state), true, read_run_state(paths)?, true),
        });
    }

    if let Some(old) = &previous
        && !same_install(old, &state)
    {
        disable_scheduler(old, runner)?;
        if let Err(error) = remove_artifacts(old) {
            restore_previous_install(paths, Some(old), runner);
            return Err(error);
        }
    }
    if let Err(error) = write_artifacts(paths, &state) {
        restore_previous_install(paths, previous.as_ref(), runner);
        return Err(error);
    }
    if let Err(error) = enable_scheduler(paths, &state, runner) {
        let _ = remove_artifacts(&state);
        restore_previous_install(paths, previous.as_ref(), runner);
        return Err(error);
    }
    if let Err(error) = persist_install(paths, &state, &config) {
        let _ = disable_scheduler(&state, runner);
        let _ = remove_artifacts(&state);
        if let Some(previous) = previous {
            let _ = write_artifacts(paths, &previous);
            let _ = write_install_state(paths, &previous);
            let _ = enable_scheduler(paths, &previous, runner);
        }
        let _ = store::write_config(paths, &config);
        return Err(error);
    }

    Ok(AutosyncMutationReport {
        action: action.to_owned(),
        dry_run: false,
        status: report_from_state(Some(state), true, read_run_state(paths)?, true),
    })
}

fn uninstall_with(
    paths: &StorePaths,
    dry_run: bool,
    runner: &dyn CommandRunner,
) -> DaloResult<AutosyncMutationReport> {
    let state = read_install_state(paths)?;
    let original_config = store::read_config(paths)?;
    let configured = original_config.settings.autosync;
    if dry_run {
        return Ok(AutosyncMutationReport {
            action: if state.is_some() || configured {
                "would_uninstall"
            } else {
                "unchanged"
            }
            .to_owned(),
            dry_run: true,
            status: report_from_state(None, false, read_run_state(paths)?, false),
        });
    }

    if let Some(state) = &state {
        disable_scheduler(state, runner)?;
        if let Err(error) = remove_artifacts(state) {
            restore_previous_install(paths, Some(state), runner);
            return Err(error);
        }
        if state.backend == SchedulerBackend::Systemd
            && let Err(error) = require_success(
                "systemctl",
                &["--user".to_owned(), "daemon-reload".to_owned()],
                runner,
                None,
            )
        {
            restore_previous_install(paths, Some(state), runner);
            return Err(error);
        }
    }
    let mut config = original_config.clone();
    config.settings.autosync = false;
    config.settings.sync_interval = None;
    if let Err(error) = store::write_config(paths, &config) {
        restore_previous_install(paths, state.as_ref(), runner);
        return Err(error);
    }
    if paths.autosync_file.exists()
        && let Err(error) = fs::remove_file(&paths.autosync_file)
    {
        let _ = store::write_config(paths, &original_config);
        restore_previous_install(paths, state.as_ref(), runner);
        return Err(error.into());
    }

    Ok(AutosyncMutationReport {
        action: if state.is_some() || configured {
            "uninstalled"
        } else {
            "unchanged"
        }
        .to_owned(),
        dry_run: false,
        status: report_from_state(None, false, read_run_state(paths)?, false),
    })
}

fn restore_previous_install(
    paths: &StorePaths,
    previous: Option<&AutosyncInstallState>,
    runner: &dyn CommandRunner,
) {
    if let Some(previous) = previous {
        let _ = write_artifacts(paths, previous);
        let _ = write_install_state(paths, previous);
        let _ = enable_scheduler(paths, previous, runner);
    }
}

fn persist_install(
    paths: &StorePaths,
    state: &AutosyncInstallState,
    original_config: &crate::config::UserConfig,
) -> DaloResult<()> {
    write_install_state(paths, state)?;
    let mut config = original_config.clone();
    config.settings.autosync = true;
    config.settings.sync_interval = Some(state.schedule.as_str().to_owned());
    if let Err(error) = store::write_config(paths, &config) {
        let _ = fs::remove_file(&paths.autosync_file);
        return Err(error);
    }
    Ok(())
}

fn status_with(paths: &StorePaths, runner: &dyn CommandRunner) -> DaloResult<AutosyncStatusReport> {
    let state = read_install_state(paths)?;
    let configured = if paths.config_file.exists() {
        store::read_config(paths).is_ok_and(|config| config.settings.autosync)
    } else {
        false
    };
    let mut scheduler_error = None;
    let enabled = match &state {
        Some(state) => {
            let artifacts_present = state.backend == SchedulerBackend::Cron
                || state
                    .artifacts
                    .iter()
                    .all(|artifact| Path::new(artifact).is_file());
            if artifacts_present
                && executable_available(&state.executable)
                && state.store == paths.root
            {
                match scheduler_enabled(state, runner) {
                    Ok(enabled) => enabled,
                    Err(error) => {
                        scheduler_error = Some(error.to_string());
                        false
                    }
                }
            } else {
                false
            }
        }
        None => false,
    };
    let mut report = report_from_state(state, enabled, read_run_state(paths)?, configured);
    report.scheduler_error = scheduler_error;
    Ok(report)
}

fn executable_available(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn report_from_state(
    state: Option<AutosyncInstallState>,
    enabled: bool,
    last_run: Option<AutosyncRunState>,
    configured: bool,
) -> AutosyncStatusReport {
    AutosyncStatusReport {
        configured,
        installed: state.is_some(),
        enabled,
        backend: state.as_ref().map(|state| state.backend),
        schedule: state.as_ref().map(|state| state.schedule),
        executable: state.as_ref().map(|state| state.executable.clone()),
        store: state.as_ref().map(|state| state.store.clone()),
        identifier: state.as_ref().map(|state| state.identifier.clone()),
        artifacts: state.map_or_else(Vec::new, |state| state.artifacts),
        scheduler_error: None,
        last_run,
    }
}

#[cfg(target_os = "macos")]
fn detect_backend(runner: &dyn CommandRunner) -> DaloResult<SchedulerBackend> {
    let _ = runner.run("launchctl", &["version".to_owned()], None)?;
    Ok(SchedulerBackend::Launchd)
}

#[cfg(target_os = "linux")]
fn detect_backend(runner: &dyn CommandRunner) -> DaloResult<SchedulerBackend> {
    if runner
        .run(
            "systemctl",
            &["--user".to_owned(), "show-environment".to_owned()],
            None,
        )
        .is_ok_and(|result| result.success)
    {
        return Ok(SchedulerBackend::Systemd);
    }
    if read_crontab(runner).is_ok() {
        return Ok(SchedulerBackend::Cron);
    }
    Err(DaloError::CheckFailed {
        reason: "no supported user scheduler is available (launchd, systemd --user, or crontab)"
            .to_owned(),
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_backend(runner: &dyn CommandRunner) -> DaloResult<SchedulerBackend> {
    if read_crontab(runner).is_ok() {
        Ok(SchedulerBackend::Cron)
    } else {
        Err(DaloError::CheckFailed {
            reason:
                "no supported user scheduler is available (launchd, systemd --user, or crontab)"
                    .to_owned(),
        })
    }
}

fn planned_state(
    paths: &StorePaths,
    backend: SchedulerBackend,
    schedule: AutosyncSchedule,
    executable: &Path,
    home: &Path,
) -> AutosyncInstallState {
    let hash = store_hash(&paths.root);
    let (identifier, artifacts) = match backend {
        SchedulerBackend::Launchd => {
            let identifier = format!("dev.dalo.autosync.{hash}");
            let artifact = home
                .join("Library/LaunchAgents")
                .join(format!("{identifier}.plist"));
            (identifier, vec![artifact.display().to_string()])
        }
        SchedulerBackend::Systemd => {
            let identifier = format!("dalo-autosync-{hash}");
            let root = home.join(".config/systemd/user");
            (
                identifier.clone(),
                vec![
                    root.join(format!("{identifier}.service"))
                        .display()
                        .to_string(),
                    root.join(format!("{identifier}.timer"))
                        .display()
                        .to_string(),
                ],
            )
        }
        SchedulerBackend::Cron => {
            let identifier = format!("dalo-autosync-{hash}");
            (identifier, vec!["crontab".to_owned()])
        }
    };
    AutosyncInstallState {
        schema_version: AUTOSYNC_SCHEMA_VERSION,
        backend,
        schedule,
        executable: executable.to_path_buf(),
        store: paths.root.clone(),
        identifier,
        artifacts,
        installed_at_unix: now_unix(),
    }
}

fn same_install(left: &AutosyncInstallState, right: &AutosyncInstallState) -> bool {
    left.schema_version == right.schema_version
        && left.backend == right.backend
        && left.schedule == right.schedule
        && left.executable == right.executable
        && left.store == right.store
        && left.identifier == right.identifier
        && left.artifacts == right.artifacts
}

fn write_artifacts(paths: &StorePaths, state: &AutosyncInstallState) -> DaloResult<()> {
    match state.backend {
        SchedulerBackend::Launchd => {
            write_text_atomic(
                Path::new(&state.artifacts[0]),
                &render_launchd(paths, state),
            )?;
        }
        SchedulerBackend::Systemd => {
            write_text_atomic(
                Path::new(&state.artifacts[0]),
                &render_systemd_service(paths, state),
            )?;
            write_text_atomic(Path::new(&state.artifacts[1]), &render_systemd_timer(state))?;
        }
        SchedulerBackend::Cron => {}
    }
    Ok(())
}

fn remove_artifacts(state: &AutosyncInstallState) -> DaloResult<()> {
    if state.backend == SchedulerBackend::Cron {
        return Ok(());
    }
    for artifact in &state.artifacts {
        let path = Path::new(artifact);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn enable_scheduler(
    paths: &StorePaths,
    state: &AutosyncInstallState,
    runner: &dyn CommandRunner,
) -> DaloResult<()> {
    match state.backend {
        SchedulerBackend::Launchd => {
            let domain = launchd_domain(runner)?;
            let artifact = state.artifacts[0].clone();
            let _ = runner.run(
                "launchctl",
                &["bootout".to_owned(), domain.clone(), artifact.clone()],
                None,
            );
            require_success(
                "launchctl",
                &["bootstrap".to_owned(), domain.clone(), artifact],
                runner,
                None,
            )?;
            require_success(
                "launchctl",
                &[
                    "enable".to_owned(),
                    format!("{domain}/{}", state.identifier),
                ],
                runner,
                None,
            )
            .map(|_| ())
        }
        SchedulerBackend::Systemd => {
            require_success(
                "systemctl",
                &["--user".to_owned(), "daemon-reload".to_owned()],
                runner,
                None,
            )?;
            require_success(
                "systemctl",
                &[
                    "--user".to_owned(),
                    "enable".to_owned(),
                    "--now".to_owned(),
                    format!("{}.timer", state.identifier),
                ],
                runner,
                None,
            )
            .map(|_| ())
        }
        SchedulerBackend::Cron => install_cron(paths, state, runner),
    }
}

fn disable_scheduler(state: &AutosyncInstallState, runner: &dyn CommandRunner) -> DaloResult<()> {
    match state.backend {
        SchedulerBackend::Launchd => {
            let domain = launchd_domain(runner)?;
            let loaded = runner
                .run(
                    "launchctl",
                    &["print".to_owned(), format!("{domain}/{}", state.identifier)],
                    None,
                )?
                .success;
            if loaded {
                require_success(
                    "launchctl",
                    &["bootout".to_owned(), domain, state.artifacts[0].clone()],
                    runner,
                    None,
                )?;
            }
            Ok(())
        }
        SchedulerBackend::Systemd => {
            let disable = runner.run(
                "systemctl",
                &[
                    "--user".to_owned(),
                    "disable".to_owned(),
                    "--now".to_owned(),
                    format!("{}.timer", state.identifier),
                ],
                None,
            )?;
            if !disable.success && !scheduler_absent(&disable.stderr) {
                return Err(DaloError::CommandFailed {
                    program: "systemctl".to_owned(),
                    args: format!("--user disable --now {}.timer", state.identifier),
                    cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                    status: disable.status,
                    stderr: disable.stderr.trim().to_owned(),
                });
            }
            require_success(
                "systemctl",
                &["--user".to_owned(), "daemon-reload".to_owned()],
                runner,
                None,
            )?;
            Ok(())
        }
        SchedulerBackend::Cron => remove_cron(state, runner),
    }
}

fn scheduler_absent(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("does not exist")
        || stderr.contains("not loaded")
        || stderr.contains("not found")
}

fn scheduler_enabled(state: &AutosyncInstallState, runner: &dyn CommandRunner) -> DaloResult<bool> {
    match state.backend {
        SchedulerBackend::Launchd => {
            let domain = launchd_domain(runner)?;
            Ok(runner
                .run(
                    "launchctl",
                    &["print".to_owned(), format!("{domain}/{}", state.identifier)],
                    None,
                )?
                .success)
        }
        SchedulerBackend::Systemd => {
            let unit = format!("{}.timer", state.identifier);
            let enabled = runner
                .run(
                    "systemctl",
                    &["--user".to_owned(), "is-enabled".to_owned(), unit.clone()],
                    None,
                )?
                .success;
            let active = runner
                .run(
                    "systemctl",
                    &["--user".to_owned(), "is-active".to_owned(), unit],
                    None,
                )?
                .success;
            Ok(enabled && active)
        }
        SchedulerBackend::Cron => {
            Ok(read_crontab(runner)?.contains(&cron_begin(&state.identifier)))
        }
    }
}

fn launchd_domain(runner: &dyn CommandRunner) -> DaloResult<String> {
    let result = require_success("id", &["-u".to_owned()], runner, None)?;
    let uid = result.stdout.trim();
    if uid.is_empty() || !uid.chars().all(|character| character.is_ascii_digit()) {
        return Err(DaloError::CheckFailed {
            reason: "could not determine the current numeric user ID for launchd".to_owned(),
        });
    }
    Ok(format!("gui/{uid}"))
}

fn install_cron(
    paths: &StorePaths,
    state: &AutosyncInstallState,
    runner: &dyn CommandRunner,
) -> DaloResult<()> {
    let current = read_crontab(runner)?;
    let without = strip_cron_block(&current, &state.identifier)?;
    let block = render_cron(paths, state);
    let content = if without.trim().is_empty() {
        block
    } else {
        format!("{}\n{}", without.trim_end(), block)
    };
    require_success("crontab", &["-".to_owned()], runner, Some(&content))?;
    Ok(())
}

fn remove_cron(state: &AutosyncInstallState, runner: &dyn CommandRunner) -> DaloResult<()> {
    let current = read_crontab(runner)?;
    let content = strip_cron_block(&current, &state.identifier)?;
    require_success("crontab", &["-".to_owned()], runner, Some(&content))?;
    Ok(())
}

fn read_crontab(runner: &dyn CommandRunner) -> DaloResult<String> {
    let args = ["-l".to_owned()];
    let result = runner.run("crontab", &args, None)?;
    if result.success || result.stderr.to_ascii_lowercase().contains("no crontab") {
        Ok(result.stdout)
    } else {
        Err(DaloError::CommandFailed {
            program: "crontab".to_owned(),
            args: "-l".to_owned(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            status: result.status,
            stderr: result.stderr.trim().to_owned(),
        })
    }
}

fn require_success(
    program: &str,
    args: &[String],
    runner: &dyn CommandRunner,
    input: Option<&str>,
) -> DaloResult<CommandResult> {
    let result = runner.run(program, args, input)?;
    if result.success {
        Ok(result)
    } else {
        Err(DaloError::CommandFailed {
            program: program.to_owned(),
            args: args.join(" "),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            status: result.status,
            stderr: result.stderr.trim().to_owned(),
        })
    }
}

fn render_launchd(paths: &StorePaths, state: &AutosyncInstallState) -> String {
    let (hour, minute) = schedule_time(&state.store);
    let calendar = match state.schedule {
        AutosyncSchedule::Hourly => format!("<key>Minute</key><integer>{minute}</integer>"),
        AutosyncSchedule::Daily => format!(
            "<key>Hour</key><integer>{hour}</integer><key>Minute</key><integer>{minute}</integer>"
        ),
        AutosyncSchedule::Weekly => format!(
            "<key>Weekday</key><integer>0</integer><key>Hour</key><integer>{hour}</integer><key>Minute</key><integer>{minute}</integer>"
        ),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>--store</string>\n    <string>{}</string>\n    <string>autosync</string>\n    <string>run</string>\n  </array>\n  <key>StartCalendarInterval</key><dict>{calendar}</dict>\n  <key>ProcessType</key><string>Background</string>\n  <key>StandardOutPath</key><string>{}</string>\n  <key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
        xml_escape(&state.identifier),
        xml_escape(&state.executable.display().to_string()),
        xml_escape(&state.store.display().to_string()),
        xml_escape(&paths.autosync_log_file.display().to_string()),
        xml_escape(&paths.autosync_error_log_file.display().to_string())
    )
}

fn render_systemd_service(paths: &StorePaths, state: &AutosyncInstallState) -> String {
    format!(
        "[Unit]\nDescription=Dalo scheduled synchronization\n\n[Service]\nType=oneshot\nExecStart={} --store {} autosync run\nStandardOutput={}\nStandardError={}\n",
        systemd_quote(&state.executable),
        systemd_quote(&state.store),
        systemd_append_quote(&paths.autosync_log_file),
        systemd_append_quote(&paths.autosync_error_log_file)
    )
}

fn render_systemd_timer(state: &AutosyncInstallState) -> String {
    let (hour, minute) = schedule_time(&state.store);
    let calendar = match state.schedule {
        AutosyncSchedule::Hourly => format!("*-*-* *:{minute:02}:00"),
        AutosyncSchedule::Daily => format!("*-*-* {hour:02}:{minute:02}:00"),
        AutosyncSchedule::Weekly => format!("Sun *-*-* {hour:02}:{minute:02}:00"),
    };
    format!(
        "[Unit]\nDescription=Run Dalo synchronization on schedule\n\n[Timer]\nOnCalendar={calendar}\nPersistent=true\nRandomizedDelaySec=15m\nUnit={}.service\n\n[Install]\nWantedBy=timers.target\n",
        state.identifier
    )
}

fn render_cron(paths: &StorePaths, state: &AutosyncInstallState) -> String {
    let (hour, minute) = schedule_time(&state.store);
    let expression = match state.schedule {
        AutosyncSchedule::Hourly => format!("{minute} * * * *"),
        AutosyncSchedule::Daily => format!("{minute} {hour} * * *"),
        AutosyncSchedule::Weekly => format!("{minute} {hour} * * 0"),
    };
    format!(
        "{}\n{expression} {} --store {} autosync run >> {} 2>> {}\n{}\n",
        cron_begin(&state.identifier),
        cron_shell_quote(&state.executable),
        cron_shell_quote(&state.store),
        cron_shell_quote(&paths.autosync_log_file),
        cron_shell_quote(&paths.autosync_error_log_file),
        cron_end(&state.identifier)
    )
}

fn cron_begin(identifier: &str) -> String {
    format!("{CRON_BEGIN_PREFIX}{identifier}")
}

fn cron_end(identifier: &str) -> String {
    format!("{CRON_END_PREFIX}{identifier}")
}

fn strip_cron_block(content: &str, identifier: &str) -> DaloResult<String> {
    let begin = cron_begin(identifier);
    let end = cron_end(identifier);
    let mut output = Vec::new();
    let mut skipping = false;
    for line in content.lines() {
        if line == begin {
            if skipping {
                return Err(DaloError::CheckFailed {
                    reason: format!(
                        "cron fallback contains duplicate start markers for `{identifier}`"
                    ),
                });
            }
            skipping = true;
            continue;
        }
        if line == end {
            if !skipping {
                return Err(DaloError::CheckFailed {
                    reason: format!(
                        "cron fallback contains an unmatched end marker for `{identifier}`"
                    ),
                });
            }
            skipping = false;
            continue;
        }
        if !skipping {
            output.push(line);
        }
    }
    let mut rendered = output.join("\n");
    if !rendered.is_empty() {
        rendered.push('\n');
    }
    if skipping {
        return Err(DaloError::CheckFailed {
            reason: format!("cron fallback has no end marker for `{identifier}`"),
        });
    }
    Ok(rendered)
}

fn schedule_time(store: &Path) -> (u8, u8) {
    let digest = Sha256::digest(store.as_os_str().as_encoded_bytes());
    (digest[0] % 6, digest[1] % 60)
}

fn store_hash(store: &Path) -> String {
    Sha256::digest(store.as_os_str().as_encoded_bytes())
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn systemd_quote(path: &Path) -> String {
    format!("\"{}\"", systemd_escape(path))
}

fn systemd_escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$")
}

fn systemd_append_quote(path: &Path) -> String {
    format!("\"append:{}\"", systemd_escape(path))
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\"'\"'"))
}

fn cron_shell_quote(path: &Path) -> String {
    shell_quote(path).replace('%', "\\%")
}

fn write_text_atomic(path: &Path, content: &str) -> DaloResult<()> {
    let parent = path.parent().ok_or_else(|| DaloError::InvalidStorePath {
        path: path.to_path_buf(),
        reason: "scheduler artifact has no parent directory".to_owned(),
    })?;
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn read_install_state(paths: &StorePaths) -> DaloResult<Option<AutosyncInstallState>> {
    if !paths.autosync_file.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&paths.autosync_file)?;
    let state: AutosyncInstallState =
        toml::from_str(&content).map_err(|error| DaloError::FileParse {
            path: paths.autosync_file.clone(),
            reason: error.to_string(),
        })?;
    if state.schema_version != AUTOSYNC_SCHEMA_VERSION {
        return Err(DaloError::UnsupportedSchema {
            path: paths.autosync_file.clone(),
            version: state.schema_version,
            supported: AUTOSYNC_SCHEMA_VERSION,
        });
    }
    Ok(Some(state))
}

fn write_install_state(paths: &StorePaths, state: &AutosyncInstallState) -> DaloResult<()> {
    store::write_toml_atomic(&paths.autosync_file, state)
}

fn read_run_state(paths: &StorePaths) -> DaloResult<Option<AutosyncRunState>> {
    if !paths.autosync_run_file.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&paths.autosync_run_file)?;
    let state: AutosyncRunState =
        toml::from_str(&content).map_err(|error| DaloError::FileParse {
            path: paths.autosync_run_file.clone(),
            reason: error.to_string(),
        })?;
    if state.schema_version != AUTOSYNC_SCHEMA_VERSION {
        return Err(DaloError::UnsupportedSchema {
            path: paths.autosync_run_file.clone(),
            version: state.schema_version,
            supported: AUTOSYNC_SCHEMA_VERSION,
        });
    }
    Ok(Some(state))
}

fn write_run_state(paths: &StorePaths, state: &AutosyncRunState) -> DaloResult<()> {
    store::write_toml_atomic(&paths.autosync_run_file, state)
}

fn user_home() -> DaloResult<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| DaloError::StorePath {
            reason: "HOME is not set; cannot locate the user scheduler directory".to_owned(),
        })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
type RecordedCall = (String, Vec<String>, Option<String>);

#[cfg(test)]
#[derive(Default)]
struct FakeRunner {
    calls: RefCell<Vec<RecordedCall>>,
    crontab: RefCell<String>,
}

#[cfg(test)]
impl CommandRunner for FakeRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        input: Option<&str>,
    ) -> DaloResult<CommandResult> {
        self.calls
            .borrow_mut()
            .push((program.to_owned(), args.to_vec(), input.map(str::to_owned)));
        if program == "id" {
            return Ok(CommandResult {
                success: true,
                stdout: "501\n".to_owned(),
                stderr: String::new(),
                status: "0".to_owned(),
            });
        }
        if program == "crontab" && args == ["-l"] {
            return Ok(CommandResult {
                success: true,
                stdout: self.crontab.borrow().clone(),
                stderr: String::new(),
                status: "0".to_owned(),
            });
        }
        if program == "crontab" && args == ["-"] {
            *self.crontab.borrow_mut() = input.unwrap_or_default().to_owned();
        }
        Ok(CommandResult {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
            status: "0".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, StorePaths, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let store = temp.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let home = temp.path().join("home with spaces");
        fs::create_dir_all(&home).expect("home should exist");
        let executable = temp.path().join("bin/dalo '%$pecial");
        fs::create_dir_all(executable.parent().expect("binary has parent"))
            .expect("binary dir should exist");
        fs::write(&executable, "binary").expect("binary should exist");
        let mut permissions = fs::metadata(&executable)
            .expect("binary metadata readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("binary should be executable");
        (temp, StorePaths::new(store), home, executable)
    }

    #[test]
    fn generated_scheduler_artifacts_should_preserve_exact_paths() {
        let (_temp, paths, home, executable) = setup();
        for backend in [
            SchedulerBackend::Launchd,
            SchedulerBackend::Systemd,
            SchedulerBackend::Cron,
        ] {
            let state = planned_state(&paths, backend, AutosyncSchedule::Daily, &executable, &home);
            match backend {
                SchedulerBackend::Launchd => {
                    let artifact = render_launchd(&paths, &state);
                    assert!(artifact.contains("dalo &apos;%$pecial"));
                    assert!(artifact.contains("<string>--store</string>"));
                }
                SchedulerBackend::Systemd => {
                    let artifact = render_systemd_service(&paths, &state);
                    assert!(artifact.contains("ExecStart=\""));
                    assert!(artifact.contains(" --store \""));
                    assert!(artifact.contains("%%"));
                }
                SchedulerBackend::Cron => {
                    let artifact = render_cron(&paths, &state);
                    assert!(artifact.contains("'\"'\"'"));
                    assert!(artifact.contains("\\%"));
                    assert!(artifact.contains("autosync run"));
                }
            }
        }
    }

    #[test]
    fn weekly_schedule_should_use_sunday_for_every_backend() {
        let (_temp, paths, home, executable) = setup();

        let launchd = planned_state(
            &paths,
            SchedulerBackend::Launchd,
            AutosyncSchedule::Weekly,
            &executable,
            &home,
        );
        assert!(
            render_launchd(&paths, &launchd).contains("<key>Weekday</key><integer>0</integer>")
        );

        let systemd = planned_state(
            &paths,
            SchedulerBackend::Systemd,
            AutosyncSchedule::Weekly,
            &executable,
            &home,
        );
        assert!(render_systemd_timer(&systemd).contains("OnCalendar=Sun "));

        let cron = planned_state(
            &paths,
            SchedulerBackend::Cron,
            AutosyncSchedule::Weekly,
            &executable,
            &home,
        );
        assert!(render_cron(&paths, &cron).contains(" * * 0 "));
    }

    #[test]
    fn install_and_uninstall_should_be_idempotent_for_every_backend() {
        for backend in [
            SchedulerBackend::Launchd,
            SchedulerBackend::Systemd,
            SchedulerBackend::Cron,
        ] {
            let (_temp, paths, home, executable) = setup();
            let runner = FakeRunner::default();
            let first = install_with(
                &paths,
                AutosyncSchedule::Hourly,
                false,
                backend,
                &executable,
                &home,
                &runner,
            )
            .expect("first install should succeed");
            assert_eq!(first.action, "installed");
            let second = install_with(
                &paths,
                AutosyncSchedule::Hourly,
                false,
                backend,
                &executable,
                &home,
                &runner,
            )
            .expect("second install should succeed");
            assert_eq!(second.action, "unchanged");
            assert!(
                store::read_config(&paths)
                    .expect("config parses")
                    .settings
                    .autosync
            );
            let updated = install_with(
                &paths,
                AutosyncSchedule::Weekly,
                false,
                backend,
                &executable,
                &home,
                &runner,
            )
            .expect("schedule update should succeed");
            assert_eq!(updated.action, "updated");
            let installed_status = status_with(&paths, &runner).expect("status should succeed");
            assert!(installed_status.configured);
            assert!(installed_status.installed);
            assert!(installed_status.enabled);

            let removed =
                uninstall_with(&paths, false, &runner).expect("first uninstall should succeed");
            assert_eq!(removed.action, "uninstalled");
            let repeated =
                uninstall_with(&paths, false, &runner).expect("second uninstall should succeed");
            assert_eq!(repeated.action, "unchanged");
            assert!(
                !store::read_config(&paths)
                    .expect("config parses")
                    .settings
                    .autosync
            );
        }
    }

    #[test]
    fn dry_run_should_not_write_artifacts_config_or_scheduler_state() {
        let (_temp, paths, home, executable) = setup();
        let config_before = fs::read(&paths.config_file).expect("config readable");
        let runner = FakeRunner::default();
        let report = install_with(
            &paths,
            AutosyncSchedule::Weekly,
            true,
            SchedulerBackend::Systemd,
            &executable,
            &home,
            &runner,
        )
        .expect("dry-run should succeed");
        assert_eq!(report.action, "would_install");
        assert!(!paths.autosync_file.exists());
        assert_eq!(
            fs::read(&paths.config_file).expect("config readable"),
            config_before
        );
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn install_should_reject_scheduler_path_control_characters() {
        let (_temp, paths, home, _executable) = setup();
        let executable = home.join("dalo\nunsafe");
        fs::write(&executable, "binary").expect("binary should exist");
        let mut permissions = fs::metadata(&executable)
            .expect("binary metadata readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("binary should be executable");
        let error = install_with(
            &paths,
            AutosyncSchedule::Daily,
            true,
            SchedulerBackend::Cron,
            &executable,
            &home,
            &FakeRunner::default(),
        )
        .expect_err("control characters must be rejected");
        assert!(error.to_string().contains("control characters"));
    }

    #[test]
    fn run_state_should_preserve_last_success_across_skip() {
        let (_temp, paths, _home, _executable) = setup();
        let first = begin_run(&paths).expect("run should begin");
        finish_run(&paths, first, AutosyncRunOutcome::Succeeded, None)
            .expect("success should persist");
        let successful = read_run_state(&paths)
            .expect("state readable")
            .expect("state exists")
            .last_successful_at_unix;
        let second = begin_run(&paths).expect("second run should begin");
        finish_run(
            &paths,
            second,
            AutosyncRunOutcome::Skipped,
            Some("store lock held by pid=42".to_owned()),
        )
        .expect("skip should persist");
        let state = read_run_state(&paths)
            .expect("state readable")
            .expect("state exists");
        assert_eq!(state.last_successful_at_unix, successful);
        assert_eq!(state.outcome, AutosyncRunOutcome::Skipped);
    }

    #[test]
    fn cron_update_should_preserve_unrelated_entries() {
        let (_temp, paths, home, executable) = setup();
        let runner = FakeRunner::default();
        *runner.crontab.borrow_mut() = "5 4 * * * /usr/bin/backup\n".to_owned();
        install_with(
            &paths,
            AutosyncSchedule::Daily,
            false,
            SchedulerBackend::Cron,
            &executable,
            &home,
            &runner,
        )
        .expect("cron install should succeed");
        install_with(
            &paths,
            AutosyncSchedule::Daily,
            false,
            SchedulerBackend::Cron,
            &executable,
            &home,
            &runner,
        )
        .expect("cron reinstall should succeed");
        let crontab = runner.crontab.borrow();
        assert!(crontab.contains("/usr/bin/backup"));
        assert_eq!(crontab.matches(CRON_BEGIN_PREFIX).count(), 1);
    }

    #[test]
    fn cron_update_should_fail_closed_on_malformed_owned_block() {
        let (_temp, paths, home, executable) = setup();
        let runner = FakeRunner::default();
        let state = planned_state(
            &paths,
            SchedulerBackend::Cron,
            AutosyncSchedule::Daily,
            &executable,
            &home,
        );
        let original = format!(
            "{}\n5 4 * * * /usr/bin/backup\n",
            cron_begin(&state.identifier)
        );
        *runner.crontab.borrow_mut() = original.clone();

        let error = install_with(
            &paths,
            AutosyncSchedule::Daily,
            false,
            SchedulerBackend::Cron,
            &executable,
            &home,
            &runner,
        )
        .expect_err("malformed owned block should fail");
        assert!(error.to_string().contains("no end marker"));
        assert_eq!(*runner.crontab.borrow(), original);
    }
}
