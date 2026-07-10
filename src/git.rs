//! Narrow wrapper around the system `git` command.

use std::ffi::OsStr;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};

const GIT_LOCAL_TIMEOUT: Duration = Duration::from_secs(60);
const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(300);
// Most local Git commands finish well below 50 ms. Poll at 1 ms so process
// collection does not impose a fixed latency floor on every command.
const GIT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const GIT_TIMEOUT_ENV: &str = "DALO_GIT_TIMEOUT_SECS";

/// Run `git init` in the provided directory.
pub fn init_repo(path: &Path) -> DaloResult<()> {
    run_git(path, &["init", "-q"]).map(|_| ())
}

/// Clone a Git repository.
pub fn clone_repo(url: &str, destination: &Path) -> DaloResult<()> {
    let cwd = destination.parent().unwrap_or_else(|| Path::new("."));
    let destination_arg = destination.to_string_lossy().into_owned();
    print_network_progress(&format!(
        "Cloning repository `{}`...",
        redact_url_userinfo(url)
    ));
    // `--` terminates option parsing so a user-supplied URL that looks like a
    // flag (e.g. `--upload-pack=...`) can never be treated as a git option.
    run_git_network(cwd, &["clone", "--quiet", "--", url, &destination_arg]).map(|_| ())
}

/// Reject Git URLs that embed userinfo so credentials never reach config files,
/// command displays, or Git's remote configuration.
pub fn validate_remote_url(url: &str) -> DaloResult<()> {
    if url_has_userinfo(url) {
        return Err(DaloError::UnsafeRemoteUrl);
    }
    Ok(())
}

/// Update the current tracking branch through a fast-forward-only pull.
pub fn pull_ff_only(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!("Refreshing source `{}`...", path.display()));
    run_git_network(path, &["pull", "--ff-only", "--quiet"]).map(|_| ())
}

/// Return whether a checkout has local changes.
pub fn is_dirty(path: &Path) -> DaloResult<bool> {
    let output = run_git(path, &["status", "--porcelain=v2"])?;
    Ok(!output.trim().is_empty())
}

/// Return the current HEAD commit.
pub fn rev_parse_head(path: &Path) -> DaloResult<String> {
    run_git(path, &["rev-parse", "HEAD"]).map(|output| output.trim().to_owned())
}

/// Resolve a fixed revision (such as `FETCH_HEAD`) to a commit hash.
pub fn rev_parse(path: &Path, revision: &str) -> DaloResult<String> {
    run_git(path, &["rev-parse", revision]).map(|output| output.trim().to_owned())
}

/// Read-only fetch of the remote's HEAD. Records it in `FETCH_HEAD` without
/// moving the working tree.
pub fn fetch(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!(
        "Checking upstream drift for `{}`...",
        path.display()
    ));
    run_git_network(path, &["fetch", "--quiet", "origin", "HEAD"]).map(|_| ())
}

/// Check a commit out into a detached worktree for read-only inspection. The
/// caller's own checkout (and pin) is left untouched.
pub fn add_detached_worktree(repo: &Path, dest: &Path, commit: &str) -> DaloResult<()> {
    let dest_arg = dest.to_string_lossy().into_owned();
    run_git(
        repo,
        &["worktree", "add", "--detach", "--quiet", &dest_arg, commit],
    )
    .map(|_| ())
}

/// Remove a worktree created with [`add_detached_worktree`].
pub fn remove_worktree(repo: &Path, dest: &Path) -> DaloResult<()> {
    let dest_arg = dest.to_string_lossy().into_owned();
    run_git(repo, &["worktree", "remove", "--force", &dest_arg]).map(|_| ())
}

/// Prune stale Git worktree administrative records.
pub fn prune_worktrees(repo: &Path) -> DaloResult<()> {
    run_git(repo, &["worktree", "prune"]).map(|_| ())
}

fn run_git(path: &Path, args: &[&str]) -> DaloResult<String> {
    run_git_program("git", path, args, git_timeout(GIT_LOCAL_TIMEOUT))
}

fn run_git_network(path: &Path, args: &[&str]) -> DaloResult<String> {
    run_git_program("git", path, args, git_timeout(GIT_NETWORK_TIMEOUT))
}

fn run_git_program(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
) -> DaloResult<String> {
    run_git_program_with_options(
        program,
        path,
        args,
        timeout,
        std::env::var_os("GIT_SSH_COMMAND"),
        has_core_ssh_command(path),
    )
}

fn run_git_program_with_options(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
    ssh_command_env: Option<impl AsRef<OsStr>>,
    core_ssh_command_configured: bool,
) -> DaloResult<String> {
    let stdout = NamedTempFile::new()?;
    let stderr = NamedTempFile::new()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?));
    configure_ssh_command(
        &mut command,
        ssh_command_env.as_ref(),
        core_ssh_command_configured,
    );
    let mut child = command.spawn()?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DaloError::Io(error));
            }
        }

        let elapsed = start.elapsed();
        if elapsed >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DaloError::CommandFailed {
                program: program.to_owned(),
                args: display_git_args(args),
                cwd: path.to_path_buf(),
                status: format!("timed out after {}", format_duration(timeout)),
                stderr: humanize_git_failure(args, &timeout_stderr(&stderr)),
            });
        }

        thread::sleep(GIT_POLL_INTERVAL.min(timeout - elapsed));
    };
    let stdout_text = read_tempfile_lossy(&stdout);
    let stderr_text = read_tempfile_lossy(&stderr).trim().to_owned();

    if status.success() {
        return Ok(stdout_text);
    }

    Err(DaloError::CommandFailed {
        program: program.to_owned(),
        args: display_git_args(args),
        cwd: path.to_path_buf(),
        status: status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        stderr: humanize_git_failure(args, &stderr_text),
    })
}

fn print_network_progress(message: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("{message}");
    }
}

fn humanize_git_failure(args: &[&str], stderr: &str) -> String {
    let raw = redact_urls_in_text(stderr.trim());
    let Some(summary) = git_failure_summary(args) else {
        return raw;
    };
    if raw.is_empty() {
        return summary;
    }
    format!("{summary}\n\nGit said: {raw}")
}

fn git_failure_summary(args: &[&str]) -> Option<String> {
    match args {
        ["clone", .., "--", url, _destination] => Some(format!(
            "Could not clone repository `{}`. Check the URL, network/proxy access, and repository permissions.",
            redact_url_userinfo(url)
        )),
        ["pull", ..] => Some(
            "Could not refresh this source. Check network/proxy access, repository permissions, and whether the tracking branch can fast-forward."
                .to_owned(),
        ),
        ["fetch", ..] => Some(
            "Could not check the repository for upstream changes. Check network/proxy access and repository permissions."
                .to_owned(),
        ),
        _ => None,
    }
}

fn display_git_args(args: &[&str]) -> String {
    args.iter()
        .map(|arg| redact_url_userinfo(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn url_has_userinfo(url: &str) -> bool {
    let Some(scheme_end) = url.find("://") else {
        return false;
    };
    let authority = &url[scheme_end + 3..];
    let authority_end = authority
        .find(|character: char| ['/', '?', '#'].contains(&character))
        .unwrap_or(authority.len());
    authority[..authority_end].contains('@')
}

fn redact_url_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority = &url[authority_start..];
    let authority_end = authority
        .find(|character: char| ['/', '?', '#'].contains(&character))
        .unwrap_or(authority.len());
    let Some(userinfo_end) = authority[..authority_end].rfind('@') else {
        return url.to_owned();
    };

    format!(
        "{}***@{}",
        &url[..authority_start],
        &authority[userinfo_end + 1..]
    )
}

fn redact_urls_in_text(text: &str) -> String {
    text.split_whitespace()
        .map(redact_url_userinfo)
        .collect::<Vec<_>>()
        .join(" ")
}

fn configure_ssh_command(
    command: &mut Command,
    ssh_command_env: Option<&impl AsRef<OsStr>>,
    core_ssh_command_configured: bool,
) {
    if let Some(value) = ssh_command_env {
        command.env("GIT_SSH_COMMAND", value.as_ref());
    } else if !core_ssh_command_configured {
        command.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    } else {
        command.env_remove("GIT_SSH_COMMAND");
    }
}

fn has_core_ssh_command(path: &Path) -> bool {
    Command::new("git")
        .args(["config", "--get", "core.sshCommand"])
        .current_dir(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn git_timeout(default: Duration) -> Duration {
    std::env::var(GIT_TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(default)
}

fn read_tempfile_lossy(file: &NamedTempFile) -> String {
    fs::read(file.path())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn timeout_stderr(stderr: &NamedTempFile) -> String {
    let text = read_tempfile_lossy(stderr).trim().to_owned();
    if text.is_empty() {
        "git command timed out; terminal prompts are disabled".to_owned()
    } else {
        text
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}s", duration.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    #[test]
    fn run_git_program_should_report_missing_binary() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");

        let error = run_git_program(
            "dalo-definitely-missing-git-binary",
            temp_dir.path(),
            &["--version"],
            Duration::from_millis(10),
        )
        .expect_err("missing binary should fail");

        assert!(matches!(error, DaloError::Io(_)));
    }

    #[test]
    fn run_git_program_should_disable_interactive_prompts() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nprintf 'prompt=%s ssh=%s\\n' \"$GIT_TERMINAL_PROMPT\" \"$GIT_SSH_COMMAND\" >&2\nexit 2\n",
        );

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["pull"],
            Duration::from_secs(1),
            Option::<&str>::None,
            false,
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("prompt=0"));
        assert!(stderr.contains("BatchMode=yes"));
    }

    #[test]
    fn run_git_program_should_preserve_user_ssh_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nprintf 'ssh=%s\\n' \"$GIT_SSH_COMMAND\" >&2\nexit 2\n",
        );

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["fetch"],
            Duration::from_secs(1),
            Some("ssh -i deploy-key -oBatchMode=yes"),
            false,
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("ssh=ssh -i deploy-key -oBatchMode=yes"));
    }

    #[test]
    fn run_git_program_should_not_override_core_ssh_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nprintf \"ssh=${GIT_SSH_COMMAND:-unset}\\n\" >&2\nexit 2\n",
        );

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["fetch"],
            Duration::from_secs(1),
            Option::<&str>::None,
            true,
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("ssh=unset"));
        assert!(!stderr.contains("BatchMode=yes"));
    }

    #[test]
    fn run_git_program_should_timeout_hung_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nwhile :; do :; done\n",
        );

        let error = run_git_program(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["pull"],
            Duration::from_millis(10),
        )
        .expect_err("hung command should time out");

        let DaloError::CommandFailed { status, stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(status.contains("timed out after"));
        assert!(stderr.contains("terminal prompts are disabled"));
    }

    #[test]
    fn git_poll_interval_should_not_add_a_50ms_latency_floor() {
        assert!(GIT_POLL_INTERVAL < Duration::from_millis(50));
    }

    #[test]
    fn humanize_git_failure_should_explain_clone_errors() {
        let message = humanize_git_failure(
            &[
                "clone",
                "--quiet",
                "--",
                "https://example.invalid/repo.git",
                "/tmp/checkout",
            ],
            "fatal: unable to access repository",
        );

        assert!(message.contains("Could not clone repository"));
        assert!(message.contains("https://example.invalid/repo.git"));
        assert!(message.contains("Git said: fatal: unable to access repository"));
    }

    #[test]
    fn humanize_git_failure_should_redact_url_userinfo() {
        let secret_url = "https://octo:token-value@example.invalid/repo.git";
        let message = humanize_git_failure(
            &["clone", "--quiet", "--", secret_url, "/tmp/checkout"],
            &format!("fatal: unable to access '{secret_url}': denied"),
        );

        assert!(message.contains("https://***@example.invalid/repo.git"));
        assert!(!message.contains("token-value"));
    }

    #[test]
    fn validate_remote_url_should_reject_userinfo() {
        assert!(matches!(
            validate_remote_url("https://octo:token-value@example.invalid/repo.git"),
            Err(DaloError::UnsafeRemoteUrl)
        ));
        assert!(validate_remote_url("git@github.com:sebastian-software/dalo.git").is_ok());
    }

    #[test]
    fn humanize_git_failure_should_explain_tracking_refresh_errors() {
        let message =
            humanize_git_failure(&["pull", "--ff-only", "--quiet"], "fatal: not possible");

        assert!(message.contains("Could not refresh this source"));
        assert!(message.contains("Git said: fatal: not possible"));
    }

    #[test]
    fn clone_repo_should_treat_dash_prefixed_url_as_repository_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let repo = temp_dir.path().join("--upload-pack=touch-owned");
        let destination = temp_dir.path().join("checkout");
        fs::create_dir_all(&repo).expect("repo dir should be created");
        run_git_program(
            "git",
            &repo,
            &["init", "-q", "--bare"],
            Duration::from_secs(5),
        )
        .expect("bare repo should be initialized");

        clone_repo(
            repo.file_name()
                .expect("repo should have file name")
                .to_str()
                .expect("repo name should be utf-8"),
            &destination,
        )
        .expect("dash-prefixed repo path should clone after -- separator");

        assert!(destination.join(".git").is_dir());
    }

    fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("script should be written");
        let mut permissions = fs::metadata(&path)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script should be executable");
        path
    }
}
