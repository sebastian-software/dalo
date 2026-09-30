//! Shared GitHub CLI adapter for review workflows.

use crate::error::{DaloError, DaloResult};
use std::path::Path;
use std::process::Command;

pub(crate) struct GhRepository {
    pub(crate) name_with_owner: String,
    pub(crate) default_branch: String,
}

fn blocked(reason: impl Into<String>) -> DaloError {
    DaloError::StateError {
        reason: reason.into(),
    }
}

pub(crate) fn github_slug(value: &str) -> Option<String> {
    let path = value
        .strip_prefix("https://github.com/")
        .or_else(|| value.strip_prefix("ssh://git@github.com/"))
        .or_else(|| value.strip_prefix("git@github.com:"))?;
    let path = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_end_matches('/'));
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
    {
        return None;
    }
    Some(format!(
        "{}/{}",
        parts[0].to_ascii_lowercase(),
        parts[1].to_ascii_lowercase()
    ))
}

pub(crate) fn gh_repository(path: &Path) -> DaloResult<GhRepository> {
    let output = gh(
        path,
        &["repo", "view", "--json", "nameWithOwner,defaultBranchRef"],
    )?;
    let value: serde_json::Value = serde_json::from_str(&output)?;
    let name_with_owner = value
        .get("nameWithOwner")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| blocked("GitHub did not return the team repository identity"))?
        .to_owned();
    let default_branch = value
        .get("defaultBranchRef")
        .and_then(|value| value.get("name"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| blocked("GitHub did not return a default branch for the team repository"))?
        .to_owned();
    Ok(GhRepository {
        name_with_owner,
        default_branch,
    })
}

pub(crate) fn gh(path: &Path, args: &[&str]) -> DaloResult<String> {
    let output = Command::new("gh").args(args).current_dir(path).output().map_err(|error| {
        blocked(format!("could not start GitHub CLI `gh`: {error}; install GitHub CLI and authenticate with `gh auth login`"))
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(blocked(format!(
            "GitHub CLI command failed ({}): {}",
            args.first().copied().unwrap_or("gh"),
            crate::term::terminal_safe_text(&detail)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
