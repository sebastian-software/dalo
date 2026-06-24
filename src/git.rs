//! Narrow wrapper around the system `git` command.

use std::path::Path;
use std::process::Command;

use crate::error::{SkillmgrError, SkillmgrResult};

/// Run `git init` in the provided directory.
pub fn init_repo(path: &Path) -> SkillmgrResult<()> {
    let output = Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    Err(SkillmgrError::CommandFailed {
        program: "git".to_owned(),
        args: "init -q".to_owned(),
        cwd: path.to_path_buf(),
        status: output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}
