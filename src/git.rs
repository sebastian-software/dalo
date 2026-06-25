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
    run_git(cwd, &["clone", "--quiet", url, &destination_arg]).map(|_| ())
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
