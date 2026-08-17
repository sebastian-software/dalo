//! Inert generated-delivery recipe approval and provenance checks.
//!
//! This module validates and approves recipe metadata only. It never invokes a
//! generator or creates derived output.

use std::path::PathBuf;

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::inventory::SkillDelivery;
use crate::source::SourceKind;
use crate::store::{self, ApprovalRecord, StorePaths};

/// Stable approval scope for exact generated-delivery recipes.
pub const APPROVAL_SCOPE: &str = "delivery";

/// Result of granting or revoking one generated recipe approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeliveryApprovalReport {
    /// Canonical source-qualified logical skill.
    pub skill: String,
    /// Exact revision- and recipe-bound approval value.
    pub approval_value: String,
    /// Same-source generator tool identity.
    pub generator: String,
    /// Exact generator invocation-contract hash.
    pub generator_contract_hash: String,
    /// Expected outputs keyed by logical target ID.
    pub providers: std::collections::BTreeMap<String, PathBuf>,
    /// `granted`, `revoked`, or `unchanged`.
    pub action: String,
    /// Whether no approval file was changed.
    pub dry_run: bool,
    /// This phase never executes generator code.
    pub execution: String,
}

/// Grant approval for one exact generated recipe without executing its tool.
pub fn approve(
    paths: &StorePaths,
    value: &str,
    dry_run: bool,
) -> DaloResult<DeliveryApprovalReport> {
    let mut report = inspect(paths, value)?;
    let mut approvals = store::read_approvals(paths)?;
    let record = ApprovalRecord {
        scope: APPROVAL_SCOPE.to_owned(),
        value: report.approval_value.clone(),
    };
    let exists = approvals.approvals.contains(&record);
    if !exists && !dry_run {
        approvals.approvals.push(record);
        approvals.approvals.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then(left.value.cmp(&right.value))
        });
        store::write_approvals(paths, &approvals)?;
    }
    report.action = if exists { "unchanged" } else { "granted" }.to_owned();
    report.dry_run = dry_run;
    Ok(report)
}

/// Revoke every revision-bound generated recipe approval for one logical skill.
pub fn revoke(
    paths: &StorePaths,
    value: &str,
    dry_run: bool,
) -> DaloResult<DeliveryApprovalReport> {
    let canonical = crate::approval::canonical_skill(paths, value)?;
    let mut approvals = store::read_approvals(paths)?;
    let prefix = format!("{canonical}@");
    let mut removed = None;
    approvals.approvals.retain(|record| {
        let matches = record.scope == APPROVAL_SCOPE && record.value.starts_with(&prefix);
        if matches {
            removed = Some(record.value.clone());
        }
        !matches
    });
    let changed = removed.is_some();
    if changed && !dry_run {
        store::write_approvals(paths, &approvals)?;
    }
    let mut report = inspect(paths, &canonical)?;
    report.approval_value = removed.unwrap_or(report.approval_value);
    report.action = if changed { "revoked" } else { "unchanged" }.to_owned();
    report.dry_run = dry_run;
    Ok(report)
}

fn inspect(paths: &StorePaths, value: &str) -> DaloResult<DeliveryApprovalReport> {
    let canonical = crate::approval::canonical_skill(paths, value)?;
    let (source_id, _) = canonical
        .split_once(':')
        .expect("canonical skill references are source-qualified");
    let config = store::read_config(paths)?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .expect("canonical skill source remains configured");
    if source.kind == SourceKind::Local {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery `{canonical}` requires immutable Git source provenance; local recipes cannot be approved"
            ),
        });
    }
    if crate::git::is_dirty(&source.path)? {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery source `{source_id}` has tracked changes; commit or restore them before approving `{canonical}`"
            ),
        });
    }
    let commit = crate::git::rev_parse_head(&source.path)?;
    let source_lock = crate::catalog::read_source_lock(paths).ok();
    let provenance = crate::source::source_provenance(source, source_lock.as_ref());
    if provenance
        .resolved_commit
        .as_deref()
        .is_some_and(|resolved| resolved != commit)
    {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery source `{source_id}` checkout does not match its resolved pin"
            ),
        });
    }
    let mut inventory = crate::inventory::scan_source(source_id, &source.path)?;
    let (generator, generator_contract_hash, providers, manifest_path, approval_value) = {
        let skill = inventory
            .skills
            .iter_mut()
            .find(|skill| skill.source_ref == canonical)
            .expect("canonical skill remains present in the same inventory");
        skill
            .delivery
            .bind_generated_approvals(&canonical, Some(commit), &[]);
        let SkillDelivery::Generated {
            generator,
            generator_contract_hash,
            providers,
            manifest_path,
            ..
        } = &skill.delivery
        else {
            return Err(DaloError::InvalidArgument {
                reason: format!("skill `{canonical}` does not declare a generated delivery recipe"),
            });
        };
        let approval_value = skill
            .delivery
            .generated_approval_value(&canonical)
            .expect("non-local generated delivery has bound commit provenance");
        (
            generator.clone(),
            generator_contract_hash.clone(),
            providers.clone(),
            manifest_path.clone(),
            approval_value,
        )
    };
    if !crate::git::is_tracked_file(&source.path, &manifest_path)? {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery manifest `{}` is not tracked by source commit `{}`",
                manifest_path.display(),
                provenance.checkout_commit.as_deref().unwrap_or("unknown")
            ),
        });
    }
    let (plugin, tool) = inventory
        .plugins
        .iter()
        .find_map(|plugin| {
            plugin
                .tools
                .iter()
                .find(|tool| tool.source_ref == generator)
                .map(|tool| (plugin, tool))
        })
        .expect("generated delivery scanner resolved the generator tool");
    let mut generator_files = vec![plugin.manifest_file.clone()];
    generator_files.extend(tool.files.iter().map(|file| plugin.path.join(&file.path)));
    for file in generator_files {
        if !crate::git::is_tracked_file(&source.path, &file)? {
            return Err(DaloError::StateError {
                reason: format!(
                    "generator contract file `{}` is not tracked by source commit `{}`",
                    file.display(),
                    provenance.checkout_commit.as_deref().unwrap_or("unknown")
                ),
            });
        }
    }
    Ok(DeliveryApprovalReport {
        skill: canonical,
        approval_value,
        generator,
        generator_contract_hash,
        providers,
        action: String::new(),
        dry_run: false,
        execution: "not_run".to_owned(),
    })
}
