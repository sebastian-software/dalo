//! Explicit, source-qualified approval lifecycle operations.

use serde::Serialize;

use crate::agent;
use crate::error::{DaloError, DaloResult};
use crate::inventory;
use crate::source::SourceKind;
use crate::store::{self, ApprovalRecord, ApprovalsFile, StorePaths};

/// Result of granting or revoking an approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalReport {
    /// Approval scope.
    pub scope: String,
    /// Source-qualified approved value.
    pub value: String,
    /// `granted`, `revoked`, or `unchanged`.
    pub action: String,
    /// Whether no file was changed.
    pub dry_run: bool,
}

/// Grant one approval after validating its scope and source qualification.
pub fn grant(
    paths: &StorePaths,
    scope: &str,
    value: &str,
    dry_run: bool,
) -> DaloResult<ApprovalReport> {
    let value = canonical_value(paths, scope, value)?;
    let mut approvals = store::read_approvals(paths)?;
    let record = ApprovalRecord {
        scope: scope.to_owned(),
        value: value.clone(),
    };
    let exists = approvals.approvals.contains(&record);
    if !exists {
        approvals.approvals.push(record);
        approvals.approvals.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then(left.value.cmp(&right.value))
        });
        if !dry_run {
            store::write_approvals(paths, &approvals)?;
        }
    }
    Ok(ApprovalReport {
        scope: scope.to_owned(),
        value,
        action: if exists { "unchanged" } else { "granted" }.to_owned(),
        dry_run,
    })
}

/// Revoke one exact, source-qualified approval.
pub fn revoke(
    paths: &StorePaths,
    scope: &str,
    value: &str,
    dry_run: bool,
) -> DaloResult<ApprovalReport> {
    // Resolve canonically when possible so a live reference matches its stored
    // canonical form. Tolerate a source or skill that no longer resolves so a
    // stale trust record can always be withdrawn; scope and value-shape errors
    // (InvalidArgument) are still surfaced, except for old hand-written agent
    // records that need an exact raw-value escape hatch.
    let canonical = match canonical_value(paths, scope, value) {
        Ok(canonical) => Some(canonical),
        // Agent approvals predate the CLI and may have been hand-written. Keep
        // revocation available for those records even if their agent package
        // has since disappeared or the stored value is not a current ref.
        Err(DaloError::UnknownSource { .. } | DaloError::InvalidArgument { .. })
            if scope == "agent" =>
        {
            None
        }
        Err(DaloError::UnknownSource { .. } | DaloError::SkillNotFound { .. }) => None,
        Err(error @ DaloError::InvalidArgument { .. }) => return Err(error),
        Err(error) => return Err(error),
    };
    let mut approvals = store::read_approvals(paths)?;
    let before = approvals.approvals.len();
    approvals.approvals.retain(|record| {
        let matches = record.scope == scope
            && (record.value == value || canonical.as_deref() == Some(record.value.as_str()));
        !matches
    });
    let changed = approvals.approvals.len() != before;
    if changed && !dry_run {
        store::write_approvals(paths, &approvals)?;
    }
    Ok(ApprovalReport {
        scope: scope.to_owned(),
        value: canonical.unwrap_or_else(|| value.to_owned()),
        action: if changed { "revoked" } else { "unchanged" }.to_owned(),
        dry_run,
    })
}

/// Read approvals for `approve list`.
pub fn list(paths: &StorePaths) -> DaloResult<ApprovalsFile> {
    store::read_approvals(paths)
}

fn canonical_value(paths: &StorePaths, scope: &str, value: &str) -> DaloResult<String> {
    match scope {
        "source" => {
            source_exists(paths, value)?;
            Ok(value.to_owned())
        }
        "skill" => canonical_skill(paths, value),
        "agent" => canonical_agent(paths, value),
        "author" | "org" => {
            let (source, owner) = source_qualified_owner(scope, value)?;
            source_exists(paths, source)?;
            if owner.trim().is_empty() {
                return invalid("approval owner must not be empty");
            }
            Ok(format!("{source}:{owner}"))
        }
        _ => invalid("approval scope must be one of skill, source, agent, author, or org"),
    }
}

/// Resolve a source-qualified slot or stable ID to its canonical skill ref.
pub fn canonical_skill(paths: &StorePaths, value: &str) -> DaloResult<String> {
    let (source_id, selector) = value
        .split_once(':')
        .filter(|(source, skill)| !source.is_empty() && !skill.is_empty())
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: "skill approval values must use `<source>:<slot>`, for example `catalog:review-helper`"
                .to_owned(),
        })?;
    let config = store::read_config(paths)?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            DaloError::unknown_source(
                source_id,
                config
                    .sources
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect(),
            )
        })?;
    let inventory = inventory::scan_source(source_id, &source.path)?;
    let skill = inventory
        .skills
        .iter()
        .find(|skill| skill.slot_name == selector || skill.id.as_deref() == Some(selector))
        .ok_or_else(|| {
            // `source inspect` is catalog-only; point team/local sources at a
            // command that actually lists their skills (#402).
            let next_command = match source.kind {
                SourceKind::Catalog => format!("dalo source inspect {source_id}"),
                _ => "dalo status".to_owned(),
            };
            DaloError::skill_not_found(
                value,
                inventory
                    .skills
                    .iter()
                    .map(|candidate| candidate.source_ref.clone())
                    .collect(),
                next_command,
            )
        })?;
    Ok(skill.source_ref.clone())
}

/// Resolve a source-qualified slot or stable ID to its canonical agent ref.
fn canonical_agent(paths: &StorePaths, value: &str) -> DaloResult<String> {
    let (source_id, _) = value
        .split_once(':')
        .filter(|(source, agent)| !source.is_empty() && !agent.is_empty())
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: "agent approval values must use `<source>:<name>`, for example `team:reviewer`"
                .to_owned(),
        })?;
    let config = store::read_config(paths)?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            DaloError::unknown_source(
                source_id,
                config
                    .sources
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect(),
            )
        })?;
    let inventory = inventory::scan_source(source_id, &source.path)?;
    Ok(agent::find_agent(&config.sources, &[inventory], value)?.source_ref)
}

fn source_exists(paths: &StorePaths, source_id: &str) -> DaloResult<()> {
    if store::read_config(paths)?
        .sources
        .iter()
        .any(|source| source.id == source_id)
    {
        Ok(())
    } else {
        Err(DaloError::unknown_source(
            source_id,
            store::read_config(paths)?
                .sources
                .into_iter()
                .map(|source| source.id)
                .collect(),
        ))
    }
}

fn source_qualified_owner<'a>(scope: &str, value: &'a str) -> DaloResult<(&'a str, &'a str)> {
    value
        .split_once(':')
        .filter(|(source, value)| !source.is_empty() && !value.is_empty())
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: format!(
                "{scope} approval values must use `<source>:<owner>`, for example `catalog:{}`",
                if scope == "author" {
                    "maintainers"
                } else {
                    "example-org"
                }
            ),
        })
}

fn invalid<T>(reason: &str) -> DaloResult<T> {
    Err(DaloError::InvalidArgument {
        reason: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_paths() -> (tempfile::TempDir, StorePaths) {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let root = temp.path().join("store");
        store::init_store(root.clone(), false).expect("store should initialize");
        (temp, StorePaths::new(root))
    }

    #[test]
    fn revoke_should_remove_approval_when_source_no_longer_resolves() {
        let (_temp, paths) = init_paths();
        let mut approvals = store::read_approvals(&paths).expect("approvals should read");
        approvals.approvals.push(ApprovalRecord {
            scope: "source".to_owned(),
            value: "ghost".to_owned(),
        });
        store::write_approvals(&paths, &approvals).expect("approvals should write");

        let report = revoke(&paths, "source", "ghost", false)
            .expect("revoke should tolerate a source that no longer exists");
        assert_eq!(report.action, "revoked");
        assert!(
            store::read_approvals(&paths)
                .expect("approvals should read")
                .approvals
                .is_empty()
        );
    }

    #[test]
    fn revoke_should_remove_skill_approval_by_stored_value_when_source_is_gone() {
        let (_temp, paths) = init_paths();
        // `grant` stores a skill approval as its source_ref, which is always
        // `<source>:<slot>` (see inventory), i.e. the same string the user sees
        // in `approve list`. Revoking by that value must succeed even once the
        // `catalog` source no longer exists (canonical resolution fails).
        let mut approvals = store::read_approvals(&paths).expect("approvals should read");
        approvals.approvals.push(ApprovalRecord {
            scope: "skill".to_owned(),
            value: "catalog:review-helper".to_owned(),
        });
        store::write_approvals(&paths, &approvals).expect("approvals should write");

        let report = revoke(&paths, "skill", "catalog:review-helper", false)
            .expect("revoke should tolerate a skill source that no longer exists");
        assert_eq!(report.action, "revoked");
        assert!(
            store::read_approvals(&paths)
                .expect("approvals should read")
                .approvals
                .is_empty()
        );
    }

    #[test]
    fn revoke_should_remove_hand_written_agent_approval_when_it_no_longer_resolves() {
        let (_temp, paths) = init_paths();
        let mut approvals = store::read_approvals(&paths).expect("approvals should read");
        approvals.approvals.push(ApprovalRecord {
            scope: "agent".to_owned(),
            value: "retired-reviewer".to_owned(),
        });
        store::write_approvals(&paths, &approvals).expect("approvals should write");

        let report = revoke(&paths, "agent", "retired-reviewer", false)
            .expect("revoke should tolerate a hand-written agent approval");
        assert_eq!(report.action, "revoked");
        assert!(
            store::read_approvals(&paths)
                .expect("approvals should read")
                .approvals
                .is_empty()
        );
    }

    #[test]
    fn revoke_should_fail_closed_for_an_agent_when_the_config_cannot_be_read() {
        let (_temp, paths) = init_paths();
        let mut approvals = store::read_approvals(&paths).expect("approvals should read");
        approvals.approvals.push(ApprovalRecord {
            scope: "agent".to_owned(),
            value: "team:reviewer".to_owned(),
        });
        store::write_approvals(&paths, &approvals).expect("approvals should write");
        std::fs::write(&paths.config_file, "schema_version = ")
            .expect("config should be corrupted");

        assert!(revoke(&paths, "agent", "team:reviewer", false).is_err());
        assert_eq!(
            store::read_approvals(&paths)
                .expect("approvals should read")
                .approvals
                .len(),
            1
        );
    }

    #[test]
    fn revoke_should_still_reject_an_invalid_scope() {
        let (_temp, paths) = init_paths();
        assert!(matches!(
            revoke(&paths, "bogus", "x", false),
            Err(DaloError::InvalidArgument { .. })
        ));
    }
}
