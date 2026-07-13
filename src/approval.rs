//! Explicit, source-qualified approval lifecycle operations.

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::inventory;
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
    let value = canonical_value(paths, scope, value)?;
    let mut approvals = store::read_approvals(paths)?;
    let before = approvals.approvals.len();
    approvals
        .approvals
        .retain(|record| !(record.scope == scope && record.value == value));
    let changed = approvals.approvals.len() != before;
    if changed && !dry_run {
        store::write_approvals(paths, &approvals)?;
    }
    Ok(ApprovalReport {
        scope: scope.to_owned(),
        value,
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
        "author" | "org" => {
            let (source, owner) = source_qualified(value)?;
            source_exists(paths, source)?;
            if owner.trim().is_empty() {
                return invalid("approval owner must not be empty");
            }
            Ok(format!("{source}:{owner}"))
        }
        _ => invalid("approval scope must be one of skill, source, author, or org"),
    }
}

fn canonical_skill(paths: &StorePaths, value: &str) -> DaloResult<String> {
    let (source_id, selector) = source_qualified(value)?;
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
            DaloError::skill_not_found(
                value,
                inventory
                    .skills
                    .iter()
                    .map(|candidate| candidate.source_ref.clone())
                    .collect(),
                format!("dalo source inspect {source_id}"),
            )
        })?;
    Ok(skill.source_ref.clone())
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

fn source_qualified(value: &str) -> DaloResult<(&str, &str)> {
    value
        .split_once(':')
        .filter(|(source, value)| !source.is_empty() && !value.is_empty())
        .ok_or_else(|| DaloError::CheckFailed {
            reason: "approval values must be source-qualified, for example `catalog:skill`"
                .to_owned(),
        })
}

fn invalid<T>(reason: &str) -> DaloResult<T> {
    Err(DaloError::CheckFailed {
        reason: reason.to_owned(),
    })
}
