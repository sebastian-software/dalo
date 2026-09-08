//! Explicit, source-qualified approval lifecycle operations.

use serde::Serialize;

use crate::agent;
use crate::audit;
use crate::error::{DaloError, DaloResult};
use crate::inventory;
use crate::source::SourceKind;
use crate::store::{self, ApprovalRecord, StorePaths};

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

/// One persisted risk acceptance surfaced by `approve list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AcceptedRiskSummary {
    /// Source-qualified audited skill reference.
    pub source_ref: String,
    /// Exact audited content hash.
    pub content_hash: String,
    /// User-provided reason for accepting the risk.
    pub reason: String,
    /// Unix timestamp of acceptance.
    pub accepted_at_unix: u64,
    /// Hash binding the acceptance to the exact audit inputs and findings.
    pub scope_hash: String,
    /// Copyable command for inspecting the persisted audit again.
    pub audit_command: String,
}

/// Read model for the approval inspection command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalListReport {
    /// Persisted approval schema version.
    pub schema_version: u32,
    /// Explicit approval records.
    pub approvals: Vec<ApprovalRecord>,
    /// Persisted accepted-risk audits, including those not represented by an
    /// explicit per-skill approval record.
    pub accepted_risks: Vec<AcceptedRiskSummary>,
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

/// Read approvals and persisted risk acceptances for `approve list`.
pub fn list(paths: &StorePaths) -> DaloResult<ApprovalListReport> {
    let approvals = store::read_approvals(paths)?;
    let accepted_risks = audit::read_persisted_reports(paths)?
        .into_iter()
        .filter_map(|report| {
            report
                .risk_acceptance
                .map(|acceptance| AcceptedRiskSummary {
                    source_ref: report.source_ref.clone(),
                    content_hash: report.content_hash,
                    reason: acceptance.reason,
                    accepted_at_unix: acceptance.accepted_at_unix,
                    scope_hash: acceptance.scope_hash,
                    audit_command: audit_command(paths, &report.source_ref, &report.skill_path),
                })
        })
        .collect();

    Ok(ApprovalListReport {
        schema_version: approvals.schema_version,
        approvals: approvals.approvals,
        accepted_risks,
    })
}

fn audit_command(paths: &StorePaths, source_ref: &str, skill_path: &std::path::Path) -> String {
    // Path audits use a synthetic source ref that is an identity, not a valid
    // CLI selector. Replay them from the persisted directory instead, and
    // quote every target as one shell word because it may contain metacharacters.
    let target = if source_ref.starts_with("path:") {
        skill_path.to_string_lossy().into_owned()
    } else {
        source_ref.to_owned()
    };
    store::dalo_command(
        &paths.root,
        &format!(
            "audit {}",
            crate::error::shell_quote_path(std::path::Path::new(&target))
        ),
    )
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
    fn list_should_surface_persisted_risk_acceptances_with_exact_bindings() {
        let (_temp, paths) = init_paths();
        let path_target = std::path::Path::new("/tmp/danger's $(touch pwned)");
        std::fs::write(
            paths.audits_dir.join("danger.json"),
            serde_json::json!({
                "schema_version": 1,
                "source_ref": "catalog:danger-tool",
                "skill_path": "/tmp/danger-tool",
                "content_hash": "deadbeef",
                "static_engine_version": "5",
                "static_scan_excludes_root_source_metadata": false,
                "scanned_at_unix": 100,
                "coverage": "complete",
                "status": "blocked",
                "max_severity": "high",
                "static_findings": [{
                    "id": "shell",
                    "severity": "high",
                    "category": "execution",
                    "path": "SKILL.md",
                    "message": "runs a shell command"
                }],
                "risk_acceptance": {
                    "reason": "reviewed exception",
                    "accepted_at_unix": 200,
                    "scope_hash": "cafebabe"
                }
            })
            .to_string(),
        )
        .expect("audit report should be written");
        std::fs::write(
            paths.audits_dir.join("path.json"),
            serde_json::json!({
                "schema_version": 1,
                "source_ref": "path:danger@cafebabe",
                "skill_path": path_target,
                "content_hash": "feedface",
                "static_engine_version": "5",
                "static_scan_excludes_root_source_metadata": false,
                "scanned_at_unix": 101,
                "coverage": "complete",
                "status": "blocked",
                "max_severity": "high",
                "static_findings": [],
                "risk_acceptance": {
                    "reason": "reviewed path exception",
                    "accepted_at_unix": 201,
                    "scope_hash": "badcafe"
                }
            })
            .to_string(),
        )
        .expect("path audit report should be written");

        let report = list(&paths).expect("approval list should include audits");
        assert!(report.approvals.is_empty());
        assert_eq!(report.accepted_risks.len(), 2);
        let acceptance = &report.accepted_risks[0];
        assert_eq!(acceptance.source_ref, "catalog:danger-tool");
        assert_eq!(acceptance.content_hash, "deadbeef");
        assert_eq!(acceptance.reason, "reviewed exception");
        assert_eq!(acceptance.accepted_at_unix, 200);
        assert_eq!(acceptance.scope_hash, "cafebabe");
        assert_eq!(
            acceptance.audit_command,
            store::dalo_command(&paths.root, "audit 'catalog:danger-tool'")
        );
        let path_acceptance = &report.accepted_risks[1];
        assert_eq!(path_acceptance.source_ref, "path:danger@cafebabe");
        assert_eq!(
            path_acceptance.audit_command,
            store::dalo_command(
                &paths.root,
                &format!("audit {}", crate::error::shell_quote_path(path_target)),
            )
        );
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
