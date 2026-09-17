//! Passive release notices and install-channel-specific upgrade guidance.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::fs::OpenOptions;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use update_informer::http_client::{GenericHttpClient, HttpClient};
use update_informer::{Check, Package, Registry};

const REPOSITORY: &str = "sebastian-software/dalo";
const RELEASE_API: &str = "https://api.github.com/repos/sebastian-software/dalo/releases/latest";
const INSTALL_RECEIPT: &str = ".dalo-install-channel";
const CHECK_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a finished check stays authoritative before the next one runs.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a check that never finished suppresses the next attempt.
const RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// How long a finishing command waits for a check it started itself.
///
/// Short enough that an interactive command still feels immediate, long enough
/// that a normal release-API round trip lands inside the same run. Commands
/// that never start a check (`--json`, CI, `DALO_OFFLINE=1`,
/// `DALO_UPDATE_CHECK=never`, non-interactive stderr) never wait at all.
const NOTICE_WAIT: Duration = Duration::from_millis(150);
/// Record of the last release check inside the update-notice cache directory.
const CHECK_RECORD: &str = "last-check";
/// Schema version of [`CHECK_RECORD`].
///
/// Bump it whenever the stored keys change meaning. A record written by a newer
/// schema is ignored, which makes the next run check again.
const CHECK_RECORD_SCHEMA: u32 = 1;

/// Outcome of one release check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckOutcome {
    /// The release API answered with a version newer than the running binary.
    Newer(String),
    /// The release API answered and the running binary is current.
    UpToDate,
    /// The check did not finish, so the next run retries it.
    Failed,
}

/// A passive update notice waiting to be printed.
///
/// The source stays private so the CLI cannot depend on how the notice was
/// obtained: an earlier run's record and a check started by this run print the
/// same way.
pub struct PendingNotice(NoticeSource);

enum NoticeSource {
    /// A newer release an earlier run already recorded.
    Recorded(String),
    /// A check started for this run.
    Running(Receiver<CheckOutcome>),
}

impl PendingNotice {
    fn recorded(latest: String) -> Self {
        Self(NoticeSource::Recorded(latest))
    }

    fn running(receiver: Receiver<CheckOutcome>) -> Self {
        Self(NoticeSource::Running(receiver))
    }

    /// Whether the notice needs no further wait.
    #[cfg(test)]
    fn is_recorded(&self) -> bool {
        matches!(self.0, NoticeSource::Recorded(_))
    }
}

/// What this invocation should do about the release check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckPlan {
    /// Announce this recorded release without touching the network.
    Announce(String),
    /// Start a check for this run.
    Check,
    /// Stay silent: a recent record already answers this run.
    Silent,
}

/// State of the recorded release check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckState {
    /// A check was started but never reported back.
    Pending,
    /// A check reached the release API and recorded its answer.
    Complete,
}

impl CheckState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Complete => "complete",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }

    /// How long a record in this state stays authoritative.
    fn interval(self) -> Duration {
        match self {
            Self::Pending => RETRY_INTERVAL,
            Self::Complete => CHECK_INTERVAL,
        }
    }
}

/// The persisted result of the last release check.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckRecord {
    state: CheckState,
    /// Version that ran the check. A different version invalidates the record.
    installed: String,
    /// Newer release the check found, if any.
    latest: Option<String>,
}

/// Start a passive release check when this invocation is eligible for one.
///
/// Update checks are advisory and fail open: network, cache, parsing, and
/// install-channel detection failures never change the requested command's
/// output or exit status.
pub fn start_notice_check() -> Option<PendingNotice> {
    if !update_checks_enabled() {
        return None;
    }

    let cache_dir = update_notice_cache_dir()?;
    start_notice_check_in(cache_dir, env!("CARGO_PKG_VERSION"))
}

fn start_notice_check_in(cache_dir: PathBuf, installed: &str) -> Option<PendingNotice> {
    match plan_check(read_check_record(&cache_dir), installed) {
        CheckPlan::Silent => None,
        CheckPlan::Announce(latest) => Some(PendingNotice::recorded(latest)),
        CheckPlan::Check => {
            // Claim the slot before the request so parallel and back-to-back
            // commands do not each open their own connection.
            write_check_record(
                &cache_dir,
                &CheckRecord {
                    state: CheckState::Pending,
                    installed: installed.to_owned(),
                    latest: None,
                },
            );
            let installed = installed.to_owned();
            spawn_notice_check(move || {
                let outcome = check_latest_version();
                record_outcome(&cache_dir, &installed, &outcome);
                outcome
            })
        }
    }
}

/// Decide what to do from the recorded check and its age.
fn plan_check(record: Option<(CheckRecord, Duration)>, installed: &str) -> CheckPlan {
    let Some((record, age)) = record else {
        // No record at all, including the first run after a fresh install.
        return CheckPlan::Check;
    };
    if record.installed != installed || age >= record.state.interval() {
        return CheckPlan::Check;
    }
    match (record.state, record.latest) {
        (CheckState::Complete, Some(latest)) => CheckPlan::Announce(latest),
        _ => CheckPlan::Silent,
    }
}

fn spawn_notice_check<F>(check: F) -> Option<PendingNotice>
where
    F: FnOnce() -> CheckOutcome + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("dalo-update-check".to_owned())
        .spawn(move || {
            let _ = sender.send(check());
        })
        .ok()?;
    Some(PendingNotice::running(receiver))
}

fn check_latest_version() -> CheckOutcome {
    // The interval is zero so `update_informer` performs the request instead of
    // consulting its own cache: the cadence is this module's `CHECK_RECORD`,
    // which does not seed itself with the running version and therefore lets a
    // fresh install check on its first interactive command.
    let informer = update_informer::new(DaloGitHub, REPOSITORY, env!("CARGO_PKG_VERSION"))
        .interval(Duration::ZERO)
        .timeout(CHECK_TIMEOUT);
    match informer.check_version() {
        Ok(Some(version)) => CheckOutcome::Newer(version.semver().to_string()),
        Ok(None) => CheckOutcome::UpToDate,
        Err(_) => CheckOutcome::Failed,
    }
}

/// Persist a finished check. An unfinished one keeps the pending record.
fn record_outcome(cache_dir: &Path, installed: &str, outcome: &CheckOutcome) {
    let latest = match outcome {
        CheckOutcome::Newer(version) => Some(version.clone()),
        CheckOutcome::UpToDate => None,
        CheckOutcome::Failed => return,
    };
    write_check_record(
        cache_dir,
        &CheckRecord {
            state: CheckState::Complete,
            installed: installed.to_owned(),
            latest,
        },
    );
}

fn render_check_record(record: &CheckRecord) -> String {
    let mut text = format!(
        "schema={CHECK_RECORD_SCHEMA}\nstate={}\ninstalled={}\n",
        record.state.as_str(),
        record.installed
    );
    if let Some(latest) = &record.latest {
        text.push_str(&format!("latest={latest}\n"));
    }
    text
}

fn parse_check_record(text: &str) -> Option<CheckRecord> {
    let mut schema = None;
    let mut state = None;
    let mut installed = None;
    let mut latest = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "schema" => schema = value.parse::<u32>().ok(),
            "state" => state = CheckState::parse(value),
            "installed" => installed = Some(value.to_owned()),
            "latest" => latest = Some(value.to_owned()),
            _ => {}
        }
    }
    if schema != Some(CHECK_RECORD_SCHEMA) {
        return None;
    }
    Some(CheckRecord {
        state: state?,
        installed: installed?,
        latest,
    })
}

/// Read the recorded check together with how long ago it was written.
fn read_check_record(cache_dir: &Path) -> Option<(CheckRecord, Duration)> {
    let path = cache_dir.join(CHECK_RECORD);
    let age = fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()?
        .elapsed()
        .unwrap_or_default();
    let record = parse_check_record(&fs::read_to_string(&path).ok()?)?;
    Some((record, age))
}

/// Write the record, ignoring failures: the check is advisory.
fn write_check_record(cache_dir: &Path, record: &CheckRecord) {
    if fs::create_dir_all(cache_dir).is_err() {
        return;
    }
    let _ = fs::write(cache_dir.join(CHECK_RECORD), render_check_record(record));
}

/// Print a passive release notice, waiting at most 150 milliseconds for a check
/// this run started.
pub fn print_notice_if_ready(pending: Option<PendingNotice>) {
    let Some(latest_version) = pending.and_then(take_notice) else {
        return;
    };
    if !mark_version_notified(&latest_version) {
        return;
    }

    let executable = env::current_exe().ok();
    let channel = detect_install_channel(executable.as_deref());
    eprintln!(
        "\n{}",
        render_notice(
            &latest_version,
            env!("CARGO_PKG_VERSION"),
            channel,
            executable.as_deref()
        )
    );
}

/// Resolve the notice, waiting for a running check only up to [`NOTICE_WAIT`].
///
/// A check that misses the window is not lost: the background thread records
/// its answer when it still can, and the next run announces it without waiting.
fn take_notice(pending: PendingNotice) -> Option<String> {
    match pending.0 {
        NoticeSource::Recorded(version) => Some(version),
        NoticeSource::Running(receiver) => match receiver.recv_timeout(NOTICE_WAIT) {
            Ok(CheckOutcome::Newer(version)) => Some(version),
            _ => None,
        },
    }
}

fn render_notice(
    latest_version: &str,
    installed_version: &str,
    channel: InstallChannel,
    executable: Option<&Path>,
) -> String {
    let mut notice = format!(
        "update available: dalo v{latest_version} (installed v{installed_version} via {})",
        channel.label()
    );
    if let Some(command) = channel.upgrade_command(executable) {
        notice.push_str(&format!("\nupgrade with: {command}"));
    } else {
        notice.push_str("\nupgrade guide: https://dalo.sh/install.md");
    }
    notice
}

fn update_checks_enabled() -> bool {
    update_checks_enabled_for(
        env::var("DALO_UPDATE_CHECK").ok().as_deref(),
        std::io::stderr().is_terminal(),
        env_truthy("CI"),
        env_truthy("DALO_OFFLINE"),
    )
}

fn update_checks_enabled_for(
    setting: Option<&str>,
    stderr_is_terminal: bool,
    ci: bool,
    offline: bool,
) -> bool {
    let disabled = setting.is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "never" | "off" | "false" | "0"
        )
    });
    !disabled && stderr_is_terminal && !ci && !offline
}

fn env_truthy(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            !matches!(
                value.to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "off"
            )
        })
        .unwrap_or(false)
}

fn mark_version_notified(version: &str) -> bool {
    let Some(cache_dir) = update_notice_cache_dir() else {
        return true;
    };
    mark_version_notified_in(&cache_dir, version)
}

fn mark_version_notified_in(cache_dir: &Path, version: &str) -> bool {
    if fs::create_dir_all(cache_dir).is_err() {
        return true;
    }

    let marker = cache_dir.join(format!("notified-{version}"));
    match OpenOptions::new().write(true).create_new(true).open(marker) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => true,
    }
}

fn update_notice_cache_dir() -> Option<PathBuf> {
    env::var_os("XDG_CACHE_HOME")
        .filter(|path| Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|cache| cache.join("dalo/update-notices"))
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

struct DaloGitHub;

impl Registry for DaloGitHub {
    const NAME: &'static str = "github-dalo";

    fn get_latest_version<T: HttpClient>(
        http_client: GenericHttpClient<T>,
        _package: &Package,
    ) -> update_informer::Result<Option<String>> {
        let release = http_client
            .add_header("Accept", "application/vnd.github+json")
            .add_header("User-Agent", "dalo-update-informer")
            .get::<GitHubRelease>(&release_api())?;

        Ok(normalize_release_tag(&release.tag_name).map(str::to_owned))
    }
}

/// The release endpoint to query.
#[cfg(not(test))]
fn release_api() -> String {
    RELEASE_API.to_owned()
}

/// Test-only endpoint override.
///
/// It exists behind `cfg(test)`, so it is compiled out of the shipped binary
/// and is not part of the CLI surface: unit tests point the check at a
/// loopback server instead of reaching the real release API.
#[cfg(test)]
static TEST_RELEASE_API: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Where an unconfigured test check goes: a closed loopback port, so a check
/// left running by an earlier test can never reach the real release API.
#[cfg(test)]
const TEST_UNREACHABLE_API: &str = "http://127.0.0.1:1/no-release-api-in-tests";

#[cfg(test)]
fn release_api() -> String {
    TEST_RELEASE_API
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| TEST_UNREACHABLE_API.to_owned())
}

fn normalize_release_tag(tag: &str) -> Option<&str> {
    tag.strip_prefix("dalo-v")
        .or_else(|| tag.strip_prefix('v'))
        .or_else(|| {
            tag.chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
                .then_some(tag)
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallChannel {
    Homebrew,
    Npm,
    Npx,
    Mise,
    MiseUbi,
    Cargo,
    Standalone,
    Unknown,
}

impl InstallChannel {
    fn label(self) -> &'static str {
        match self {
            Self::Homebrew => "Homebrew",
            Self::Npm => "npm",
            Self::Npx => "npx",
            Self::Mise | Self::MiseUbi => "mise",
            Self::Cargo => "Cargo",
            Self::Standalone => "the hosted installer",
            Self::Unknown => "an unknown installation method",
        }
    }

    fn upgrade_command(self, executable: Option<&Path>) -> Option<String> {
        match self {
            Self::Homebrew => Some("brew upgrade sebastian-software/tap/dalo".to_owned()),
            Self::Npm => Some("npm install --global getdalo@latest".to_owned()),
            Self::Npx => Some("npx getdalo@latest".to_owned()),
            Self::Mise => Some("mise upgrade github:sebastian-software/dalo".to_owned()),
            Self::MiseUbi => Some("mise upgrade ubi:sebastian-software/dalo".to_owned()),
            Self::Cargo => Some(cargo_upgrade_command()),
            Self::Standalone => standalone_upgrade_command(executable),
            Self::Unknown => None,
        }
    }
}

fn detect_install_channel(executable: Option<&Path>) -> InstallChannel {
    detect_install_channel_from(
        env::var("DALO_INSTALL_CHANNEL").ok().as_deref(),
        executable,
        env::var_os("HOME").as_deref().map(Path::new),
        env::var_os("CARGO_HOME").as_deref().map(Path::new),
    )
}

fn detect_install_channel_from(
    explicit: Option<&str>,
    executable: Option<&Path>,
    home: Option<&Path>,
    cargo_home: Option<&Path>,
) -> InstallChannel {
    if let Some(channel) = explicit.and_then(parse_install_channel) {
        return channel;
    }

    let Some(executable) = executable else {
        return InstallChannel::Unknown;
    };
    let components = executable
        .components()
        .map(|part| part.as_os_str())
        .collect::<Vec<_>>();

    if components
        .windows(2)
        .any(|parts| parts[0] == OsStr::new("Cellar") && parts[1] == OsStr::new("dalo"))
    {
        return InstallChannel::Homebrew;
    }

    if let Some(install_id) = components.windows(3).find_map(|parts| {
        (parts[0] == OsStr::new("mise") && parts[1] == OsStr::new("installs"))
            .then(|| parts[2].to_string_lossy().to_ascii_lowercase())
    }) {
        return if install_id == "ubi" || install_id.starts_with("ubi-") {
            InstallChannel::MiseUbi
        } else {
            InstallChannel::Mise
        };
    }

    let cargo_root = cargo_home
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".cargo")));
    if cargo_root
        .as_deref()
        .is_some_and(|root| executable == root.join("bin/dalo"))
        || executable
            .parent()
            .and_then(Path::parent)
            .is_some_and(|root| root.join(".crates2.json").is_file())
    {
        return InstallChannel::Cargo;
    }

    if has_standalone_receipt(executable) {
        return InstallChannel::Standalone;
    }

    InstallChannel::Unknown
}

fn parse_install_channel(channel: &str) -> Option<InstallChannel> {
    match channel.to_ascii_lowercase().as_str() {
        "homebrew" | "brew" => Some(InstallChannel::Homebrew),
        "npm" => Some(InstallChannel::Npm),
        "npx" => Some(InstallChannel::Npx),
        "mise" | "mise-github" => Some(InstallChannel::Mise),
        "mise-ubi" => Some(InstallChannel::MiseUbi),
        "cargo" | "cargo-binstall" => Some(InstallChannel::Cargo),
        "standalone" | "installer" => Some(InstallChannel::Standalone),
        _ => None,
    }
}

fn has_standalone_receipt(executable: &Path) -> bool {
    executable
        .parent()
        .and_then(|parent| fs::read_to_string(parent.join(INSTALL_RECEIPT)).ok())
        .is_some_and(|receipt| receipt.trim() == "standalone")
}

fn cargo_upgrade_command() -> String {
    if command_exists("cargo-binstall") {
        "cargo binstall dalo".to_owned()
    } else {
        "cargo install dalo --locked --force".to_owned()
    }
}

fn command_exists(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| is_executable_file(&directory.join(program)))
    })
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn standalone_upgrade_command(executable: Option<&Path>) -> Option<String> {
    let install_dir = executable?.parent()?;
    let default_dir = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local/bin"));
    if default_dir.as_deref() == Some(install_dir) {
        return Some("curl -fsSL https://dalo.sh/install.sh | sh".to_owned());
    }

    Some(format!(
        "curl -fsSL https://dalo.sh/install.sh | DALO_INSTALL_DIR={} sh",
        shell_quote(install_dir)
    ))
}

fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::Instant;
    use tempfile::tempdir;

    /// Upper bound for how long resolving a notice may take a command.
    ///
    /// Generous next to [`NOTICE_WAIT`] so a loaded runner cannot fail the
    /// test, tight enough that waiting for the request itself would.
    const WAIT_BOUND: Duration = Duration::from_secs(2);

    /// Serializes the tests that repoint the release endpoint.
    static RELEASE_API_GUARD: Mutex<()> = Mutex::new(());

    fn lock_release_api() -> MutexGuard<'static, ()> {
        RELEASE_API_GUARD
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn set_release_api(url: Option<String>) {
        *TEST_RELEASE_API
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = url;
    }

    /// A loopback stand-in for the release API, so no test leaves the machine.
    struct FakeReleaseEndpoint {
        url: String,
    }

    impl FakeReleaseEndpoint {
        /// An endpoint that answers every request with `body`.
        fn answering(body: &'static str) -> Self {
            Self::serve(Some(body))
        }

        /// An endpoint that accepts the connection and never answers.
        fn silent() -> Self {
            Self::serve(None)
        }

        fn serve(body: Option<&'static str>) -> Self {
            let listener =
                std::net::TcpListener::bind(("127.0.0.1", 0)).expect("loopback port should bind");
            let port = listener.local_addr().expect("local address").port();
            thread::spawn(move || {
                let mut held = Vec::new();
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut request = [0_u8; 1024];
                    let _ = std::io::Read::read(&mut stream, &mut request);
                    match body {
                        Some(body) => {
                            let response = format!(
                                "HTTP/1.1 200 OK\r\n\
                                 Content-Type: application/json\r\n\
                                 Content-Length: {}\r\n\
                                 Connection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
                        }
                        // Held open so the client waits rather than seeing EOF.
                        None => held.push(stream),
                    }
                }
            });
            Self {
                url: format!("http://127.0.0.1:{port}/releases/latest"),
            }
        }

        fn url(&self) -> String {
            self.url.clone()
        }
    }

    #[test]
    fn the_release_endpoint_should_point_at_this_project() {
        assert_eq!(
            RELEASE_API,
            format!("https://api.github.com/repos/{REPOSITORY}/releases/latest")
        );
    }

    #[test]
    fn release_tags_should_accept_dalo_prefix_and_reject_unrelated_tags() {
        assert_eq!(normalize_release_tag("dalo-v1.2.3"), Some("1.2.3"));
        assert_eq!(normalize_release_tag("v1.2.3"), Some("1.2.3"));
        assert_eq!(normalize_release_tag("1.2.3"), Some("1.2.3"));
        assert_eq!(normalize_release_tag("getdalo-v1.2.3"), None);
    }

    #[test]
    fn update_policy_should_require_an_interactive_online_non_ci_run() {
        assert!(update_checks_enabled_for(None, true, false, false));
        assert!(!update_checks_enabled_for(None, false, false, false));
        assert!(!update_checks_enabled_for(None, true, true, false));
        assert!(!update_checks_enabled_for(None, true, false, true));
        assert!(!update_checks_enabled_for(
            Some("NEVER"),
            true,
            false,
            false
        ));
    }

    #[test]
    fn unfinished_update_check_should_not_delay_command_completion() {
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let pending = spawn_notice_check(move || {
            started_sender.send(()).expect("test should observe start");
            release_receiver
                .recv()
                .expect("test should release update check");
            CheckOutcome::Newer("9.9.9".to_owned())
        })
        .expect("update thread should start");
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("update check should start");

        let start = Instant::now();
        assert_eq!(take_notice(pending), None);
        let waited = start.elapsed();
        assert!(
            waited >= NOTICE_WAIT,
            "a started check should get its bounded wait, waited {waited:?}"
        );
        assert!(
            waited < WAIT_BOUND,
            "an unfinished check should not hold the command, waited {waited:?}"
        );
        release_sender
            .send(())
            .expect("update check should be released");
    }

    #[test]
    fn completed_update_check_should_return_its_notice() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(CheckOutcome::Newer("9.9.9".to_owned()))
            .expect("notice should be queued");

        assert_eq!(
            take_notice(PendingNotice::running(receiver)),
            Some("9.9.9".to_owned())
        );
    }

    #[test]
    fn recorded_release_should_be_announced_without_waiting() {
        let start = Instant::now();
        assert_eq!(
            take_notice(PendingNotice::recorded("9.9.9".to_owned())),
            Some("9.9.9".to_owned())
        );
        assert!(start.elapsed() < NOTICE_WAIT);
    }

    #[test]
    fn up_to_date_check_should_not_produce_a_notice() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(CheckOutcome::UpToDate)
            .expect("outcome should be queued");

        assert_eq!(take_notice(PendingNotice::running(receiver)), None);
    }

    #[test]
    fn check_record_should_survive_a_round_trip_and_ignore_foreign_schemas() {
        let recorded = CheckRecord {
            state: CheckState::Complete,
            installed: "1.2.3".to_owned(),
            latest: Some("1.2.4".to_owned()),
        };
        assert_eq!(
            parse_check_record(&render_check_record(&recorded)),
            Some(recorded)
        );

        let up_to_date = CheckRecord {
            state: CheckState::Pending,
            installed: "1.2.3".to_owned(),
            latest: None,
        };
        assert_eq!(
            parse_check_record(&render_check_record(&up_to_date)),
            Some(up_to_date)
        );

        assert_eq!(
            parse_check_record("schema=9999\nstate=complete\ninstalled=1.2.3\n"),
            None
        );
        assert_eq!(parse_check_record("not a record"), None);
    }

    #[test]
    fn a_fresh_install_should_check_on_its_first_interactive_command() {
        assert_eq!(plan_check(None, "1.2.3"), CheckPlan::Check);
    }

    #[test]
    fn a_recorded_check_should_hold_for_a_day_and_then_check_again() {
        let record = CheckRecord {
            state: CheckState::Complete,
            installed: "1.2.3".to_owned(),
            latest: Some("1.2.4".to_owned()),
        };

        assert_eq!(
            plan_check(Some((record.clone(), Duration::from_secs(60))), "1.2.3"),
            CheckPlan::Announce("1.2.4".to_owned())
        );
        assert_eq!(
            plan_check(Some((record.clone(), CHECK_INTERVAL)), "1.2.3"),
            CheckPlan::Check
        );
        // A replaced binary invalidates the record whatever its age.
        assert_eq!(
            plan_check(Some((record, Duration::from_secs(60))), "1.2.4"),
            CheckPlan::Check
        );

        let current = CheckRecord {
            state: CheckState::Complete,
            installed: "1.2.3".to_owned(),
            latest: None,
        };
        assert_eq!(
            plan_check(Some((current, Duration::from_secs(60))), "1.2.3"),
            CheckPlan::Silent
        );
    }

    #[test]
    fn an_unfinished_check_should_be_retried_instead_of_burning_the_day() {
        let record = CheckRecord {
            state: CheckState::Pending,
            installed: "1.2.3".to_owned(),
            latest: None,
        };

        assert_eq!(
            plan_check(Some((record.clone(), Duration::from_secs(60))), "1.2.3"),
            CheckPlan::Silent
        );
        assert_eq!(
            plan_check(Some((record, RETRY_INTERVAL)), "1.2.3"),
            CheckPlan::Check
        );
    }

    #[test]
    fn only_a_finished_check_should_be_recorded() {
        let temp = tempdir().expect("tempdir");

        record_outcome(temp.path(), "1.2.3", &CheckOutcome::Failed);
        assert_eq!(read_check_record(temp.path()), None);

        record_outcome(
            temp.path(),
            "1.2.3",
            &CheckOutcome::Newer("1.2.4".to_owned()),
        );
        let (record, _) = read_check_record(temp.path()).expect("record should be written");
        assert_eq!(
            record,
            CheckRecord {
                state: CheckState::Complete,
                installed: "1.2.3".to_owned(),
                latest: Some("1.2.4".to_owned()),
            }
        );

        record_outcome(temp.path(), "1.2.3", &CheckOutcome::UpToDate);
        let (record, _) = read_check_record(temp.path()).expect("record should be written");
        assert_eq!(record.latest, None);
    }

    #[test]
    fn a_fast_command_should_deliver_the_notice_from_a_fake_endpoint() {
        let _guard = lock_release_api();
        let temp = tempdir().expect("tempdir");
        let endpoint = FakeReleaseEndpoint::answering(r#"{"tag_name":"dalo-v99.0.0"}"#);
        set_release_api(Some(endpoint.url()));

        let start = Instant::now();
        let pending = start_notice_check_in(temp.path().to_path_buf(), env!("CARGO_PKG_VERSION"))
            .expect("a fresh cache should start a check");
        let notice = take_notice(pending);
        let elapsed = start.elapsed();

        set_release_api(None);
        assert_eq!(notice, Some("99.0.0".to_owned()));
        assert!(
            elapsed < WAIT_BOUND,
            "a fast command should not be held up, took {elapsed:?}"
        );
        let (record, _) = read_check_record(temp.path()).expect("the check should be recorded");
        assert_eq!(record.state, CheckState::Complete);
        assert_eq!(record.latest.as_deref(), Some("99.0.0"));

        // The recorded release is announced again without any network access.
        set_release_api(Some("http://127.0.0.1:1/never-reached".to_owned()));
        let pending = start_notice_check_in(temp.path().to_path_buf(), env!("CARGO_PKG_VERSION"))
            .expect("the recorded release should still be announced");
        assert!(pending.is_recorded());
        assert_eq!(take_notice(pending), Some("99.0.0".to_owned()));
        set_release_api(None);
    }

    #[test]
    fn a_slow_endpoint_should_not_delay_a_fast_command() {
        let _guard = lock_release_api();
        let temp = tempdir().expect("tempdir");
        let endpoint = FakeReleaseEndpoint::silent();
        set_release_api(Some(endpoint.url()));

        let start = Instant::now();
        let pending = start_notice_check_in(temp.path().to_path_buf(), env!("CARGO_PKG_VERSION"))
            .expect("a fresh cache should start a check");
        let notice = take_notice(pending);
        let elapsed = start.elapsed();
        set_release_api(None);

        assert_eq!(notice, None);
        assert!(
            elapsed < WAIT_BOUND,
            "a silent endpoint should not be waited out, took {elapsed:?}"
        );
        let (record, _) = read_check_record(temp.path()).expect("the attempt should be recorded");
        assert_eq!(
            record.state,
            CheckState::Pending,
            "an unfinished check stays pending so the next run retries it"
        );
    }

    #[test]
    fn explicit_launcher_channel_should_win_over_path_detection() {
        let channel = detect_install_channel_from(
            Some("npx"),
            Some(Path::new("/home/user/.cache/dalo/1.2.3/dalo")),
            Some(Path::new("/home/user")),
            None,
        );

        assert_eq!(channel, InstallChannel::Npx);
        assert_eq!(
            channel.upgrade_command(None).as_deref(),
            Some("npx getdalo@latest")
        );
    }

    #[test]
    fn managed_install_paths_should_be_detected_conservatively() {
        assert_eq!(
            detect_install_channel_from(
                None,
                Some(Path::new("/opt/homebrew/Cellar/dalo/1.2.3/bin/dalo")),
                Some(Path::new("/Users/user")),
                None,
            ),
            InstallChannel::Homebrew
        );
        assert_eq!(
            detect_install_channel_from(
                None,
                Some(Path::new(
                    "/home/rubicon/.local/share/mise/installs/github-sebastian-software-dalo/latest/bin/dalo"
                )),
                Some(Path::new("/home/rubicon")),
                None,
            ),
            InstallChannel::Mise
        );
        assert_eq!(
            detect_install_channel_from(
                None,
                Some(Path::new(
                    "/home/user/.local/share/mise/installs/ubi-sebastian-software-dalo/latest/bin/dalo"
                )),
                Some(Path::new("/home/user")),
                None,
            ),
            InstallChannel::MiseUbi
        );
        assert_eq!(
            detect_install_channel_from(
                None,
                Some(Path::new("/home/user/.cargo/bin/dalo")),
                Some(Path::new("/home/user")),
                None,
            ),
            InstallChannel::Cargo
        );
    }

    #[test]
    fn installer_receipt_should_preserve_custom_install_directory() {
        let temp = tempdir().expect("tempdir");
        let executable = temp.path().join("custom bin/dalo");
        fs::create_dir_all(executable.parent().expect("parent")).expect("create bin dir");
        fs::write(
            executable.parent().expect("parent").join(INSTALL_RECEIPT),
            "standalone\n",
        )
        .expect("write receipt");

        let channel = detect_install_channel_from(None, Some(&executable), None, None);
        assert_eq!(channel, InstallChannel::Standalone);
        let expected = format!(
            "curl -fsSL https://dalo.sh/install.sh | DALO_INSTALL_DIR={} sh",
            shell_quote(executable.parent().expect("parent"))
        );
        assert_eq!(channel.upgrade_command(Some(&executable)), Some(expected));
    }

    #[test]
    fn bare_default_install_path_should_not_guess_standalone() {
        let channel = detect_install_channel_from(
            None,
            Some(Path::new("/home/user/.local/bin/dalo")),
            Some(Path::new("/home/user")),
            None,
        );

        assert_eq!(channel, InstallChannel::Unknown);
        assert_eq!(channel.upgrade_command(None), None);
    }

    #[test]
    fn unknown_installation_should_not_guess_an_upgrade_command() {
        let temp = tempdir().expect("tempdir");
        let executable = temp.path().join("bin/dalo");
        fs::create_dir_all(executable.parent().expect("parent")).expect("create bin dir");

        let channel = detect_install_channel_from(
            Some("not-a-channel"),
            Some(&executable),
            Some(temp.path()),
            None,
        );

        assert_eq!(channel, InstallChannel::Unknown);
        assert_eq!(channel.upgrade_command(None), None);
    }

    #[test]
    fn notice_should_include_channel_specific_or_generic_guidance() {
        let latest_version = update_informer::fake(DaloGitHub, "dalo", "1.2.3", "1.2.4")
            .check_version()
            .expect("fake version check should succeed")
            .expect("fake version check should return a version");

        assert_eq!(
            render_notice(
                &latest_version.semver().to_string(),
                "1.2.3",
                InstallChannel::Homebrew,
                None,
            ),
            "update available: dalo v1.2.4 (installed v1.2.3 via Homebrew)\n\
             upgrade with: brew upgrade sebastian-software/tap/dalo"
        );
        assert_eq!(
            render_notice(
                &latest_version.semver().to_string(),
                "1.2.3",
                InstallChannel::Unknown,
                None,
            ),
            "update available: dalo v1.2.4 (installed v1.2.3 via an unknown installation method)\n\
             upgrade guide: https://dalo.sh/install.md"
        );
    }

    #[test]
    fn major_release_notice_should_keep_a_single_version_prefix() {
        assert_eq!(normalize_release_tag("dalo-v1.0.0"), Some("1.0.0"));

        let latest_version = update_informer::fake(DaloGitHub, "dalo", "0.16.0", "1.0.0")
            .check_version()
            .expect("fake version check should succeed")
            .expect("fake version check should return a version");

        assert_eq!(
            render_notice(
                &latest_version.semver().to_string(),
                "0.16.0",
                InstallChannel::Npm,
                None,
            ),
            "update available: dalo v1.0.0 (installed v0.16.0 via npm)\n\
             upgrade with: npm install --global getdalo@latest"
        );
    }

    #[test]
    fn each_new_version_should_be_notified_only_once() {
        let temp = tempdir().expect("tempdir");

        assert!(mark_version_notified_in(temp.path(), "v1.2.3"));
        assert!(!mark_version_notified_in(temp.path(), "v1.2.3"));
        assert!(mark_version_notified_in(temp.path(), "v1.2.4"));
    }

    #[test]
    fn shell_quote_should_handle_single_quotes() {
        assert_eq!(
            shell_quote(Path::new("/tmp/dalo's bin")),
            "'/tmp/dalo'\"'\"'s bin'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_detection_should_require_an_executable_file() {
        let temp = tempdir().expect("tempdir");
        let command = temp.path().join("cargo-binstall");
        fs::write(&command, "not really a binary").expect("write command");

        let mut permissions = fs::metadata(&command).expect("metadata").permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&command, permissions).expect("remove execute permission");
        assert!(!is_executable_file(&command));

        let mut permissions = fs::metadata(&command).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command, permissions).expect("add execute permission");
        assert!(is_executable_file(&command));
    }
}
