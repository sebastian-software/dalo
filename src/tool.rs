//! Portable local-tool trust, audit, argument composition, and immutable staging.
//!
//! Every operation in this module is inert with respect to the declared tool:
//! Dalo validates bytes and constructs invocation data but never starts it.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::inventory::SourceInventory;
use crate::plugin::{
    self, PluginInventoryWarning, ToolInputType, ToolPlatform, ToolRecord, ToolRuntime,
};
use crate::source::{SourceConfig, SourceHeadCache, SourceProvenance};
use crate::store::{self, ApprovalRecord, StorePaths};

/// Stable approval scope for exact executable contracts.
pub const APPROVAL_SCOPE: &str = "tool";

/// Complete read-only inventory and state report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolListReport {
    /// Validated tools in deterministic identity order.
    pub tools: Vec<ToolStatusReport>,
    /// Plugin packages rejected while collecting the inventory.
    pub warnings: Vec<PluginInventoryWarning>,
}

/// One discovered tool joined with local trust and availability state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolStatusReport {
    /// Validated descriptor and contract hash.
    pub tool: ToolRecord,
    /// Whole plugin package hash retained only as provenance.
    pub plugin_package_hash: String,
    /// Package directory retained only as provenance.
    pub plugin_path: PathBuf,
    /// Source revision and origin provenance, excluded from approval identity.
    pub source_provenance: SourceProvenance,
    /// Exact approval value for this contract.
    pub approval_value: String,
    /// Current trust/availability state.
    pub state: ToolState,
    /// Immutable path, when staged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged_path: Option<PathBuf>,
    /// Actionable explanation.
    pub diagnostic: String,
}

/// Mutually exclusive local-tool readiness states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    /// Current platform is excluded by the descriptor.
    PlatformMismatch,
    /// Required runtime executable is absent from PATH.
    RuntimeMissing,
    /// Exact current contract has never been approved.
    PendingApproval,
    /// A prior contract for this identity was approved, but the current hash differs.
    HashDrift,
    /// Immutable bytes remain staged but the exact approval was revoked.
    Revoked,
    /// Approval exists, but immutable staging has not completed.
    ApprovedNotStaged,
    /// Staged files no longer match their contract hashes.
    AuditFailure,
    /// Exact approval and immutable staged bytes are both valid.
    Ready,
}

/// Read-only deterministic executable-closure audit.
#[derive(Debug, Clone, Serialize)]
pub struct ToolAuditReport {
    /// Source-qualified tool identity.
    pub tool: String,
    /// Exact invocation-contract hash.
    pub contract_hash: String,
    /// Whole plugin hash retained as provenance.
    pub plugin_package_hash: String,
    /// Whether every closure file still matches inventory.
    pub passed: bool,
    /// Stable findings; empty on success.
    pub findings: Vec<String>,
}

/// Result of granting or revoking exact tool trust.
#[derive(Debug, Clone, Serialize)]
pub struct ToolApprovalReport {
    /// Exact source-qualified tool identity.
    pub tool: String,
    /// Exact content-bound approval value.
    pub approval_value: String,
    /// `granted`, `revoked`, or `unchanged`.
    pub action: String,
    /// Immutable staged closure path for a grant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged_path: Option<PathBuf>,
    /// Whether mutation was suppressed.
    pub dry_run: bool,
}

/// Collect all valid tool descriptors without executing code.
pub fn list(paths: &StorePaths) -> DaloResult<ToolListReport> {
    let config = store::read_config(paths)?;
    let approvals = store::read_approvals(paths)?;
    let inventories = scan_plugin_inventories(&config.sources);
    Ok(list_from_inventories(
        paths,
        &config.sources,
        &approvals.approvals,
        &inventories,
    ))
}

/// Scan only the plugin portion of every enabled source for standalone
/// tool/hook commands. Shared command paths instead consume the resolver's
/// full source inventories through [`list_from_inventories`].
pub(crate) fn scan_plugin_inventories(sources: &[SourceConfig]) -> Vec<SourceInventory> {
    sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| {
            let plugins = plugin::scan_source_plugins(&source.id, &source.path);
            SourceInventory {
                source_id: source.id.clone(),
                skills: Vec::new(),
                agents: Vec::new(),
                plugins: plugins.plugins,
                warnings: Vec::new(),
                agent_warnings: Vec::new(),
                plugin_warnings: plugins.warnings,
            }
        })
        .collect()
}

/// Join already-scanned plugin inventories with current local tool trust state.
///
/// Status, doctor, sync, and planning reuse the inventories produced by their
/// shared resolver pass rather than reopening every plugin package for each
/// consumer. The standalone tool command uses [`list`] above, which creates
/// the same input once.
#[must_use]
pub fn list_from_inventories(
    paths: &StorePaths,
    sources: &[SourceConfig],
    approvals: &[ApprovalRecord],
    inventories: &[SourceInventory],
) -> ToolListReport {
    list_from_inventories_with_head_cache(
        paths,
        sources,
        approvals,
        inventories,
        &mut SourceHeadCache::default(),
    )
}

pub(crate) fn list_from_inventories_with_head_cache(
    paths: &StorePaths,
    sources: &[SourceConfig],
    approvals: &[ApprovalRecord],
    inventories: &[SourceInventory],
    head_cache: &mut SourceHeadCache,
) -> ToolListReport {
    let source_lock = crate::catalog::read_source_lock(paths).ok();
    let mut tools = Vec::new();
    let mut warnings = Vec::new();
    for inventory in inventories {
        let Some(source) = sources
            .iter()
            .find(|source| source.enabled && source.id == inventory.source_id)
        else {
            continue;
        };
        let provenance = crate::source::source_provenance_with_head_cache(
            source,
            source_lock.as_ref(),
            head_cache,
        );
        warnings.extend(inventory.plugin_warnings.iter().cloned());
        for plugin in &inventory.plugins {
            for tool in &plugin.tools {
                tools.push(status_for(
                    paths,
                    tool.clone(),
                    plugin.package_hash.clone(),
                    plugin.path.clone(),
                    provenance.clone(),
                    approvals,
                ));
            }
        }
    }
    tools.sort_by(|left, right| left.tool.source_ref.cmp(&right.tool.source_ref));
    warnings.sort_by(|left, right| left.path.cmp(&right.path));
    ToolListReport { tools, warnings }
}

/// Find one exact source-qualified tool.
pub fn show(paths: &StorePaths, value: &str) -> DaloResult<ToolStatusReport> {
    let report = list(paths)?;
    report
        .tools
        .into_iter()
        .find(|candidate| candidate.tool.source_ref == value)
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: format!(
                "unknown tool `{value}`; use `dalo tool list` and an exact `<source>:<plugin>#tool:<id>` identity"
            ),
        })
}

/// Audit the source closure without invoking its entry point.
pub fn audit(paths: &StorePaths, value: &str) -> DaloResult<ToolAuditReport> {
    let status = show(paths, value)?;
    Ok(audit_status(&status))
}

/// Audit and stage one exact tool contract without granting execution trust.
///
/// Aggregated reviews use this as a prepare phase before committing all
/// separately scoped approval records with one atomic ledger write. The
/// staged bytes remain inert until the matching content-bound approval exists.
pub fn prepare_approval(
    paths: &StorePaths,
    value: &str,
    expected_approval_value: &str,
) -> DaloResult<PathBuf> {
    let status = show(paths, value)?;
    if status.approval_value != expected_approval_value {
        return Err(DaloError::StateError {
            reason: format!(
                "tool `{}` changed after review: expected `{expected_approval_value}`, found `{}`",
                status.tool.source_ref, status.approval_value
            ),
        });
    }
    let audit = audit_status(&status);
    if !audit.passed {
        return Err(DaloError::StateError {
            reason: format!(
                "tool `{}` failed its executable-closure audit: {}",
                status.tool.source_ref,
                audit.findings.join("; ")
            ),
        });
    }
    if matches!(
        status.state,
        ToolState::PlatformMismatch | ToolState::RuntimeMissing
    ) {
        return Err(DaloError::StateError {
            reason: status.diagnostic,
        });
    }
    stage(paths, &status)?;
    Ok(staged_root(paths, &status.tool.contract_hash))
}

/// Grant exact tool trust and atomically stage the reviewed bytes.
pub fn approve(paths: &StorePaths, value: &str, dry_run: bool) -> DaloResult<ToolApprovalReport> {
    let status = show(paths, value)?;
    let audit = audit_status(&status);
    if !audit.passed {
        return Err(DaloError::StateError {
            reason: format!(
                "tool `{}` failed its executable-closure audit: {}",
                status.tool.source_ref,
                audit.findings.join("; ")
            ),
        });
    }
    if matches!(
        status.state,
        ToolState::PlatformMismatch | ToolState::RuntimeMissing
    ) {
        return Err(DaloError::StateError {
            reason: status.diagnostic,
        });
    }
    let mut approvals = store::read_approvals(paths)?;
    let record = ApprovalRecord {
        scope: APPROVAL_SCOPE.to_owned(),
        value: status.approval_value.clone(),
    };
    let exists = approvals.approvals.contains(&record);
    let staged_path = staged_root(paths, &status.tool.contract_hash);
    if !dry_run {
        stage(paths, &status)?;
        if !exists {
            approvals.approvals.push(record);
            approvals.approvals.sort_by(|left, right| {
                left.scope
                    .cmp(&right.scope)
                    .then(left.value.cmp(&right.value))
            });
            store::write_approvals(paths, &approvals)?;
        }
    }
    Ok(ToolApprovalReport {
        tool: status.tool.source_ref,
        approval_value: status.approval_value,
        action: if exists { "unchanged" } else { "granted" }.to_owned(),
        staged_path: Some(staged_path),
        dry_run,
    })
}

/// Revoke every approved contract hash for one exact tool identity.
pub fn revoke(paths: &StorePaths, value: &str, dry_run: bool) -> DaloResult<ToolApprovalReport> {
    validate_identity_shape(value)?;
    let mut approvals = store::read_approvals(paths)?;
    let prefix = format!("{value}@sha256:");
    let before = approvals.approvals.len();
    let mut removed = None;
    approvals.approvals.retain(|record| {
        let matches = record.scope == APPROVAL_SCOPE && record.value.starts_with(&prefix);
        if matches {
            removed = Some(record.value.clone());
        }
        !matches
    });
    let changed = before != approvals.approvals.len();
    if changed && !dry_run {
        store::write_approvals(paths, &approvals)?;
    }
    Ok(ToolApprovalReport {
        tool: value.to_owned(),
        approval_value: removed.unwrap_or(prefix),
        action: if changed { "revoked" } else { "unchanged" }.to_owned(),
        staged_path: None,
        dry_run,
    })
}

/// Build the invariant argv vector with values kept as opaque data.
pub fn build_argv(
    tool: &ToolRecord,
    staged_tool_root: &Path,
    inputs: &BTreeMap<String, String>,
) -> DaloResult<Vec<String>> {
    let declared = tool
        .inputs
        .iter()
        .map(|input| (input.name.as_str(), input))
        .collect::<BTreeMap<_, _>>();
    for name in inputs.keys() {
        if !declared.contains_key(name.as_str()) {
            return Err(DaloError::InvalidArgument {
                reason: format!("unknown input `{name}` for tool `{}`", tool.source_ref),
            });
        }
    }
    for input in &tool.inputs {
        match inputs.get(&input.name) {
            None if input.required => {
                return Err(DaloError::InvalidArgument {
                    reason: format!("missing required input `{}`", input.name),
                });
            }
            Some(value) => validate_input_value(input.kind, value)?,
            None => {}
        }
    }
    let entry = staged_tool_root.join(&tool.entry);
    let mut argv = match tool.runtime {
        ToolRuntime::Executable => vec![entry.to_string_lossy().into_owned()],
        ToolRuntime::Python => vec!["python3".to_owned(), entry.to_string_lossy().into_owned()],
        ToolRuntime::Node => vec!["node".to_owned(), entry.to_string_lossy().into_owned()],
    };
    for template in &tool.argv {
        if let Some(name) = template
            .strip_prefix("${input.")
            .and_then(|value| value.strip_suffix('}'))
        {
            argv.push(inputs.get(name).cloned().unwrap_or_default());
        } else {
            argv.push(template.clone());
        }
    }
    Ok(argv)
}

fn status_for(
    paths: &StorePaths,
    tool: ToolRecord,
    plugin_package_hash: String,
    plugin_path: PathBuf,
    source_provenance: SourceProvenance,
    approvals: &[ApprovalRecord],
) -> ToolStatusReport {
    let approval_value = approval_value(&tool);
    let exact_approval = approvals
        .iter()
        .any(|record| record.scope == APPROVAL_SCOPE && record.value == approval_value);
    let identity_prefix = format!("{}@sha256:", tool.source_ref);
    let prior_approval = approvals
        .iter()
        .any(|record| record.scope == APPROVAL_SCOPE && record.value.starts_with(&identity_prefix));
    let staged_path = staged_root(paths, &tool.contract_hash);
    let staged = staged_path.is_dir();
    let platform_matches = platform_matches(&tool.platforms);
    let runtime_available = tool.runtime.executable().is_none_or(executable_on_path);
    let (state, diagnostic) = if !platform_matches {
        (
            ToolState::PlatformMismatch,
            "current platform is not admitted by the tool descriptor".to_owned(),
        )
    } else if !runtime_available {
        (
            ToolState::RuntimeMissing,
            format!(
                "required runtime `{}` is absent from PATH",
                tool.runtime.executable().unwrap_or_default()
            ),
        )
    } else if exact_approval && staged && !verify_staged(&tool, &staged_path) {
        (
            ToolState::AuditFailure,
            "immutable staged files do not match the approved contract".to_owned(),
        )
    } else if exact_approval && staged {
        (
            ToolState::Ready,
            "exact contract approved and staged".to_owned(),
        )
    } else if exact_approval {
        (
            ToolState::ApprovedNotStaged,
            "exact contract approved but immutable staging is incomplete".to_owned(),
        )
    } else if staged {
        (
            ToolState::Revoked,
            "staged bytes exist but no exact execution approval remains".to_owned(),
        )
    } else if prior_approval {
        (
            ToolState::HashDrift,
            "security-relevant tool contract changed and requires reapproval".to_owned(),
        )
    } else {
        (
            ToolState::PendingApproval,
            format!("run `dalo approve tool {}`", tool.source_ref),
        )
    };
    ToolStatusReport {
        tool,
        plugin_package_hash,
        plugin_path,
        source_provenance,
        approval_value,
        state,
        staged_path: staged.then_some(staged_path),
        diagnostic,
    }
}

fn audit_status(status: &ToolStatusReport) -> ToolAuditReport {
    let mut findings = Vec::new();
    for file in &status.tool.files {
        let path = status.plugin_path.join(&file.path);
        match fs::read(&path) {
            Ok(bytes)
                if hash_bytes(&bytes) == file.content_hash
                    && fs::symlink_metadata(&path).is_ok_and(|metadata| {
                        metadata.is_file()
                            && !metadata.file_type().is_symlink()
                            && (metadata.permissions().mode() & 0o111 != 0) == file.executable
                    }) => {}
            Ok(_) => findings.push(format!("hash_drift:{}", file.path)),
            Err(error) => findings.push(format!("unreadable:{}:{error}", file.path)),
        }
    }
    ToolAuditReport {
        tool: status.tool.source_ref.clone(),
        contract_hash: status.tool.contract_hash.clone(),
        plugin_package_hash: status.plugin_package_hash.clone(),
        passed: findings.is_empty(),
        findings,
    }
}

fn stage(paths: &StorePaths, status: &ToolStatusReport) -> DaloResult<()> {
    let destination = staged_root(paths, &status.tool.contract_hash);
    if destination.exists() {
        if verify_staged(&status.tool, &destination) {
            return Ok(());
        }
        return Err(DaloError::StateError {
            reason: format!(
                "content-addressed tool path `{}` exists with unexpected bytes",
                destination.display()
            ),
        });
    }
    let parent = destination.parent().ok_or_else(|| DaloError::StateError {
        reason: "tool staging destination has no parent".to_owned(),
    })?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".tool-stage-")
        .tempdir_in(parent)?;
    for file in &status.tool.files {
        let source = status.plugin_path.join(&file.path);
        let target = temporary.path().join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = fs::read(&source)?;
        if hash_bytes(&bytes) != file.content_hash {
            return Err(DaloError::StateError {
                reason: format!("tool source changed during staging: `{}`", file.path),
            });
        }
        fs::write(&target, bytes)?;
        fs::set_permissions(
            &target,
            fs::Permissions::from_mode(if file.executable { 0o555 } else { 0o444 }),
        )?;
    }
    make_directories_read_only(temporary.path())?;
    let temporary_path = temporary.keep();
    fs::rename(&temporary_path, &destination)?;
    Ok(())
}

fn make_directories_read_only(root: &Path) -> DaloResult<()> {
    let mut directories = vec![root.to_path_buf()];
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            make_directories_read_only(&path)?;
        }
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o555))?;
    }
    Ok(())
}

fn verify_staged(tool: &ToolRecord, root: &Path) -> bool {
    let expected = tool
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    if !collect_staged_paths(root, root, &mut actual) || actual != expected {
        return false;
    }
    tool.files.iter().all(|file| {
        fs::symlink_metadata(root.join(&file.path)).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && fs::read(root.join(&file.path))
                    .is_ok_and(|bytes| hash_bytes(&bytes) == file.content_hash)
        })
    })
}

/// Verify a staged immutable tool closure before dispatcher execution.
#[must_use]
pub(crate) fn verify_staged_contract(tool: &ToolRecord, root: &Path) -> bool {
    verify_staged(tool, root)
}

fn collect_staged_paths(root: &Path, current: &Path, paths: &mut BTreeSet<String>) -> bool {
    let Ok(entries) = fs::read_dir(current) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.file_type().is_symlink() {
            return false;
        }
        if metadata.is_dir() {
            if !collect_staged_paths(root, &path, paths) {
                return false;
            }
        } else if metadata.is_file() {
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            let Some(relative) = relative.to_str() else {
                return false;
            };
            paths.insert(relative.replace(std::path::MAIN_SEPARATOR, "/"));
        } else {
            return false;
        }
    }
    true
}

fn staged_root(paths: &StorePaths, contract_hash: &str) -> PathBuf {
    paths.tools_dir.join("sha256").join(contract_hash)
}

fn approval_value(tool: &ToolRecord) -> String {
    format!("{}@sha256:{}", tool.source_ref, tool.contract_hash)
}

fn validate_identity_shape(value: &str) -> DaloResult<()> {
    let valid = value
        .split_once("#tool:")
        .is_some_and(|(plugin, tool)| plugin.contains(':') && !tool.is_empty());
    if valid {
        Ok(())
    } else {
        Err(DaloError::InvalidArgument {
            reason: "tool values must use `<source>:<plugin>#tool:<id>`".to_owned(),
        })
    }
}

fn platform_matches(platforms: &[ToolPlatform]) -> bool {
    platforms.is_empty()
        || if cfg!(target_os = "macos") {
            platforms.contains(&ToolPlatform::Macos)
        } else if cfg!(target_os = "linux") {
            platforms.contains(&ToolPlatform::Linux)
        } else {
            false
        }
}

fn executable_on_path(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            fs::metadata(directory.join(name)).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn validate_input_value(kind: ToolInputType, value: &str) -> DaloResult<()> {
    let valid = match kind {
        ToolInputType::String | ToolInputType::Path => !value.contains('\0'),
        ToolInputType::Integer => value.parse::<i64>().is_ok(),
        ToolInputType::Boolean => matches!(value, "true" | "false"),
    };
    if valid {
        Ok(())
    } else {
        Err(DaloError::InvalidArgument {
            reason: format!("input value does not match declared {kind:?} type"),
        })
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{ToolAvailability, ToolCapability, ToolCwd, ToolFileRecord, ToolInput};
    use tempfile::TempDir;

    fn tool() -> ToolRecord {
        ToolRecord {
            schema_version: 1,
            id: "detector".to_owned(),
            source_ref: "team:quality#tool:detector".to_owned(),
            entry: "bin/detect".to_owned(),
            runtime: ToolRuntime::Executable,
            runtime_version: None,
            platforms: Vec::new(),
            inputs: vec![ToolInput {
                name: "path".to_owned(),
                kind: ToolInputType::Path,
                required: true,
            }],
            argv: vec!["--path".to_owned(), "${input.path}".to_owned()],
            cwd: ToolCwd::ToolRoot,
            env: Vec::new(),
            capabilities: vec![ToolCapability::FilesystemRead],
            availability: ToolAvailability::Required,
            files: vec![ToolFileRecord {
                path: "bin/detect".to_owned(),
                executable: true,
                content_hash: "00".repeat(32),
            }],
            contract_hash: "11".repeat(32),
        }
    }

    #[test]
    fn invocation_keeps_shell_syntax_inside_one_data_argument() {
        let mut inputs = BTreeMap::new();
        inputs.insert("path".to_owned(), "$(touch /tmp/pwned); *.md".to_owned());
        let argv =
            build_argv(&tool(), Path::new("/immutable"), &inputs).expect("typed argv should build");
        assert_eq!(argv.len(), 3);
        assert_eq!(argv[2], "$(touch /tmp/pwned); *.md");
    }

    #[test]
    fn invocation_rejects_unknown_inputs_without_changing_shape() {
        let mut inputs = BTreeMap::new();
        inputs.insert("path".to_owned(), "x".to_owned());
        inputs.insert("extra".to_owned(), "--evil".to_owned());
        assert!(build_argv(&tool(), Path::new("/immutable"), &inputs).is_err());
    }

    fn fixture() -> (TempDir, StorePaths, PathBuf) {
        let temp = TempDir::new().unwrap();
        store::init_store(temp.path().join("store"), false).unwrap();
        let paths = StorePaths::new(temp.path().join("store"));
        let package = paths.local_dir.join("plugins/quality");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(
            package.join(crate::plugin::PLUGIN_FILE),
            r#"schema_version = 1
[plugin]
name = "quality"
description = "Quality tools"

[[tool]]
schema_version = 1
id = "detector"
entry = "bin/detect"
runtime = "executable"
platforms = ["macos", "linux"]
argv = ["--check"]
cwd = "tool_root"
capabilities = ["filesystem_read"]
availability = "required"
"#,
        )
        .unwrap();
        let entry = package.join("bin/detect");
        fs::write(&entry, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
        (temp, paths, package)
    }

    fn make_staging_removable(path: &Path) {
        if let Ok(entries) = fs::read_dir(path) {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    make_staging_removable(&entry.path());
                }
            }
        }
    }

    #[test]
    fn approval_stages_exact_bytes_and_readme_changes_reuse_trust() {
        let (_temp, paths, package) = fixture();
        let identity = "local:quality#tool:detector";
        let pending = show(&paths, identity).unwrap();
        assert_eq!(pending.state, ToolState::PendingApproval);
        let contract = pending.tool.contract_hash.clone();
        let package_hash = pending.plugin_package_hash;

        let granted = approve(&paths, identity, false).unwrap();
        let staged = granted.staged_path.unwrap();
        assert!(staged.join("bin/detect").is_file());
        assert_eq!(show(&paths, identity).unwrap().state, ToolState::Ready);

        fs::write(package.join("README.md"), "unrelated").unwrap();
        let advanced = show(&paths, identity).unwrap();
        assert_eq!(advanced.tool.contract_hash, contract);
        assert_ne!(advanced.plugin_package_hash, package_hash);
        assert_eq!(advanced.state, ToolState::Ready);
        make_staging_removable(&paths.tools_dir);
    }

    #[test]
    fn byte_drift_revocation_and_staged_audit_failure_are_distinct() {
        let (_temp, paths, package) = fixture();
        let identity = "local:quality#tool:detector";
        let granted = approve(&paths, identity, false).unwrap();
        let staged = granted.staged_path.unwrap();

        revoke(&paths, identity, false).unwrap();
        assert_eq!(show(&paths, identity).unwrap().state, ToolState::Revoked);
        approve(&paths, identity, false).unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(staged.join("bin"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(staged.join("bin/detect"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(staged.join("bin/detect"), "tampered").unwrap();
        assert_eq!(
            show(&paths, identity).unwrap().state,
            ToolState::AuditFailure
        );

        fs::write(package.join("bin/detect"), "changed source").unwrap();
        fs::set_permissions(
            package.join("bin/detect"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(show(&paths, identity).unwrap().state, ToolState::HashDrift);
        make_staging_removable(&paths.tools_dir);
    }

    #[test]
    fn dry_run_approval_never_stages_or_writes_approval() {
        let (_temp, paths, _package) = fixture();
        let identity = "local:quality#tool:detector";
        let report = approve(&paths, identity, true).unwrap();
        assert!(report.dry_run);
        assert!(!report.staged_path.unwrap().exists());
        assert!(store::read_approvals(&paths).unwrap().approvals.is_empty());
    }

    #[test]
    fn interrupted_temporary_stage_never_becomes_the_promoted_contract() {
        let (_temp, paths, _package) = fixture();
        let staging_parent = paths.tools_dir.join("sha256");
        let interrupted = staging_parent.join(".tool-stage-interrupted");
        fs::create_dir_all(&interrupted).unwrap();
        fs::write(interrupted.join("partial"), "not approved").unwrap();

        let identity = "local:quality#tool:detector";
        let granted = approve(&paths, identity, false).unwrap();
        let destination = granted.staged_path.unwrap();
        assert!(destination.join("bin/detect").is_file());
        assert!(!destination.join("partial").exists());
        assert_eq!(show(&paths, identity).unwrap().state, ToolState::Ready);
        make_staging_removable(&paths.tools_dir);
    }

    #[test]
    fn platform_mismatch_is_distinct_and_never_stages() {
        let (_temp, paths, package) = fixture();
        let unsupported = if cfg!(target_os = "macos") {
            "linux"
        } else {
            "macos"
        };
        let manifest = fs::read_to_string(package.join(crate::plugin::PLUGIN_FILE)).unwrap();
        fs::write(
            package.join(crate::plugin::PLUGIN_FILE),
            manifest.replace("[\"macos\", \"linux\"]", &format!("[\"{unsupported}\"]")),
        )
        .unwrap();
        let identity = "local:quality#tool:detector";
        assert_eq!(
            show(&paths, identity).unwrap().state,
            ToolState::PlatformMismatch
        );
        assert!(approve(&paths, identity, false).is_err());
        assert!(!paths.tools_dir.join("sha256").exists());
    }
}
