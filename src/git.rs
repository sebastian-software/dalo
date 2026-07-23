//! Narrow wrapper around the system `git` command.

use std::ffi::OsStr;
use std::fs;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process_group};
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

/// Return a remote location that is safe to include in human or JSON output.
#[must_use]
pub fn display_remote_url(url: &str) -> String {
    redact_url_userinfo(url)
}

/// Update the current tracking branch through a fast-forward-only pull.
pub fn pull_ff_only(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!("Refreshing source `{}`...", path.display()));
    run_git_network(path, &["pull", "--ff-only", "--quiet"]).map(|_| ())
}

/// Return whether a checkout has local changes to tracked files.
///
/// Untracked files are ignored: a fast-forward or reset never destroys them, so
/// a stray file (for example macOS `.DS_Store`) must not make a dalo-managed
/// checkout look dirty and block refresh or sync. Only tracked modifications,
/// staged changes, and unresolved merges count.
pub fn is_dirty(path: &Path) -> DaloResult<bool> {
    let output = run_git(path, &["status", "--porcelain=v2", "--untracked-files=no"])?;
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

/// Fetch the configured upstream without moving the current checkout.
pub fn fetch_upstream(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!(
        "Staging source refresh for `{}`...",
        path.display()
    ));
    run_git_network(path, &["fetch", "--quiet"]).map(|_| ())
}

/// Resolve a manifest-declared revision to a concrete commit.
///
/// Remote branches are preferred over local branches so a freshly fetched
/// branch name cannot accidentally resolve to a stale local tracking branch.
pub fn resolve_manifest_revision(path: &Path, revision: &str) -> DaloResult<String> {
    validate_manifest_revision(revision)?;

    let remote = format!("refs/remotes/origin/{revision}^{{commit}}");
    match rev_parse(path, &remote) {
        Ok(commit) => Ok(commit),
        Err(_) => {
            let requested = format!("{revision}^{{commit}}");
            rev_parse(path, &requested)
        }
    }
}

/// Validate a human-authored manifest revision before it reaches Git.
pub fn validate_manifest_revision(revision: &str) -> DaloResult<()> {
    // A manifest pin must name a single concrete commit, tag, or ref -- not a
    // Git revision expression. Reject empty/flag-like/whitespace values and the
    // range (`..`), reflog (`@{`), ancestry (`^`, `~`), and refspec/glob
    // (`:`, `?`, `*`, `[`, `\`) operators, plus control characters. This still
    // accepts commit hashes, `v1.0.0`, `main`, and `release/2024`.
    let has_operator = revision.contains(['^', '~', ':', '?', '*', '[', '\\']);
    if revision.is_empty()
        || revision.starts_with('-')
        || revision
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || revision.contains("..")
        || revision.contains("@{")
        || has_operator
    {
        Err(DaloError::CheckFailed {
            reason: format!("invalid manifest Git revision `{revision}`"),
        })
    } else {
        Ok(())
    }
}

/// Move a clean managed checkout to an exact detached commit.
pub fn checkout_detached(path: &Path, commit: &str) -> DaloResult<()> {
    run_git(
        path,
        &["checkout", "--detach", "--force", "--quiet", commit],
    )
    .map(|_| ())
}

/// Count commits reachable from `to` but not from `from`.
pub fn revision_count(path: &Path, from: &str, to: &str) -> DaloResult<usize> {
    let range = format!("{from}..{to}");
    let output = run_git(path, &["rev-list", "--count", &range])?;
    output.trim().parse().map_err(|error| {
        DaloError::Io(std::io::Error::other(format!(
            "git returned an invalid revision count `{}`: {error}",
            output.trim()
        )))
    })
}

/// Fast-forward the current branch to an already fetched revision.
pub fn fast_forward_to(path: &Path, revision: &str) -> DaloResult<()> {
    run_git(path, &["merge", "--ff-only", "--quiet", revision]).map(|_| ())
}

/// Restore a clean managed checkout to a previously recorded commit.
///
/// Callers must verify that the checkout is clean before beginning the
/// transaction. This is intentionally reserved for command-level rollback.
pub fn reset_hard_to(path: &Path, revision: &str) -> DaloResult<()> {
    run_git(path, &["reset", "--hard", "--quiet", revision]).map(|_| ())
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
    let ssh_command_env = std::env::var_os("GIT_SSH_COMMAND");
    let preflight_timeout = timeout.min(git_timeout(GIT_LOCAL_TIMEOUT));
    run_git_program_with_options(
        program,
        path,
        args,
        timeout,
        ssh_command_env,
        has_core_ssh_command(program, path, preflight_timeout),
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
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_git_process(&mut child);
                return Err(DaloError::Io(error));
            }
        }

        let elapsed = start.elapsed();
        if elapsed >= timeout {
            terminate_git_process(&mut child);
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

/// Terminate Git and any remote helpers it spawned, then reap the direct child.
fn terminate_git_process(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = Pid::from_child(child);
        if kill_process_group(process_group, Signal::KILL).is_ok() {
            let _ = child.wait();
            return;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
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

fn has_core_ssh_command(program: &str, path: &Path, timeout: Duration) -> bool {
    run_git_program_with_options(
        program,
        path,
        &["config", "--get", "core.sshCommand"],
        timeout,
        Option::<&str>::None,
        true,
    )
    .is_ok()
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
    fn run_git_program_should_terminate_helper_processes_on_timeout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let helper_pid_file = temp_dir.path().join("helper.pid");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nsh -c 'while :; do sleep 1; done' &\nprintf '%s\\n' \"$!\" > \"$1\"\nwhile :; do sleep 1; done\n",
        );
        let helper_pid_file_arg = helper_pid_file
            .to_str()
            .expect("helper PID path should be utf-8");

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &[helper_pid_file_arg],
            Duration::from_secs(1),
            Option::<&str>::None,
            false,
        )
        .expect_err("hung command should time out");

        assert!(matches!(error, DaloError::CommandFailed { .. }));
        let helper_pid = fs::read_to_string(&helper_pid_file)
            .expect("helper PID should be written")
            .trim()
            .parse::<i32>()
            .expect("helper PID should be numeric");
        let helper_pid = Pid::from_raw(helper_pid).expect("helper PID should be positive");

        for _ in 0..100 {
            if process_is_gone_or_zombie(helper_pid) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timeout should terminate the helper process");
    }

    fn process_is_gone_or_zombie(pid: Pid) -> bool {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("ps should run");
        !output.status.success()
            || String::from_utf8_lossy(&output.stdout)
                .trim_start()
                .starts_with('Z')
    }

    #[test]
    fn core_ssh_preflight_should_share_the_git_timeout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nif [ \"$1\" = config ]; then while :; do :; done; fi\nprintf 'ok\\n'\n",
        );
        let start = Instant::now();

        let output = run_git_program(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["--version"],
            Duration::from_millis(200),
        )
        .expect("main command should run after the timed-out preflight");

        assert_eq!(output.trim(), "ok");
        assert!(start.elapsed() < Duration::from_secs(2));
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
    fn is_dirty_should_ignore_untracked_files_but_report_tracked_changes() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir should be created");
        run_git(&repo, &["init", "-q"]).expect("repo should initialize");
        fs::write(repo.join("SKILL.md"), "# Skill\n").expect("skill should be written");
        run_git(&repo, &["add", "."]).expect("files should stage");
        run_git(
            &repo,
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
        )
        .expect("initial commit should succeed");

        // A stray untracked file must not count as dirty.
        fs::write(repo.join(".DS_Store"), b"junk").expect("untracked file should be written");
        assert!(!is_dirty(&repo).expect("status should run"));

        // A tracked modification must still count as dirty.
        fs::write(repo.join("SKILL.md"), "# Changed\n").expect("tracked file should change");
        assert!(is_dirty(&repo).expect("status should run"));
    }

    #[test]
    fn validate_manifest_revision_should_reject_revision_expressions() {
        for accepted in [
            "0123456789abcdef0123456789abcdef01234567",
            "v1.0.0",
            "main",
            "release/2024",
        ] {
            assert!(
                validate_manifest_revision(accepted).is_ok(),
                "expected `{accepted}` to be accepted"
            );
        }
        for rejected in [
            "", "-x", "a b", "main^", "HEAD~1", "a..b", "HEAD@{1}", "a:b", "a*",
        ] {
            assert!(
                validate_manifest_revision(rejected).is_err(),
                "expected `{rejected}` to be rejected"
            );
        }
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
