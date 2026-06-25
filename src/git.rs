//! Narrow wrapper around the system `git` command.

use std::path::Path;
use std::process::Command;

use crate::error::{DaloError, DaloResult};

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

fn run_git(path: &Path, args: &[&str]) -> DaloResult<String> {
    let output = Command::new("git").args(args).current_dir(path).output()?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    Err(DaloError::CommandFailed {
        program: "git".to_owned(),
        args: args.join(" "),
        cwd: path.to_path_buf(),
        status: output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}
