//! Passive version checks through the shared app-tracking client, with Dalo-owned guidance.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use app_tracking::{Mode, Tool, UpdateClient};

// Public, update-only service origin. The request carries no installed version or identity.
const VERSION_ORIGIN: &str = "https://sebastian-app-tracking.b-cdn.net";
const INSTALL_RECEIPT: &str = ".dalo-install-channel";

/// Check and print an advisory notice after an eligible successful command.
/// The shared client completes a due request synchronously with a 200 ms network
/// deadline. It owns hourly caching, bounded retry backoff, and notice throttling.
pub fn print_notice() {
    if !update_checks_enabled() {
        return;
    }
    let Some(cache_dir) = update_notice_cache_dir() else {
        return;
    };
    let executable = env::current_exe().ok();
    let channel = detect_install_channel(executable.as_deref());
    if let Some(version) = check_notice_in(
        VERSION_ORIGIN,
        &cache_dir,
        env!("CARGO_PKG_VERSION"),
        Mode::Enabled,
    ) {
        let _ = writeln!(
            std::io::stderr(),
            "\n{}",
            render_notice(
                &version,
                env!("CARGO_PKG_VERSION"),
                channel,
                executable.as_deref()
            )
        );
    }
}

fn check_notice_in(origin: &str, cache_dir: &Path, installed: &str, mode: Mode) -> Option<String> {
    let tool = Tool::new("dalo", installed, "github", "stable").ok()?;
    UpdateClient::new(origin, cache_dir, tool)
        .ok()?
        .check(mode)
        .ok()
        .flatten()
        .map(|update| update.latest_version)
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
        let guide = if channel == InstallChannel::DebianPackage {
            "https://dalo.sh/install.md#debian-packages"
        } else {
            "https://dalo.sh/install.md"
        };
        notice.push_str(&format!("\nupgrade guide: {guide}"));
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

fn update_notice_cache_dir() -> Option<PathBuf> {
    env::var_os("XDG_CACHE_HOME")
        .filter(|path| Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|cache| cache.join("dalo/update-notices/app-tracking-v1"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallChannel {
    DebianPackage,
    Homebrew,
    Npm,
    Npx,
    Mise,
    MiseUbi,
    Nix,
    Cargo,
    Standalone,
    Unknown,
}

impl InstallChannel {
    fn label(self) -> &'static str {
        match self {
            Self::DebianPackage => "Debian package",
            Self::Homebrew => "Homebrew",
            Self::Npm => "npm",
            Self::Npx => "npx",
            Self::Mise | Self::MiseUbi => "mise",
            Self::Nix => "Nix",
            Self::Cargo => "Cargo",
            Self::Standalone => "the hosted installer",
            Self::Unknown => "an unknown installation method",
        }
    }

    fn upgrade_command(self, executable: Option<&Path>) -> Option<String> {
        match self {
            Self::DebianPackage => None,
            Self::Homebrew => Some("brew upgrade sebastian-software/tap/dalo".to_owned()),
            Self::Npm => Some("npm install --global getdalo@latest".to_owned()),
            Self::Npx => Some("npx getdalo@latest".to_owned()),
            Self::Mise => Some("mise upgrade github:sebastian-software/dalo".to_owned()),
            Self::MiseUbi => Some("mise upgrade ubi:sebastian-software/dalo".to_owned()),
            Self::Nix => Some("nix profile upgrade dalo".to_owned()),
            Self::Cargo => Some(cargo_upgrade_command()),
            Self::Standalone => standalone_upgrade_command(executable),
            Self::Unknown => None,
        }
    }
}

fn detect_install_channel(executable: Option<&Path>) -> InstallChannel {
    detect_install_channel_from_with_debian_marker(
        env::var("DALO_INSTALL_CHANNEL").ok().as_deref(),
        executable,
        env::var_os("HOME").as_deref().map(Path::new),
        env::var_os("CARGO_HOME").as_deref().map(Path::new),
        Some(Path::new("/usr/share/dalo/.dalo-install-channel")),
    )
}

#[cfg(test)]
fn detect_install_channel_from(
    explicit: Option<&str>,
    executable: Option<&Path>,
    home: Option<&Path>,
    cargo_home: Option<&Path>,
) -> InstallChannel {
    detect_install_channel_from_with_debian_marker(explicit, executable, home, cargo_home, None)
}

fn detect_install_channel_from_with_debian_marker(
    explicit: Option<&str>,
    executable: Option<&Path>,
    home: Option<&Path>,
    cargo_home: Option<&Path>,
    debian_marker: Option<&Path>,
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

    if is_debian_package(executable, debian_marker) {
        return InstallChannel::DebianPackage;
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

    if let Some(channel) = read_install_receipt(executable) {
        return channel;
    }

    InstallChannel::Unknown
}

fn parse_install_channel(channel: &str) -> Option<InstallChannel> {
    match channel.to_ascii_lowercase().as_str() {
        "deb" | "debian" | "apt" => Some(InstallChannel::DebianPackage),
        "homebrew" | "brew" => Some(InstallChannel::Homebrew),
        "npm" => Some(InstallChannel::Npm),
        "npx" => Some(InstallChannel::Npx),
        "mise" | "mise-github" => Some(InstallChannel::Mise),
        "mise-ubi" => Some(InstallChannel::MiseUbi),
        "nix" => Some(InstallChannel::Nix),
        "cargo" | "cargo-binstall" => Some(InstallChannel::Cargo),
        "standalone" | "installer" => Some(InstallChannel::Standalone),
        _ => None,
    }
}

fn read_install_receipt(executable: &Path) -> Option<InstallChannel> {
    executable
        .parent()
        .and_then(|parent| fs::read_to_string(parent.join(INSTALL_RECEIPT)).ok())
        .and_then(|receipt| match receipt.trim() {
            "standalone" => Some(InstallChannel::Standalone),
            "nix" => Some(InstallChannel::Nix),
            _ => None,
        })
}

fn has_debian_install_marker(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|receipt| receipt.trim() == "debian")
}

fn is_debian_package(executable: &Path, marker: Option<&Path>) -> bool {
    executable == Path::new("/usr/bin/dalo") && marker.is_some_and(has_debian_install_marker)
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

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    fn payload(distribution: &str, age: i64, stale: bool) -> String {
        let checked = OffsetDateTime::now_utc() - time::Duration::seconds(age);
        serde_json::json!({
            "schema_version": 1, "tool_id": "dalo", "distribution": distribution,
            "channel": "stable", "latest_version": "99.0.0",
            "published_at": (checked - time::Duration::days(1)).format(&Rfc3339).unwrap(),
            "checked_at": checked.format(&Rfc3339).unwrap(),
            "release_url": "https://example.com/releases/99.0.0", "stale": stale,
        })
        .to_string()
    }

    fn endpoint(
        status: u16,
        body: String,
        delay: Duration,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let _ = tx.send(String::from_utf8(request).unwrap());
            thread::sleep(delay);
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (origin, rx, task)
    }

    #[test]
    fn update_policy_preserves_interactive_online_non_ci_eligibility() {
        assert!(update_checks_enabled_for(None, true, false, false));
        for setting in ["never", "NEVER", "off", "FALSE", "0"] {
            assert!(!update_checks_enabled_for(
                Some(setting),
                true,
                false,
                false
            ));
        }
        assert!(!update_checks_enabled_for(None, false, false, false));
        assert!(!update_checks_enabled_for(None, true, true, false));
        assert!(!update_checks_enabled_for(None, true, false, true));
    }

    #[test]
    fn all_installations_share_the_release_feed_and_keep_host_guidance() {
        for (channel, guidance) in [
            (InstallChannel::Npm, "npm install --global getdalo@latest"),
            (InstallChannel::Npx, "npx getdalo@latest"),
            (
                InstallChannel::Mise,
                "mise upgrade github:sebastian-software/dalo",
            ),
            (
                InstallChannel::MiseUbi,
                "mise upgrade ubi:sebastian-software/dalo",
            ),
            (
                InstallChannel::Homebrew,
                "brew upgrade sebastian-software/tap/dalo",
            ),
            (InstallChannel::Cargo, "cargo install dalo --locked"),
            (InstallChannel::Nix, "nix profile upgrade"),
            (InstallChannel::DebianPackage, "https://dalo.sh/install.md"),
            (InstallChannel::Standalone, "https://dalo.sh/install.md"),
            (InstallChannel::Unknown, "https://dalo.sh/install.md"),
        ] {
            let temp = tempdir().unwrap();
            let (origin, request, task) =
                endpoint(200, payload("github", 0, false), Duration::ZERO);
            let latest = check_notice_in(&origin, temp.path(), "1.0.0", Mode::Enabled).unwrap();
            assert_eq!(latest, "99.0.0");
            assert!(render_notice(&latest, "1.0.0", channel, None).contains(guidance));
            let request = request.recv().unwrap();
            assert!(request.starts_with("GET /v1/versions/dalo/github/stable HTTP/1.1\r\n"));
            assert!(!request.contains("1.0.0"));
            assert!(!request.to_ascii_lowercase().contains("authorization:"));
            task.join().unwrap();
            // The now-closed endpoint cannot receive a second request; installation
            // detection does not partition the shared cache or notice throttle.
            assert!(check_notice_in(&origin, temp.path(), "1.0.0", Mode::Enabled).is_none());
        }
    }

    #[test]
    fn disabled_and_offline_checks_create_no_update_state() {
        let temp = tempdir().unwrap();
        let absent = temp.path().join("absent");
        for mode in [Mode::Disabled, Mode::Offline] {
            assert!(check_notice_in("http://127.0.0.1:1", &absent, "1.0.0", mode).is_none());
        }
        assert!(!absent.exists());
    }

    #[test]
    fn stale_data_and_network_failures_remain_advisory_and_bounded() {
        let temp = tempdir().unwrap();
        let (origin, _, task) = endpoint(200, payload("github", 7200, true), Duration::ZERO);
        assert_eq!(
            check_notice_in(&origin, temp.path(), "1.0.0", Mode::Enabled).as_deref(),
            Some("99.0.0")
        );
        task.join().unwrap();
        let state: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path().join("dalo.github.stable.json")).unwrap())
                .unwrap();
        assert_eq!(state["failures"], 1);
        for (status, body, delay) in [
            (503, "not JSON".to_owned(), Duration::ZERO),
            (200, payload("github", 86400, true), Duration::ZERO),
            (200, payload("github", 0, false), Duration::from_millis(400)),
        ] {
            let temp = tempdir().unwrap();
            let (origin, _, task) = endpoint(status, body, delay);
            let start = Instant::now();
            assert!(check_notice_in(&origin, temp.path(), "1.0.0", Mode::Enabled).is_none());
            assert!(start.elapsed() < Duration::from_secs(2));
            task.join().unwrap();
            assert!(check_notice_in(&origin, temp.path(), "1.0.0", Mode::Enabled).is_none());
        }
    }

    #[test]
    fn legacy_records_are_left_untouched_and_do_not_drive_the_new_client() {
        let temp = tempdir().unwrap();
        let legacy = temp.path().join("last-check");
        fs::write(
            &legacy,
            "schema=1\nstate=complete\ninstalled=1.0.0\nlatest=100.0.0\n",
        )
        .unwrap();
        let (origin, _, task) = endpoint(200, payload("github", 0, false), Duration::ZERO);
        assert_eq!(
            check_notice_in(
                &origin,
                &temp.path().join("app-tracking-v1"),
                "1.0.0",
                Mode::Enabled
            )
            .as_deref(),
            Some("99.0.0")
        );
        task.join().unwrap();
        assert!(
            fs::read_to_string(legacy)
                .unwrap()
                .contains("latest=100.0.0")
        );
    }

    #[test]
    #[ignore = "opt-in read-only smoke against the public Bunny version service"]
    fn live_bunny_version_client_smoke() {
        for channel in [
            InstallChannel::Npm,
            InstallChannel::Homebrew,
            InstallChannel::Standalone,
        ] {
            let distribution = "github";
            // Warm public metadata before exercising the CLI's stricter deadline.
            // No service state is changed and no fake release is published.
            let preflight: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .build()
                .into();
            let response = preflight
                .get(format!(
                    "{VERSION_ORIGIN}/v1/versions/dalo/{distribution}/stable"
                ))
                .call()
                .unwrap();
            assert_eq!(response.status(), 200);
            let temp = tempdir().unwrap();
            let latest = check_notice_in(
                VERSION_ORIGIN,
                temp.path(),
                "0.0.0",
                Mode::Enabled,
            )
            .expect(
                "live metadata must identify a newer release for the synthetic installed version",
            );
            assert_ne!(latest, "0.0.0");
            let notice = render_notice(
                &latest,
                "0.0.0",
                channel,
                Some(Path::new("/tmp/dalo-smoke/bin/dalo")),
            );
            assert!(notice.contains("update available: dalo v"));
            assert!(notice.contains("upgrade with:"));
            println!("Bunny {distribution}: version comparison and host guidance passed");
        }
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
    fn only_a_marked_system_binary_should_be_detected_as_a_debian_package() {
        let temp = tempdir().expect("tempdir");
        let marker = temp.path().join(".dalo-install-channel");
        fs::write(&marker, "debian\n").expect("write Debian marker");

        assert!(is_debian_package(Path::new("/usr/bin/dalo"), Some(&marker)));
        assert!(!is_debian_package(Path::new("/usr/bin/dalo"), None));
        assert!(!is_debian_package(
            Path::new("/usr/local/bin/dalo"),
            Some(&marker)
        ));
        assert!(!is_debian_package(
            Path::new("/usr/bin/dalo"),
            Some(&temp.path().join("missing-marker"))
        ));
    }

    #[test]
    fn debian_install_marker_should_be_required_and_recognized() {
        let temp = tempdir().expect("tempdir");
        let marker = temp.path().join(".dalo-install-channel");

        assert!(!has_debian_install_marker(&marker));
        fs::write(&marker, "debian\n").expect("write Debian marker");
        assert!(has_debian_install_marker(&marker));
        fs::write(&marker, "standalone\n").expect("replace marker");
        assert!(!has_debian_install_marker(&marker));
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
    fn nix_receipt_should_select_nix_profile_upgrade() {
        let temp = tempdir().expect("tempdir");
        let executable = temp.path().join("bin/.dalo-wrapped");
        fs::create_dir_all(executable.parent().expect("parent")).expect("create bin dir");
        fs::write(
            executable.parent().expect("parent").join(INSTALL_RECEIPT),
            "nix\n",
        )
        .expect("write receipt");

        let channel = detect_install_channel_from(None, Some(&executable), None, None);
        assert_eq!(channel, InstallChannel::Nix);
        assert_eq!(
            channel.upgrade_command(Some(&executable)).as_deref(),
            Some("nix profile upgrade dalo")
        );
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
        let latest_version = "1.2.4";

        assert_eq!(
            render_notice(latest_version, "1.2.3", InstallChannel::Homebrew, None,),
            "update available: dalo v1.2.4 (installed v1.2.3 via Homebrew)\n\
             upgrade with: brew upgrade sebastian-software/tap/dalo"
        );
        assert_eq!(
            render_notice(latest_version, "1.2.3", InstallChannel::Unknown, None,),
            "update available: dalo v1.2.4 (installed v1.2.3 via an unknown installation method)\n\
             upgrade guide: https://dalo.sh/install.md"
        );

        assert_eq!(
            render_notice(latest_version, "1.2.3", InstallChannel::DebianPackage, None,),
            "update available: dalo v1.2.4 (installed v1.2.3 via Debian package)\n\
             upgrade guide: https://dalo.sh/install.md#debian-packages"
        );
    }

    #[test]
    fn major_release_notice_should_keep_a_single_version_prefix() {
        let latest_version = "1.0.0";

        assert_eq!(
            render_notice(latest_version, "0.16.0", InstallChannel::Npm, None,),
            "update available: dalo v1.0.0 (installed v0.16.0 via npm)\n\
             upgrade with: npm install --global getdalo@latest"
        );
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
