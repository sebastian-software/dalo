//! Review-first promotion of one local skill into a GitHub team repository.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::audit::{self, AgentSelection, AuditOptions, AuditReport};
use crate::error::{DaloError, DaloResult};
use crate::github::{gh, gh_repository, github_slug};
use crate::{catalog, git, inventory, source, store};

/// Command input for promoting a skill.
#[derive(Debug)]
pub struct PromoteRequest {
    /// Unique skill slot name or stable ID in the selected source.
    pub skill: String,
    /// Configured team repository source.
    pub target: String,
    /// Promote this skill from the target checkout's working tree.
    pub from_dirty: bool,
    /// Create/use the authenticated user's fork as the PR head repository.
    pub fork: bool,
    /// Preview without fetching or writing persistent state.
    pub dry_run: bool,
}

/// Preview or result for one promotion.
#[derive(Debug, Serialize)]
pub struct PromoteReport {
    /// Source-qualified skill reference.
    pub skill: String,
    /// Configured destination source ID.
    pub target: String,
    /// GitHub repository receiving the review request.
    pub repository: String,
    /// Path where the skill will be added.
    pub destination: PathBuf,
    /// Commit of the source repository when it can be read.
    pub source_commit: String,
    /// Dalo's stable fingerprint of the submitted files.
    pub content_hash: String,
    /// Deterministic audit result.
    pub audit_status: String,
    /// Number of deterministic audit findings.
    pub audit_findings: usize,
    /// Whether the PR head is pushed to the user's fork.
    pub fork: bool,
    /// Created branch; absent in a dry-run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Created pull request; absent in a dry-run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<String>,
    /// Whether this report is only a preview.
    pub dry_run: bool,
}

fn invalid(reason: impl Into<String>) -> DaloError {
    DaloError::InvalidArgument {
        reason: reason.into(),
    }
}

fn blocked(reason: impl Into<String>) -> DaloError {
    DaloError::StateError {
        reason: reason.into(),
    }
}

/// Prepare a static-audited promotion and, unless dry-run, create a branch and PR.
pub fn promote(paths: &store::StorePaths, request: &PromoteRequest) -> DaloResult<PromoteReport> {
    let config = store::read_config(paths)?;
    let target = config
        .sources
        .iter()
        .find(|candidate| candidate.id == request.target)
        .ok_or_else(|| DaloError::UnknownSource {
            source_id: request.target.clone(),
            hint: String::new(),
        })?;
    if target.kind != source::SourceKind::Team
        || target.declared_by.is_some()
        || target.subpath.is_some()
        || target
            .update_policy
            .as_deref()
            .is_some_and(|policy| policy != "track")
    {
        return Err(blocked(
            "promotion target must be a directly configured team repository tracking a branch at its checkout root",
        ));
    }
    let repository_url = target
        .url
        .as_deref()
        .ok_or_else(|| blocked("team repository has no configured Git URL"))?;
    git::validate_remote_url(repository_url)?;
    let repository = github_slug(repository_url).ok_or_else(|| {
        blocked("promote currently supports GitHub.com team repositories; other forges are tracked separately")
    })?;
    let checkout = fs::canonicalize(&target.path)?;
    let sources_root = fs::canonicalize(&paths.sources_dir)?;
    if !checkout.starts_with(&sources_root) || !checkout.is_dir() {
        return Err(blocked(
            "team checkout is outside this Dalo store's managed source directory",
        ));
    }
    let checkout_remote = git::remote_url(&checkout, "origin")?;
    if github_slug(&checkout_remote).as_deref() != Some(repository.as_str()) {
        return Err(blocked(
            "team checkout's origin does not match its configured GitHub repository URL",
        ));
    }

    let (source_ref, skill_path, slot_name, source_root, source_repo, dirty_source) = if request
        .from_dirty
    {
        if request.fork {
            return Err(invalid("--fork cannot be combined with --from-dirty"));
        }
        let (skill, path) = select_skill(&request.skill, &target.id, &checkout)?;
        let relative = safe_relative(&checkout, &path)?;
        let expected = Path::new("skills").join(&skill.slot_name);
        if relative != expected {
            return Err(blocked(format!(
                "--from-dirty only promotes a team skill at `skills/{}`",
                skill.slot_name
            )));
        }
        let changes = git::status_paths(&checkout)?;
        if changes.is_empty() {
            return Err(blocked(
                "--from-dirty was requested, but the team checkout has no local changes",
            ));
        }
        if changes
            .iter()
            .any(|changed| !changed.starts_with(&expected))
        {
            return Err(blocked(
                "the team checkout has changes outside this skill; preserve or commit them before promoting",
            ));
        }
        (
            skill.source_ref,
            path,
            skill.slot_name,
            checkout.clone(),
            git::rev_parse_head(&checkout)?,
            true,
        )
    } else {
        let local = config
            .sources
            .iter()
            .find(|candidate| candidate.kind == source::SourceKind::Local)
            .ok_or_else(|| blocked("the Dalo local skill source is not configured"))?;
        let (skill, path) = select_skill(&request.skill, &local.id, &local.path)?;
        let local_commit =
            git::rev_parse_head(&local.path).unwrap_or_else(|_| "uncommitted".into());
        let changes = git::status_paths(&checkout)?;
        if !changes.is_empty() {
            return Err(blocked(format!(
                "team checkout `{}` has local changes; resolve them before promoting",
                target.id
            )));
        }
        (
            skill.source_ref,
            path,
            skill.slot_name,
            local.path.clone(),
            local_commit,
            false,
        )
    };

    let destination = Path::new("skills").join(slot_name);
    validate_relative(&destination)?;
    validate_skill_path(&source_root, &skill_path)?;
    validate_skill_tree(&skill_path)?;
    let content_hash = catalog::hash_directory(&skill_path)?;
    let audit = audit::audit_skill(
        paths,
        &source_ref,
        &skill_path,
        &AuditOptions {
            agent: AgentSelection::None,
            refresh: false,
            persist: !request.dry_run,
            accept_risk: None,
            exclude_root_source_metadata: false,
        },
    )?;
    if audit.content_hash != content_hash {
        return Err(blocked(
            "the selected skill changed while Dalo audited it; retry the promotion",
        ));
    }
    if audit.is_blocking() {
        return Err(DaloError::AuditBlocked {
            reason: format!(
                "{source_ref} has {} unaccepted deterministic audit findings; review it with `dalo audit {source_ref}` before promoting",
                audit.static_findings.len()
            ),
        });
    }
    let audit_status = audit_status(&audit);
    let report_base = PromoteReport {
        skill: source_ref.clone(),
        target: target.id.clone(),
        repository: repository.clone(),
        destination: destination.clone(),
        source_commit: source_repo.clone(),
        content_hash: content_hash.clone(),
        audit_status,
        audit_findings: audit.static_findings.len(),
        fork: request.fork,
        branch: None,
        pull_request_url: None,
        dry_run: request.dry_run,
    };
    if request.dry_run {
        return Ok(report_base);
    }

    gh(&checkout, &["auth", "status", "--hostname", "github.com"])?;
    let temporary = tempfile::tempdir()?;
    let clone = temporary.path().join("team-repository");
    git::clone_repo(repository_url, &clone)?;
    let gh_repository = gh_repository(&clone)?;
    if gh_repository.name_with_owner.to_ascii_lowercase() != repository {
        return Err(blocked(
            "GitHub resolved the configured URL to a different repository identity",
        ));
    }
    let base = target
        .branch
        .as_deref()
        .unwrap_or(&gh_repository.default_branch);
    git::validate_manifest_revision(base)?;
    let base_commit = git::resolve_manifest_revision(&clone, base)?;
    if dirty_source && source_repo != base_commit {
        return Err(blocked(
            "the team checkout is not at the current PR base; refresh it before promoting working-tree changes",
        ));
    }
    let branch = promotion_branch(&destination)?;
    git::checkout_detached(&clone, &base_commit)?;
    git::create_branch(&clone, &branch, &base_commit)?;
    let destination_path = clone.join(&destination);
    let updating_existing = match fs::symlink_metadata(&destination_path) {
        Ok(metadata) if dirty_source && !metadata.file_type().is_symlink() && metadata.is_dir() => {
            validate_skill_path(&clone, &destination_path)?;
            validate_skill_tree(&destination_path)?;
            fs::remove_dir_all(&destination_path)?;
            true
        }
        Ok(_) if dirty_source => {
            return Err(blocked(format!(
                "existing destination `{}` is not a replaceable skill directory",
                destination.display()
            )));
        }
        Ok(_) => {
            return Err(blocked(format!(
                "destination `{}` already exists in the team repository",
                destination.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    ensure_real_directory(&clone, Path::new("skills"))?;
    copy_skill_tree(&skill_path, &destination_path)?;
    if catalog::hash_directory(&clone.join(&destination))? != content_hash {
        return Err(blocked(
            "the selected skill changed after its audit; retry the promotion",
        ));
    }
    git::add_and_commit(
        &clone,
        &destination,
        &format!(
            "feat: promote {}",
            destination.file_name().unwrap().to_string_lossy()
        ),
    )?;

    let (head_owner, push_url) = if request.fork {
        gh(
            &clone,
            &[
                "repo",
                "fork",
                &repository,
                "--clone=false",
                "--remote=false",
            ],
        )?;
        let owner = gh(&clone, &["api", "user", "--jq", ".login"])?
            .trim()
            .to_owned();
        if owner.is_empty()
            || !owner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(blocked(
                "GitHub returned an invalid authenticated account name",
            ));
        }
        let repo_name = repository.split_once('/').expect("validated GitHub slug").1;
        (
            owner.clone(),
            format!("https://github.com/{owner}/{repo_name}.git"),
        )
    } else {
        let url = format!("https://github.com/{repository}.git");
        (
            repository
                .split_once('/')
                .expect("validated GitHub slug")
                .0
                .to_owned(),
            url,
        )
    };
    if let Err(error) = git::push_branch_with_gh_auth(&clone, &push_url, &branch) {
        let advice = if request.fork {
            "check that your GitHub account can push to its fork and retry"
        } else {
            "if you do not have push access, retry with --fork"
        };
        return Err(blocked(format!(
            "could not push promotion branch `{branch}` ({advice}): {error}"
        )));
    }

    let body = pull_request_body(
        &source_ref,
        &source_repo,
        dirty_source,
        &content_hash,
        &audit,
    );
    let body_file = tempfile::NamedTempFile::new_in(temporary.path())?;
    fs::write(body_file.path(), body)?;
    let head = format!("{head_owner}:{branch}");
    let action = if updating_existing { "update" } else { "add" };
    let title = format!(
        "feat: {action} {} skill",
        destination.file_name().unwrap().to_string_lossy()
    );
    let result = gh(
        &clone,
        &[
            "pr",
            "create",
            "--repo",
            &repository,
            "--base",
            base,
            "--head",
            &head,
            "--title",
            &title,
            "--body-file",
            body_file
                .path()
                .to_str()
                .ok_or_else(|| invalid("temporary PR body path is not UTF-8"))?,
        ],
    );
    let pull_request_url = result.map_err(|error| {
        blocked(format!(
            "branch `{branch}` was pushed, but GitHub could not create the pull request; the branch is preserved for retry: {error}"
        ))
    })?.trim().to_owned();
    if !pull_request_url.starts_with("https://github.com/") {
        return Err(blocked("GitHub returned an unexpected pull request URL"));
    }
    Ok(PromoteReport {
        branch: Some(branch),
        pull_request_url: Some(pull_request_url),
        dry_run: false,
        ..report_base
    })
}

fn select_skill(
    selector: &str,
    source_id: &str,
    root: &Path,
) -> DaloResult<(inventory::SkillRecord, PathBuf)> {
    let inventory = inventory::scan_source(source_id, root)?;
    let matches = inventory
        .skills
        .into_iter()
        .filter(|skill| skill.slot_name == selector || skill.id.as_deref() == Some(selector))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [skill] => Ok((skill.clone(), skill.path.clone())),
        [] => Err(blocked(format!(
            "no skill named or identified by `{selector}` exists in source `{source_id}`"
        ))),
        _ => Err(blocked(format!(
            "`{selector}` matches multiple skills in source `{source_id}`; use a unique stable skill ID"
        ))),
    }
}

fn validate_skill_path(root: &Path, path: &Path) -> DaloResult<()> {
    let relative = safe_relative(root, path)?;
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(blocked("selected skill source must be a real directory"));
    }
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("promotion path escapes the selected source"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(blocked(
                "selected skill and its parent directories must be real directories",
            ));
        }
    }
    Ok(())
}

fn safe_relative(root: &Path, path: &Path) -> DaloResult<PathBuf> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| blocked("selected skill is outside its configured source"))?;
    validate_relative(relative)?;
    Ok(relative.to_path_buf())
}

fn validate_relative(path: &Path) -> DaloResult<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid(
            "promotion paths must remain within the selected repository",
        ));
    }
    Ok(())
}

fn ensure_real_directory(root: &Path, relative: &Path) -> DaloResult<()> {
    let mut current = root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            return Err(invalid("promotion path escapes the Git checkout"));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(blocked(format!(
                    "promotion directory `{}` is redirected or not a directory",
                    relative.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn validate_skill_tree(path: &Path) -> DaloResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            return Err(blocked(
                "skill contains a nested `.git` entry; review it before promotion",
            ));
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(blocked(
                "skill contains a symlink; promotion preserves ordinary files only",
            ));
        }
        if metadata.is_dir() {
            validate_skill_tree(&entry.path())?;
        } else if !metadata.is_file() {
            return Err(blocked(
                "skill contains a special file that cannot be safely promoted",
            ));
        }
    }
    Ok(())
}

fn copy_skill_tree(source: &Path, destination: &Path) -> DaloResult<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            return Err(blocked(
                "skill contains a nested `.git` entry; review it before promotion",
            ));
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(blocked(
                "skill contains a symlink; promotion preserves ordinary files only",
            ));
        } else if metadata.is_dir() {
            copy_skill_tree(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, metadata.permissions())?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(blocked(
                "skill contains a special file that cannot be safely promoted",
            ));
        }
    }
    Ok(())
}

fn promotion_branch(destination: &Path) -> DaloResult<String> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("skill name is not UTF-8"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| DaloError::Io(std::io::Error::other(error)))?
        .as_nanos();
    Ok(format!("dalo/promote-{name}-{timestamp}"))
}

fn audit_status(report: &AuditReport) -> String {
    serde_json::to_value(report)
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".into())
}

fn pull_request_body(
    source_ref: &str,
    commit: &str,
    dirty: bool,
    hash: &str,
    report: &AuditReport,
) -> String {
    let provenance = if dirty {
        format!("team working tree based on `{commit}`")
    } else if commit == "uncommitted" {
        "local source working tree (uncommitted)".to_owned()
    } else {
        format!("local source at `{commit}`")
    };
    let accepted_risk = report
        .risk_acceptance
        .as_ref()
        .map(|_| "\n- Explicit audit risk acceptance: recorded for this snapshot".to_owned())
        .unwrap_or_default();
    format!(
        "Promoted with Dalo.\n\n- Skill: `{source_ref}`\n- Source: {provenance}\n- Dalo content fingerprint: `{hash}`\n- Deterministic audit: **{}** ({} findings){}\n\nPlease review the skill content and audit findings before merging. Dalo did not push to the default branch.",
        audit_status(report),
        report.static_findings.len(),
        accepted_risk
    )
}
