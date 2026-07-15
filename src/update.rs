//! Passive release notices and install-channel-specific upgrade guidance.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::fs::OpenOptions;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use update_informer::http_client::{GenericHttpClient, HttpClient};
use update_informer::{Check, Package, Registry};

const REPOSITORY: &str = "sebastian-software/dalo";
const RELEASE_API: &str = "https://api.github.com/repos/sebastian-software/dalo/releases/latest";
const INSTALL_RECEIPT: &str = ".dalo-install-channel";
const CHECK_TIMEOUT: Duration = Duration::from_secs(1);

/// Print a passive release notice when this invocation is eligible for one.
///
/// Update checks are advisory and fail open: network, cache, parsing, and
/// install-channel detection failures never change the requested command's
/// output or exit status.
pub fn maybe_print_notice() {
    if !update_checks_enabled() {
        return;
    }

    let informer = update_informer::new(DaloGitHub, REPOSITORY, env!("CARGO_PKG_VERSION"))
        .timeout(CHECK_TIMEOUT);
    let Ok(Some(version)) = informer.check_version() else {
        return;
    };
    if !mark_version_notified(&version.to_string()) {
        return;
    }

    let executable = env::current_exe().ok();
    let channel = detect_install_channel(executable.as_deref());
    eprintln!(
        "\n{}",
        render_notice(
            &version.to_string(),
            env!("CARGO_PKG_VERSION"),
            channel,
            executable.as_deref()
        )
    );
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
            .get::<GitHubRelease>(RELEASE_API)?;

        Ok(normalize_release_tag(&release.tag_name).map(str::to_owned))
    }
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
    use tempfile::tempdir;

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
        let channel = detect_install_channel_from(
            Some("not-a-channel"),
            Some(Path::new("/usr/local/bin/dalo")),
            Some(Path::new("/home/user")),
            None,
        );

        assert_eq!(channel, InstallChannel::Unknown);
        assert_eq!(channel.upgrade_command(None), None);
    }

    #[test]
    fn notice_should_include_channel_specific_or_generic_guidance() {
        assert_eq!(
            render_notice("1.2.4", "1.2.3", InstallChannel::Homebrew, None),
            "update available: dalo v1.2.4 (installed v1.2.3 via Homebrew)\n\
             upgrade with: brew upgrade sebastian-software/tap/dalo"
        );
        assert_eq!(
            render_notice("1.2.4", "1.2.3", InstallChannel::Unknown, None),
            "update available: dalo v1.2.4 (installed v1.2.3 via an unknown installation method)\n\
             upgrade guide: https://dalo.sh/install.md"
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
