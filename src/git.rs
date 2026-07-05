//! Narrow wrapper around the system `git` command.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};

const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const GIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Run `git init` in the provided directory.
pub fn init_repo(path: &Path) -> DaloResult<()> {
    run_git(path, &["init", "-q"]).map(|_| ())
}

/// Clone a Git repository.
pub fn clone_repo(url: &str, destination: &Path) -> DaloResult<()> {
    let cwd = destination.parent().unwrap_or_else(|| Path::new("."));
    let destination_arg = destination.to_string_lossy().into_owned();
    // `--` terminates option parsing so a user-supplied URL that looks like a
    // flag (e.g. `--upload-pack=...`) can never be treated as a git option.
    run_git(cwd, &["clone", "--quiet", "--", url, &destination_arg]).map(|_| ())
}

/// Update the current tracking branch through a fast-forward-only pull.
pub fn pull_ff_only(path: &Path) -> DaloResult<()> {
    run_git(path, &["pull", "--ff-only", "--quiet"]).map(|_| ())
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
    run_git(path, &["fetch", "--quiet", "origin", "HEAD"]).map(|_| ())
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
    run_git_program("git", path, args, GIT_TIMEOUT)
}

fn run_git_program(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
) -> DaloResult<String> {
    let stdout = NamedTempFile::new()?;
    let stderr = NamedTempFile::new()?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?))
        .spawn()?;

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }

        let elapsed = start.elapsed();
        if elapsed >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DaloError::CommandFailed {
                program: program.to_owned(),
                args: args.join(" "),
                cwd: path.to_path_buf(),
                status: format!("timed out after {}", format_duration(timeout)),
                stderr: timeout_stderr(&stderr),
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
        args: args.join(" "),
        cwd: path.to_path_buf(),
        status: status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        stderr: stderr_text,
    })
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

        let error = run_git_program(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["pull"],
            Duration::from_secs(1),
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("prompt=0"));
        assert!(stderr.contains("BatchMode=yes"));
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
