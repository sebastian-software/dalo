//! Catalog pin proposals created in isolated GitHub team checkouts.

use crate::error::{DaloError, DaloResult};
use crate::github::{gh, gh_repository, github_slug};
use crate::team_manifest::TeamCatalogUpdateReport;
use crate::{git, status, team_manifest};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

fn blocked(reason: impl Into<String>) -> DaloError {
    DaloError::StateError {
        reason: reason.into(),
    }
}

pub(crate) fn update(
    repo: &Path,
    id: &str,
    from_ref: &str,
    dry_run: bool,
    accept_risk: Option<&str>,
) -> DaloResult<TeamCatalogUpdateReport> {
    let repo = fs::canonicalize(repo)?;
    if !git::is_worktree(&repo)? {
        return Err(blocked(
            "--pr requires a team Git checkout with a GitHub.com origin",
        ));
    }
    let root = git::worktree_root(&repo)?;
    if root != repo {
        return Err(blocked(
            "--pr requires --repo to point to the team Git repository root",
        ));
    }
    if !git::status_paths(&repo)?.is_empty() {
        return Err(blocked(
            "--pr requires a clean team checkout, including untracked files; commit or remove local changes first",
        ));
    }
    let url = git::remote_url(&repo, "origin")?;
    git::validate_remote_url(&url)?;
    let repository = github_slug(&url)
        .ok_or_else(|| blocked("catalog update --pr currently supports GitHub.com origins only"))?;
    let original = fs::read(repo.join("dalo.toml"))?;
    let head = git::rev_parse(&repo, "HEAD")?;
    let mut report =
        team_manifest::update_team_catalog_pin(&repo, id, from_ref, true, accept_risk)?;
    if dry_run
        || !report.blocking_reasons.is_empty()
        || report.old_version == report.candidate_commit
    {
        report.dry_run = dry_run;
        return Ok(report);
    }
    let summary = status::team_catalog_update_summary(&report);
    gh(&repo, &["auth", "status", "--hostname", "github.com"])?;
    let metadata = gh_repository(&repo)?;
    if metadata.name_with_owner.to_ascii_lowercase() != repository {
        return Err(blocked(
            "GitHub repository identity does not match the team origin",
        ));
    }
    git::validate_manifest_revision(&metadata.default_branch)?;
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("team");
    git::clone_repo(&url, &checkout)?;
    let base = git::resolve_manifest_revision(&checkout, &metadata.default_branch)?;
    if head != base
        || fs::read(repo.join("dalo.toml"))? != original
        || !git::status_paths(&repo)?.is_empty()
        || git::rev_parse(&repo, "HEAD")? != head
    {
        return Err(blocked(
            "team checkout must match the current GitHub default branch; update it and preview again",
        ));
    }
    git::checkout_detached(&checkout, &base)?;
    if fs::read(checkout.join("dalo.toml"))? != original {
        return Err(blocked(
            "team manifest changed since the preview; update the checkout and retry",
        ));
    }
    // The branch identifies this exact base, candidate and accepted-risk review.
    // Never reuse a branch with different contents or force-push over a proposal.
    let digest = Sha256::digest(format!("{base}\n{summary}"));
    let key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let branch = format!("dalo/catalog-update/{}", &key[..32]);
    git::create_branch(&checkout, &branch, &base)?;
    write_proposal_pin(&checkout, id, &report.candidate_commit, &original)?;
    git::add_and_commit(
        &checkout,
        Path::new("dalo.toml"),
        &format!("chore: update {id} catalog pin"),
    )?;
    let proposal_commit = if let Some(existing) = git::remote_branch_commit(&checkout, &branch)? {
        if git::rev_parse(&checkout, &format!("{existing}^{{tree}}"))?
            != git::rev_parse(&checkout, "HEAD^{tree}")?
            || git::rev_parse(&checkout, &format!("{existing}^"))? != base
        {
            return Err(blocked(format!(
                "remote branch `{branch}` contains different changes; review it before retrying"
            )));
        }
        existing
    } else {
        git::push_branch_with_gh_auth(&checkout, &url, &branch)
            .map_err(|error| blocked(format!("could not push catalog proposal `{branch}`: {error}; check repository write access and retry the same command")))?;
        git::rev_parse(&checkout, "HEAD")?
    };
    let existing = gh(
        &checkout,
        &[
            "pr",
            "list",
            "--repo",
            &repository,
            "--head",
            &branch,
            "--base",
            &metadata.default_branch,
            "--state",
            "all",
            "--json",
            "url,state,headRefOid,headRepositoryOwner",
        ],
    ).map_err(|error| blocked(format!("catalog proposal branch `{branch}` is available, but PR lookup failed: {error}; retry the same command")))?;
    let prs: Vec<serde_json::Value> = serde_json::from_str(&existing)?;
    let pr_url = if let Some(pr) = prs.first() {
        let owner = repository.split('/').next().unwrap_or_default();
        if pr.get("headRefOid").and_then(serde_json::Value::as_str) != Some(&proposal_commit)
            || pr
                .pointer("/headRepositoryOwner/login")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|login| !login.eq_ignore_ascii_case(owner))
        {
            return Err(blocked(format!(
                "pull request for `{branch}` does not match the verified proposal"
            )));
        }
        if pr.get("state").and_then(serde_json::Value::as_str) != Some("OPEN") {
            return Err(blocked(format!(
                "pull request for `{branch}` is already closed; review or reopen it on GitHub"
            )));
        }
        pr.get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| blocked("GitHub did not return a pull request URL"))?
            .to_owned()
    } else {
        let body = temp.path().join("review.txt");
        fs::write(&body, &summary)?;
        let body = body
            .to_str()
            .ok_or_else(|| blocked("PR body path is not UTF-8"))?;
        gh(&checkout, &["pr", "create", "--repo", &repository,
            "--base", &metadata.default_branch, "--head", &branch,
            "--title", &format!("chore: update {id} catalog pin"), "--body-file", body])
            .map_err(|error| blocked(format!("catalog proposal branch `{branch}` was pushed, but PR creation failed: {error}; retry the same command to reuse this branch")))?
            .trim().to_owned()
    };
    report.dry_run = false;
    report.branch = Some(branch);
    report.pull_request_url = Some(pr_url);
    Ok(report)
}

// Unlike the manual canonical manifest editor, proposals preserve comments and
// formatting so the pull request changes only the reviewed version value.
fn write_proposal_pin(repo: &Path, id: &str, candidate: &str, original: &[u8]) -> DaloResult<()> {
    let text = std::str::from_utf8(original).map_err(|error| blocked(error.to_string()))?;
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| blocked(error.to_string()))?;
    let catalogs = document
        .get_mut("catalog")
        .ok_or_else(|| blocked("team manifest has no editable catalog declarations"))?;
    let version = match catalogs {
        toml_edit::Item::ArrayOfTables(tables) => tables
            .iter_mut()
            .find(|table| table.get("id").and_then(toml_edit::Item::as_str) == Some(id))
            .and_then(|table| table.get_mut("version"))
            .and_then(toml_edit::Item::as_value_mut),
        toml_edit::Item::Value(toml_edit::Value::Array(tables)) => tables
            .iter_mut()
            .filter_map(toml_edit::Value::as_inline_table_mut)
            .find(|table| table.get("id").and_then(toml_edit::Value::as_str) == Some(id))
            .and_then(|table| table.get_mut("version")),
        _ => None,
    }
    .ok_or_else(|| blocked("reviewed catalog version is missing or cannot be edited"))?;
    let decor = version.decor().clone();
    *version = toml_edit::Value::from(candidate);
    *version.decor_mut() = decor;
    fs::write(repo.join("dalo.toml"), document.to_string())?;
    Ok(())
}
