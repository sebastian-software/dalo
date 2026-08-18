//! Generated-delivery approval, execution, audit, and immutable promotion.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};

use crate::error::{DaloError, DaloResult};
use crate::inventory::{PrebuiltSkillArtifact, SkillDelivery};
use crate::resolver::Resolution;
use crate::source::SourceKind;
use crate::store::{self, ApprovalRecord, StorePaths};

const GENERATOR_TIMEOUT: Duration = Duration::from_secs(120);
const GENERATOR_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_GENERATOR_STDERR: u64 = 1024 * 1024;
const MAX_GENERATED_ENTRIES: usize = 4096;
const MAX_GENERATED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_GENERATED_DEPTH: usize = 32;

/// Stable approval scope for exact generated-delivery recipes.
pub const APPROVAL_SCOPE: &str = "delivery";

/// Cache disposition for one resolved generated derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedCacheState {
    /// No immutable result exists; a real sync would run the generator.
    Miss,
    /// An existing immutable derivation passed verification and audit.
    Hit,
    /// This sync executed, audited, and promoted a new derivation.
    Generated,
}

impl GeneratedCacheState {
    /// Stable report label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Miss => "miss",
            Self::Hit => "hit",
            Self::Generated => "generated",
        }
    }
}

/// Immutable provider artifacts prepared for one generated skill.
#[derive(Debug, Clone)]
pub(crate) struct GeneratedArtifactSet {
    pub(crate) root: PathBuf,
    pub(crate) providers: BTreeMap<String, PrebuiltSkillArtifact>,
    pub(crate) derivation_hash: String,
    pub(crate) cache_state: GeneratedCacheState,
}

struct GeneratedExecution<'a> {
    source_ref: &'a str,
    generator: &'a str,
    generator_contract_hash: &'a str,
    output_input: &'a str,
    providers: &'a BTreeMap<String, PathBuf>,
    source_commit: &'a str,
}

/// Prepare every fully approved generated delivery before link planning.
pub(crate) fn prepare_generated_artifacts(
    paths: &StorePaths,
    resolution: &Resolution,
    needed: &std::collections::BTreeSet<String>,
    dry_run: bool,
) -> DaloResult<BTreeMap<String, GeneratedArtifactSet>> {
    let mut prepared = BTreeMap::new();
    for skill in &resolution.active_skills {
        if !needed.contains(&skill.source_ref) {
            continue;
        }
        let SkillDelivery::Generated {
            generator,
            generator_contract_hash,
            output_input,
            providers,
            recipe_hash,
            source_commit: Some(source_commit),
            recipe_approved: true,
            generator_approved: true,
            ..
        } = &skill.delivery
        else {
            continue;
        };
        let derivation_hash = derivation_hash(&skill.source_ref, source_commit, recipe_hash);
        let root = paths.generated_dir.join("sha256").join(&derivation_hash);
        let (artifacts, cache_state) = if root.exists() {
            (
                validate_and_audit_outputs(
                    paths,
                    &skill.source_ref,
                    &root,
                    providers,
                    true,
                    !dry_run,
                )?,
                GeneratedCacheState::Hit,
            )
        } else if dry_run {
            (BTreeMap::new(), GeneratedCacheState::Miss)
        } else {
            let execution = GeneratedExecution {
                source_ref: &skill.source_ref,
                generator,
                generator_contract_hash,
                output_input,
                providers,
                source_commit,
            };
            let artifacts = execute_and_promote(paths, &execution, &root)?;
            (artifacts, GeneratedCacheState::Generated)
        };
        prepared.insert(
            skill.source_ref.clone(),
            GeneratedArtifactSet {
                root,
                providers: artifacts,
                derivation_hash,
                cache_state,
            },
        );
    }
    Ok(prepared)
}

pub(crate) fn derivation_hash(source_ref: &str, source_commit: &str, recipe_hash: &str) -> String {
    let mut hash = Sha256::new();
    for value in [
        "dalo-generated-derivation-v1",
        source_ref,
        source_commit,
        recipe_hash,
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn execute_and_promote(
    paths: &StorePaths,
    execution: &GeneratedExecution<'_>,
    destination: &Path,
) -> DaloResult<BTreeMap<String, PrebuiltSkillArtifact>> {
    let source_ref = execution.source_ref;
    let generator = execution.generator;
    let generator_contract_hash = execution.generator_contract_hash;
    let output_input = execution.output_input;
    let providers = execution.providers;
    let source_commit = execution.source_commit;
    verify_source_snapshot(paths, source_ref, source_commit)?;
    let status = crate::tool::show(paths, generator)?;
    if status.tool.contract_hash != generator_contract_hash
        || status.state != crate::tool::ToolState::Ready
    {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery `{source_ref}` requires the exact approved and staged generator `{generator}`"
            ),
        });
    }
    let tool_root = status.staged_path.ok_or_else(|| DaloError::StateError {
        reason: format!("generator `{generator}` has no immutable staged closure"),
    })?;
    if !crate::tool::verify_staged_contract(&status.tool, &tool_root) {
        return Err(DaloError::StateError {
            reason: format!("generator `{generator}` failed immutable closure verification"),
        });
    }

    let parent = destination.parent().ok_or_else(|| DaloError::StateError {
        reason: "generated delivery cache destination has no parent".to_owned(),
    })?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".delivery-stage-")
        .tempdir_in(parent)?;
    for relative in providers.values() {
        fs::create_dir_all(staging.path().join(relative))?;
    }
    let mut inputs = BTreeMap::new();
    inputs.insert(
        output_input.to_owned(),
        staging.path().to_string_lossy().into_owned(),
    );
    let argv = crate::tool::build_argv(&status.tool, &tool_root, &inputs)?;
    let execution = run_generator(&status.tool, &tool_root, &argv, source_ref);
    let source_snapshot = verify_source_snapshot(paths, source_ref, source_commit);
    source_snapshot?;
    execution?;
    validate_generated_tree(staging.path(), providers.values(), false)?;

    // Never promote the tree whose path was disclosed to generator code. Copy
    // only validated regular files into a fresh inode set, audit that snapshot,
    // then verify it once more from its final immutable cache path.
    let snapshot = tempfile::Builder::new()
        .prefix(".delivery-stage-")
        .tempdir_in(parent)?;
    copy_generated_tree(staging.path(), snapshot.path())?;
    validate_and_audit_outputs(paths, source_ref, snapshot.path(), providers, false, true)?;
    make_tree_read_only(snapshot.path())?;
    let snapshot_path = snapshot.keep();
    if let Err(error) = fs::rename(&snapshot_path, destination) {
        let _ = make_tree_writable(&snapshot_path);
        let _ = fs::remove_dir_all(&snapshot_path);
        return Err(error.into());
    }
    match validate_and_audit_outputs(paths, source_ref, destination, providers, true, false) {
        Ok(artifacts) => Ok(artifacts),
        Err(error) => {
            let _ = make_tree_writable(destination);
            let _ = fs::remove_dir_all(destination);
            Err(error)
        }
    }
}

fn verify_source_snapshot(
    paths: &StorePaths,
    source_ref: &str,
    expected_commit: &str,
) -> DaloResult<()> {
    let source_id = source_ref
        .split_once(':')
        .map(|(source, _)| source)
        .ok_or_else(|| DaloError::StateError {
            reason: format!("generated delivery `{source_ref}` has no source identity"),
        })?;
    let config = store::read_config(paths)?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| DaloError::StateError {
            reason: format!("generated delivery source `{source_id}` is no longer configured"),
        })?;
    let actual_commit = crate::git::rev_parse_head(&source.path)?;
    if actual_commit != expected_commit || crate::git::is_dirty(&source.path)? {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery source `{source_id}` changed during execution; output was not promoted"
            ),
        });
    }
    Ok(())
}

fn run_generator(
    tool: &crate::plugin::ToolRecord,
    tool_root: &Path,
    argv: &[String],
    source_ref: &str,
) -> DaloResult<()> {
    let (program, args) = argv.split_first().ok_or_else(|| DaloError::StateError {
        reason: "generator produced an empty argv".to_owned(),
    })?;
    let program = Path::new(program);
    let approved_entry = tool_root.join(&tool.entry);
    if tool.runtime != crate::plugin::ToolRuntime::Executable
        || store::comparable_path(program) != store::comparable_path(&approved_entry)
    {
        return Err(DaloError::StateError {
            reason: format!(
                "generated delivery must execute the exact staged entry `{}`",
                approved_entry.display()
            ),
        });
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(tool_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PATH", "");
    for name in &tool.env {
        if name == "PATH" {
            return Err(DaloError::StateError {
                reason: "generated delivery cannot admit ambient PATH".to_owned(),
            });
        }
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let stderr = child.stderr.take().expect("generator stderr is piped");
    let stderr_reader = thread::spawn(move || read_bounded(stderr));
    let deadline = Instant::now() + GENERATOR_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                terminate_generator(&mut child);
                let _ = stderr_reader.join();
                return Err(DaloError::StateError {
                    reason: format!(
                        "generated delivery `{source_ref}` timed out after {} seconds",
                        GENERATOR_TIMEOUT.as_secs()
                    ),
                });
            }
            Err(error) => {
                terminate_generator(&mut child);
                let _ = stderr_reader.join();
                return Err(error.into());
            }
        }
    };
    terminate_generator_group(&mut child)?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| std::io::Error::other("generator stderr reader panicked"))??;
    if status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&stderr).trim().to_owned();
    Err(DaloError::StateError {
        reason: if diagnostic.is_empty() {
            format!("generated delivery `{source_ref}` failed with {status}")
        } else {
            format!("generated delivery `{source_ref}` failed with {status}: {diagnostic}")
        },
    })
}

fn validate_and_audit_outputs(
    paths: &StorePaths,
    source_ref: &str,
    root: &Path,
    providers: &BTreeMap<String, PathBuf>,
    require_immutable: bool,
    persist_audit: bool,
) -> DaloResult<BTreeMap<String, PrebuiltSkillArtifact>> {
    validate_generated_tree(root, providers.values(), require_immutable)?;
    let mut artifacts = BTreeMap::new();
    for (provider, relative) in providers {
        let path = root.join(relative);
        if !path.join("SKILL.md").is_file() {
            return Err(DaloError::StateError {
                reason: format!(
                    "generated provider `{provider}` output `{}` does not contain SKILL.md",
                    relative.display()
                ),
            });
        }
        let fingerprint = crate::inventory::fingerprint_directory(&path).map_err(|reason| {
            DaloError::StateError {
                reason: format!("generated provider `{provider}` output is unsafe: {reason}"),
            }
        })?;
        let audit_ref = format!("{source_ref}@{provider}");
        let report = crate::audit::audit_skill(
            paths,
            &audit_ref,
            &path,
            &crate::audit::AuditOptions {
                persist: persist_audit,
                ..crate::audit::AuditOptions::default()
            },
        )?;
        if report.is_blocking() {
            return Err(DaloError::StateError {
                reason: format!(
                    "generated provider `{provider}` output for `{source_ref}` failed the security audit"
                ),
            });
        }
        artifacts.insert(
            provider.clone(),
            PrebuiltSkillArtifact { path, fingerprint },
        );
    }
    Ok(artifacts)
}

fn validate_generated_tree<'a>(
    root: &Path,
    outputs: impl Iterator<Item = &'a PathBuf>,
    require_immutable: bool,
) -> DaloResult<()> {
    let outputs = outputs.cloned().collect::<Vec<_>>();
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(DaloError::StateError {
            reason: "generated delivery root must be a real directory".to_owned(),
        });
    }
    if require_immutable && metadata.permissions().mode() & 0o222 != 0 {
        return Err(DaloError::StateError {
            reason: format!(
                "generated cache root `{}` is unexpectedly writable",
                root.display()
            ),
        });
    }
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("generated entry stays below its root");
            entries += 1;
            if entries > MAX_GENERATED_ENTRIES
                || relative.components().count() > MAX_GENERATED_DEPTH
            {
                return Err(DaloError::StateError {
                    reason: "generated delivery exceeds the bounded tree limits".to_owned(),
                });
            }
            if !outputs
                .iter()
                .any(|output| relative.starts_with(output) || output.starts_with(relative))
            {
                return Err(DaloError::StateError {
                    reason: format!(
                        "generator created undeclared output `{}`",
                        relative.display()
                    ),
                });
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(DaloError::StateError {
                    reason: format!("generator created symlink `{}`", relative.display()),
                });
            }
            if require_immutable && metadata.permissions().mode() & 0o222 != 0 {
                return Err(DaloError::StateError {
                    reason: format!(
                        "generated cache entry `{}` is unexpectedly writable",
                        relative.display()
                    ),
                });
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                if metadata.nlink() != 1 {
                    return Err(DaloError::StateError {
                        reason: format!(
                            "generator created multiply linked file `{}`",
                            relative.display()
                        ),
                    });
                }
                bytes = bytes.saturating_add(metadata.len());
                if bytes > MAX_GENERATED_BYTES {
                    return Err(DaloError::StateError {
                        reason: "generated delivery exceeds the 256 MiB size limit".to_owned(),
                    });
                }
            } else {
                return Err(DaloError::StateError {
                    reason: format!(
                        "generator created special filesystem entry `{}`",
                        relative.display()
                    ),
                });
            }
        }
    }
    Ok(())
}

fn copy_generated_tree(source: &Path, destination: &Path) -> DaloResult<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(DaloError::StateError {
                reason: format!(
                    "generator output changed to symlink `{}` while snapshotting",
                    source_path.display()
                ),
            });
        }
        if metadata.is_dir() {
            fs::create_dir(&destination_path)?;
            copy_generated_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            if metadata.nlink() != 1 {
                return Err(DaloError::StateError {
                    reason: format!(
                        "generator output became multiply linked `{}` while snapshotting",
                        source_path.display()
                    ),
                });
            }
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, metadata.permissions())?;
        } else {
            return Err(DaloError::StateError {
                reason: format!(
                    "generator output changed to a special entry `{}` while snapshotting",
                    source_path.display()
                ),
            });
        }
    }
    Ok(())
}

fn make_tree_read_only(root: &Path) -> DaloResult<()> {
    let mut directories = vec![root.to_path_buf()];
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                directories.push(path.clone());
                pending.push(path);
            } else {
                let executable = metadata.permissions().mode() & 0o111 != 0;
                fs::set_permissions(
                    &path,
                    fs::Permissions::from_mode(if executable { 0o555 } else { 0o444 }),
                )?;
            }
        }
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o555))?;
    }
    Ok(())
}

fn make_tree_writable(root: &Path) -> DaloResult<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
            }
        }
    }
    Ok(())
}

fn terminate_generator_group(child: &mut Child) -> DaloResult<()> {
    #[cfg(unix)]
    {
        let process_group = Pid::from_child(child);
        match kill_process_group(process_group, Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => {}
            Err(error) => {
                return Err(DaloError::StateError {
                    reason: format!(
                        "failed to terminate generated-delivery process group: {error}"
                    ),
                });
            }
        }
        let _ = child.wait();
        let deadline = Instant::now() + GENERATOR_SHUTDOWN_TIMEOUT;
        loop {
            match test_kill_process_group(process_group) {
                Err(rustix::io::Errno::SRCH) => return Ok(()),
                Ok(()) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(()) => {
                    return Err(DaloError::StateError {
                        reason: "generated-delivery process group survived termination; output was not audited or promoted"
                            .to_owned(),
                    });
                }
                Err(error) => {
                    return Err(DaloError::StateError {
                        reason: format!(
                            "failed to verify generated-delivery process-group termination: {error}"
                        ),
                    });
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        child.kill()?;
        child.wait()?;
        Ok(())
    }
}

fn terminate_generator(child: &mut Child) {
    if terminate_generator_group(child).is_ok() {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded(reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut reader = reader;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = (MAX_GENERATOR_STDERR as usize).saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(output)
}

#[cfg(test)]
mod generated_tree_tests {
    use super::*;

    #[test]
    fn generated_tree_should_reject_external_hard_link_aliases() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("stage");
        let provider = root.join("codex/review");
        fs::create_dir_all(&provider).unwrap();
        let external = temporary.path().join("external-skill.md");
        fs::write(&external, "# External\n").unwrap();
        fs::hard_link(&external, provider.join("SKILL.md")).unwrap();
        let outputs = [PathBuf::from("codex/review")];

        let error = validate_generated_tree(&root, outputs.iter(), false).unwrap_err();

        assert!(error.to_string().contains("multiply linked file"));
    }

    #[test]
    fn generated_snapshot_should_not_share_source_inodes() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        let snapshot = temporary.path().join("snapshot");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(source.join("SKILL.md"), "# Audited\n").unwrap();

        copy_generated_tree(&source, &snapshot).unwrap();
        fs::write(source.join("SKILL.md"), "# Mutated\n").unwrap();

        assert_eq!(
            fs::read_to_string(snapshot.join("SKILL.md")).unwrap(),
            "# Audited\n"
        );
        assert_ne!(
            fs::metadata(source.join("SKILL.md")).unwrap().ino(),
            fs::metadata(snapshot.join("SKILL.md")).unwrap().ino()
        );
    }
}

/// Result of granting or revoking one generated recipe approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeliveryApprovalReport {
    /// Canonical source-qualified logical skill.
    pub skill: String,
    /// Exact revision- and recipe-bound approval value.
    pub approval_value: String,
    /// Same-source generator tool identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    /// Exact generator invocation-contract hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator_contract_hash: Option<String>,
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
    validate_identity_shape(value)?;
    // Trust withdrawal must not depend on the current recipe remaining valid or
    // even present. Every generated approval persists both the grant-time slot
    // and stable ID, so either historical identity can withdraw stale trust.
    let current_identity = current_identity(paths, value);
    let skill = current_identity
        .as_ref()
        .map_or_else(|| value.to_owned(), |(canonical, _)| canonical.clone());
    let mut approvals = store::read_approvals(paths)?;
    let mut identities = vec![value.to_owned()];
    if let Some((canonical, approval_ref)) = &current_identity {
        for identity in [canonical, approval_ref] {
            if !identities.contains(identity) {
                identities.push(identity.clone());
            }
        }
    }
    let mut removed = None;
    approvals.approvals.retain(|record| {
        let matches = record.scope == APPROVAL_SCOPE
            && delivery_approval_aliases(&record.value).is_some_and(|(slot_ref, stable_ref)| {
                identities
                    .iter()
                    .any(|identity| identity == slot_ref || identity == stable_ref)
            });
        if matches {
            removed = Some(record.value.clone());
        }
        !matches
    });
    let changed = removed.is_some();
    if changed && !dry_run {
        store::write_approvals(paths, &approvals)?;
    }
    Ok(DeliveryApprovalReport {
        skill,
        approval_value: removed.unwrap_or_else(|| format!("{value}@")),
        generator: None,
        generator_contract_hash: None,
        providers: std::collections::BTreeMap::new(),
        action: if changed { "revoked" } else { "unchanged" }.to_owned(),
        dry_run,
        execution: "not_run".to_owned(),
    })
}

fn current_identity(paths: &StorePaths, value: &str) -> Option<(String, String)> {
    let (source_id, selector) = value.split_once(':')?;
    let config = store::read_config(paths).ok()?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == source_id)?;
    let inventory = crate::inventory::scan_source(source_id, &source.path).ok()?;
    let skill = inventory
        .skills
        .iter()
        .find(|skill| skill.slot_name == selector || skill.id.as_deref() == Some(selector))?;
    let stable_ref = format!("{}:{}", skill.source_id, skill.id.as_ref()?);
    Some((skill.source_ref.clone(), stable_ref))
}

fn delivery_approval_aliases(value: &str) -> Option<(&str, &str)> {
    let (slot_ref, remainder) = value.split_once("@id:")?;
    let (stable_ref, _) = remainder.split_once('@')?;
    Some((slot_ref, stable_ref))
}

fn validate_identity_shape(value: &str) -> DaloResult<()> {
    let valid = value.split_once(':').is_some_and(|(source, selector)| {
        !source.is_empty()
            && !selector.is_empty()
            && !source.contains(['@', '#', '/', '\\'])
            && !selector.contains(['@', '#', '/', '\\'])
    });
    if valid {
        Ok(())
    } else {
        Err(DaloError::InvalidArgument {
            reason: "generated delivery values must use `<source>:<slot>`".to_owned(),
        })
    }
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
        let stable_ref = format!(
            "{}:{}",
            skill.source_id,
            skill
                .id
                .as_ref()
                .expect("generated delivery requires a stable skill ID")
        );
        skill
            .delivery
            .bind_generated_approvals(&canonical, &stable_ref, Some(commit), &[]);
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
            .generated_approval_value(&canonical, &stable_ref)
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
        generator: Some(generator),
        generator_contract_hash: Some(generator_contract_hash),
        providers,
        action: String::new(),
        dry_run: false,
        execution: "not_run".to_owned(),
    })
}
