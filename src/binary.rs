//! Verified release binaries declared by skills, and their read-only inventory.
//!
//! A skill declares binaries in its `SKILL.md` frontmatter under `binaries`.
//! This module owns the declaration types, their validation, and the
//! content-bound contract hash, and joins each declaration with local approval
//! and staging state for `dalo binary list|show`. Nothing here downloads,
//! verifies, stages, or executes a binary; those steps belong to a later,
//! explicitly approved workflow.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::inventory::{InventoryWarning, InventoryWarningCode, SourceInventory};
use crate::plugin::{self, ToolAvailability};
use crate::source::{SourceConfig, SourceHeadCache, SourceProvenance};
use crate::store::{self, ApprovalRecord, StorePaths};

/// Stable approval scope for exact release-binary contracts.
pub const APPROVAL_SCOPE: &str = "binary";

/// One validated release-binary declaration. Inventory never downloads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryRecord {
    /// Binary ID, the key of the `binaries` frontmatter mapping.
    pub id: String,
    /// Skill-qualified identity, `<source>:<slot>#binary:<id>`.
    pub source_ref: String,
    /// Release source kind.
    pub source: BinarySource,
    /// GitHub repository, `<owner>/<name>`.
    pub repo: String,
    /// Release tag the assets are attached to.
    pub tag: String,
    /// Whether missing availability blocks the skill.
    pub availability: ToolAvailability,
    /// Declared asset for each platform, in platform order.
    pub assets: BTreeMap<BinaryPlatform, BinaryAsset>,
    /// Deterministic contract hash over the fields above; excludes provenance and derived URLs.
    pub contract_hash: String,
}

/// Supported release-binary sources. The enum is closed so each new kind is an explicit contract change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BinarySource {
    /// An asset attached to a GitHub release.
    #[serde(rename = "github-release")]
    GithubRelease,
}

impl BinarySource {
    /// Stable declaration label, identical to the serialized form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GithubRelease => "github-release",
        }
    }
}

/// Platforms a release binary may declare, serialized as `macos-arm64`, `macos-x64`, `linux-arm64`, or `linux-x64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BinaryPlatform {
    /// Apple Silicon macOS.
    #[serde(rename = "macos-arm64")]
    MacosArm64,
    /// Intel macOS.
    #[serde(rename = "macos-x64")]
    MacosX64,
    /// ARM64 Linux.
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
    /// x86-64 Linux.
    #[serde(rename = "linux-x64")]
    LinuxX64,
}

impl BinaryPlatform {
    /// Stable declaration label, identical to the serialized form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacosArm64 => "macos-arm64",
            Self::MacosX64 => "macos-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::LinuxX64 => "linux-x64",
        }
    }
}

impl std::fmt::Display for BinaryPlatform {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One platform asset of a release binary, with its pinned digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryAsset {
    /// Release asset file name.
    pub asset: String,
    /// Lowercase hexadecimal SHA-256 of the exact asset bytes.
    pub sha256: String,
    /// Derived download URL, `https://github.com/<repo>/releases/download/<tag>/<asset>`.
    pub url: String,
}

/// One declaration as authored, before identity and derived fields are added.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeclaration {
    source: BinarySource,
    repo: String,
    tag: String,
    #[serde(default = "default_availability")]
    availability: ToolAvailability,
    assets: BTreeMap<BinaryPlatform, RawAsset>,
}

/// One platform asset as authored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAsset {
    asset: String,
    sha256: String,
}

const fn default_availability() -> ToolAvailability {
    ToolAvailability::Required
}

/// Parse the `binaries` frontmatter value of one skill.
///
/// The value must be a mapping from binary ID to declaration, and an empty
/// mapping declares no binaries. Records are returned sorted by ID. The first
/// invalid declaration fails the whole value with a message that names its ID
/// and the rule it breaks. `skill_source_ref` is the owning skill's
/// `<source>:<slot>` reference, which prefixes every binary identity.
pub(crate) fn parse_declarations(
    skill_source_ref: &str,
    value: &yaml_serde::Value,
) -> Result<Vec<BinaryRecord>, String> {
    let yaml_serde::Value::Mapping(entries) = value else {
        return Err(
            "`binaries` must be a mapping from binary id to declaration; use `binaries: {}` for none"
                .to_owned(),
        );
    };
    let mut records = Vec::with_capacity(entries.len());
    for (key, declaration) in entries {
        let yaml_serde::Value::String(id) = key else {
            return Err("binary ids must be strings".to_owned());
        };
        if !plugin::is_plugin_name(id) {
            return Err(format!("binary `{id}`: the id must use lower kebab-case"));
        }
        let raw: RawDeclaration = yaml_serde::from_value(declaration.clone())
            .map_err(|error| format!("binary `{id}`: {error}"))?;
        records.push(build_record(skill_source_ref, id, raw)?);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

fn build_record(
    skill_source_ref: &str,
    id: &str,
    raw: RawDeclaration,
) -> Result<BinaryRecord, String> {
    let rule = |message: String| format!("binary `{id}`: {message}");
    let RawDeclaration {
        source,
        repo,
        tag,
        availability,
        assets: raw_assets,
    } = raw;
    if !is_github_repo(&repo) {
        return Err(rule(
            "repo must be `<owner>/<name>` with 1-100 character segments that do not start with `.`"
                .to_owned(),
        ));
    }
    if !is_github_tag(&tag) {
        return Err(rule(
            "tag must be 1-128 characters from letters, digits, `.`, `_`, `/`, and `-`, must not start with `-`, `.`, or `/` or end with `/`, and must not contain `..` or `//`"
                .to_owned(),
        ));
    }
    if raw_assets.is_empty() {
        return Err(rule("assets must declare at least one platform".to_owned()));
    }
    let mut assets = BTreeMap::new();
    for (platform, asset) in raw_assets {
        if !is_binary_asset_name(&asset.asset) {
            return Err(rule(format!(
                "asset for {platform} must be 1-255 characters from letters, digits, `.`, `_`, and `-`, must not start with `.`, and must not contain `..`"
            )));
        }
        if !is_sha256_hex(&asset.sha256) {
            return Err(rule(format!(
                "asset for {platform} must pin a 64-character lower-case SHA-256 digest"
            )));
        }
        let url = format!(
            "https://github.com/{repo}/releases/download/{tag}/{}",
            asset.asset
        );
        assets.insert(
            platform,
            BinaryAsset {
                asset: asset.asset,
                sha256: asset.sha256,
                url,
            },
        );
    }
    let mut record = BinaryRecord {
        id: id.to_owned(),
        source_ref: format!("{skill_source_ref}#binary:{id}"),
        source,
        repo,
        tag,
        availability,
        assets,
        contract_hash: String::new(),
    };
    record.contract_hash = hash_binary_contract(&record);
    Ok(record)
}

/// `<owner>/<name>`: two segments of 1-100 characters, none starting with `.`.
/// The leading-dot rule also excludes `.` and `..`.
fn is_github_repo(value: &str) -> bool {
    let mut segments = value.split('/');
    let (Some(owner), Some(name), None) = (segments.next(), segments.next(), segments.next())
    else {
        return false;
    };
    [owner, name].into_iter().all(|segment| {
        (1..=100).contains(&segment.len())
            && !segment.starts_with('.')
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
}

/// A release tag may not start with `-`, `.`, or `/`, may not end with `/`,
/// and may contain neither `..` nor `//`, so a derived URL never gets an empty
/// path segment.
fn is_github_tag(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && !value.starts_with(['-', '.', '/'])
        && !value.ends_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
}

fn is_binary_asset_name(value: &str) -> bool {
    (1..=255).contains(&value.len())
        && !value.starts_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn hash_binary_contract(binary: &BinaryRecord) -> String {
    let mut hash = Sha256::new();
    hash.update(b"dalo-binary-contract-v1\0");
    plugin::hash_contract_value(&mut hash, &format!("id:{}", binary.id));
    plugin::hash_contract_value(&mut hash, &format!("source:{}", binary.source.as_str()));
    plugin::hash_contract_value(&mut hash, &format!("repo:{}", binary.repo));
    plugin::hash_contract_value(&mut hash, &format!("tag:{}", binary.tag));
    plugin::hash_contract_value(&mut hash, &format!("availability:{}", binary.availability));
    for (platform, asset) in &binary.assets {
        plugin::hash_contract_value(&mut hash, &format!("platform:{platform}"));
        plugin::hash_contract_value(&mut hash, &format!("asset:{}", asset.asset));
        plugin::hash_contract_value(&mut hash, &format!("sha256:{}", asset.sha256));
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Complete read-only inventory and state report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BinaryListReport {
    /// Validated declarations in deterministic identity order.
    pub binaries: Vec<BinaryStatusReport>,
    /// Invalid `binaries` declarations found while scanning enabled sources.
    pub warnings: Vec<InventoryWarning>,
}

/// One declared binary joined with the host platform and local trust state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BinaryStatusReport {
    /// Validated declaration and contract hash.
    pub binary: BinaryRecord,
    /// Source-qualified ref of the skill that declares the binary.
    pub skill_source_ref: String,
    /// Directory of the declaring skill, retained only as provenance.
    pub skill_path: PathBuf,
    /// Source revision and origin provenance, excluded from approval identity.
    pub source_provenance: SourceProvenance,
    /// Exact approval value for this contract.
    pub approval_value: String,
    /// Platform of the running Dalo binary, or `None` when it is not a declarable platform.
    pub host_platform: Option<BinaryPlatform>,
    /// Current readiness state.
    pub state: BinaryState,
    /// Staged file for the host asset, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged_path: Option<PathBuf>,
    /// Actionable explanation.
    pub diagnostic: String,
}

/// Mutually exclusive release-binary readiness states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryState {
    /// The running host has no declared asset.
    PlatformUnsupported,
    /// No approval exists for this exact contract.
    PendingApproval,
    /// An approval exists for this identity, but the current contract hash differs.
    HashDrift,
    /// Verified bytes remain staged but the exact approval was revoked.
    Revoked,
    /// Approval exists, but the host asset has not been staged.
    ApprovedNotStaged,
    /// The staged file no longer re-hashes to its pinned digest.
    AuditFailure,
    /// Exact approval and the staged bytes both match the pinned digest.
    Ready,
}

impl std::fmt::Display for BinaryState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::PlatformUnsupported => "platform_unsupported",
            Self::PendingApproval => "pending_approval",
            Self::HashDrift => "hash_drift",
            Self::Revoked => "revoked",
            Self::ApprovedNotStaged => "approved_not_staged",
            Self::AuditFailure => "audit_failure",
            Self::Ready => "ready",
        })
    }
}

/// Collect every valid binary declaration without downloading or executing it.
pub fn list(paths: &StorePaths) -> DaloResult<BinaryListReport> {
    let config = store::read_config(paths)?;
    let approvals = store::read_approvals(paths)?;
    let inventories = scan_skill_inventories(&config.sources);
    Ok(list_from_inventories(
        paths,
        &config.sources,
        &approvals.approvals,
        &inventories,
    ))
}

/// Scan the skills of every enabled source. A source whose scan fails reports
/// nothing, because skill discovery is read-only and never executes code.
fn scan_skill_inventories(sources: &[SourceConfig]) -> Vec<SourceInventory> {
    sources
        .iter()
        .filter(|source| source.enabled)
        .filter_map(|source| {
            let root =
                crate::source::scoped_source_root(&source.path, source.subpath.as_deref()).ok()?;
            crate::inventory::scan_source(&source.id, &root).ok()
        })
        .collect()
}

/// Join already-scanned skill inventories with local binary trust and staging state.
#[must_use]
pub fn list_from_inventories(
    paths: &StorePaths,
    sources: &[SourceConfig],
    approvals: &[ApprovalRecord],
    inventories: &[SourceInventory],
) -> BinaryListReport {
    let source_lock = crate::catalog::read_source_lock(paths).ok();
    let mut head_cache = SourceHeadCache::default();
    let host_platform = host_platform();
    let mut binaries = Vec::new();
    let mut warnings = Vec::new();
    for inventory in inventories {
        let Some(source) = sources
            .iter()
            .find(|source| source.enabled && source.id == inventory.source_id)
        else {
            continue;
        };
        warnings.extend(
            inventory
                .warnings
                .iter()
                .filter(|warning| warning.code == InventoryWarningCode::InvalidBinaryDeclaration)
                .cloned(),
        );
        let provenance = crate::source::source_provenance_with_head_cache(
            source,
            source_lock.as_ref(),
            &mut head_cache,
        );
        for skill in &inventory.skills {
            for binary in &skill.binaries {
                binaries.push(status_for(
                    paths,
                    binary.clone(),
                    skill.source_ref.clone(),
                    skill.path.clone(),
                    provenance.clone(),
                    approvals,
                    host_platform,
                ));
            }
        }
    }
    binaries.sort_by(|left, right| left.binary.source_ref.cmp(&right.binary.source_ref));
    warnings.sort_by(|left, right| left.path.cmp(&right.path));
    BinaryListReport { binaries, warnings }
}

/// Find one exact skill-qualified binary declaration.
pub fn show(paths: &StorePaths, value: &str) -> DaloResult<BinaryStatusReport> {
    validate_identity_shape(value)?;
    let report = list(paths)?;
    report
        .binaries
        .into_iter()
        .find(|candidate| candidate.binary.source_ref == value)
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: format!(
                "unknown binary `{value}`; use `dalo binary list` and an exact `<source>:<skill>#binary:<id>` identity"
            ),
        })
}

/// Directory of immutable verified bytes for one pinned SHA-256 digest.
#[must_use]
pub fn staged_root(paths: &StorePaths, digest: &str) -> PathBuf {
    paths.binaries_dir.join(digest)
}

/// Platform of the running Dalo binary, when it is one of the declarable platforms.
#[must_use]
pub fn host_platform() -> Option<BinaryPlatform> {
    host_platform_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn host_platform_for(os: &str, arch: &str) -> Option<BinaryPlatform> {
    match (os, arch) {
        ("macos", "aarch64") => Some(BinaryPlatform::MacosArm64),
        ("macos", "x86_64") => Some(BinaryPlatform::MacosX64),
        ("linux", "aarch64") => Some(BinaryPlatform::LinuxArm64),
        ("linux", "x86_64") => Some(BinaryPlatform::LinuxX64),
        _ => None,
    }
}

fn status_for(
    paths: &StorePaths,
    binary: BinaryRecord,
    skill_source_ref: String,
    skill_path: PathBuf,
    source_provenance: SourceProvenance,
    approvals: &[ApprovalRecord],
    host_platform: Option<BinaryPlatform>,
) -> BinaryStatusReport {
    let evaluation = evaluate(paths, &binary, approvals, host_platform);
    BinaryStatusReport {
        approval_value: approval_value(&binary),
        binary,
        skill_source_ref,
        skill_path,
        source_provenance,
        host_platform,
        state: evaluation.state,
        staged_path: evaluation.staged_path,
        diagnostic: evaluation.diagnostic,
    }
}

struct Evaluation {
    state: BinaryState,
    staged_path: Option<PathBuf>,
    diagnostic: String,
}

/// Decide the readiness state from the declaration, approvals, and staged bytes.
fn evaluate(
    paths: &StorePaths,
    binary: &BinaryRecord,
    approvals: &[ApprovalRecord],
    host_platform: Option<BinaryPlatform>,
) -> Evaluation {
    let exact_value = approval_value(binary);
    let exact_approval = approvals
        .iter()
        .any(|record| record.scope == APPROVAL_SCOPE && record.value == exact_value);
    let identity_prefix = format!("{}@sha256:", binary.source_ref);
    let prior_approval = approvals
        .iter()
        .any(|record| record.scope == APPROVAL_SCOPE && record.value.starts_with(&identity_prefix));
    let host_asset = host_platform.and_then(|platform| binary.assets.get(&platform));
    // A symlink or other non-regular entry counts as staged so that it is
    // reported as an audit failure instead of silently treated as absent.
    let staged_path = host_asset
        .map(|asset| staged_root(paths, &asset.sha256).join(&binary.id))
        .filter(|path| fs::symlink_metadata(path).is_ok());
    let staged = staged_path.is_some();
    let verified = match (host_asset, &staged_path) {
        (Some(asset), Some(path)) => staged_bytes_match(path, &asset.sha256),
        _ => false,
    };
    let (state, diagnostic) = if host_asset.is_none() {
        let diagnostic = match host_platform {
            Some(platform) => format!("no asset is declared for host platform `{platform}`"),
            None => "this host is not one of the platforms a binary can declare".to_owned(),
        };
        (BinaryState::PlatformUnsupported, diagnostic)
    } else if exact_approval && staged && verified {
        (
            BinaryState::Ready,
            "exact binary contract approved and the staged bytes match the pinned digest"
                .to_owned(),
        )
    } else if exact_approval && staged {
        (
            BinaryState::AuditFailure,
            "staged binary bytes do not match the pinned digest".to_owned(),
        )
    } else if exact_approval {
        (
            BinaryState::ApprovedNotStaged,
            "exact binary contract approved but the host asset is not staged".to_owned(),
        )
    } else if staged {
        (
            BinaryState::Revoked,
            "staged bytes exist but no exact binary approval remains".to_owned(),
        )
    } else if prior_approval {
        (
            BinaryState::HashDrift,
            "binary contract changed since its last approval and requires reapproval".to_owned(),
        )
    } else {
        (
            BinaryState::PendingApproval,
            "exact binary contract has no approval; this release only inventories declarations"
                .to_owned(),
        )
    };
    Evaluation {
        state,
        staged_path,
        diagnostic,
    }
}

/// Re-hash one staged file, refusing anything that is not a regular file.
fn staged_bytes_match(path: &Path, expected_sha256: &str) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
        && fs::read(path).is_ok_and(|bytes| hash_bytes(&bytes) == expected_sha256)
}

fn approval_value(binary: &BinaryRecord) -> String {
    format!("{}@sha256:{}", binary.source_ref, binary.contract_hash)
}

fn validate_identity_shape(value: &str) -> DaloResult<()> {
    let valid = value
        .split_once("#binary:")
        .is_some_and(|(skill, id)| skill.contains(':') && !id.is_empty());
    if valid {
        Ok(())
    } else {
        Err(DaloError::InvalidArgument {
            reason: format!(
                "invalid binary identity `{value}`: binary values must use `<source>:<skill>#binary:<id>`"
            ),
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
    use tempfile::TempDir;

    const SKILL_REF: &str = "team:page-engine";
    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// The `binaries` mapping of one valid declaration.
    const MAPPING: &str = "impeccino:
  source: github-release
  repo: sebastian-software/impeccino
  tag: engine-v0.2.0
  availability: required
  assets:
    macos-arm64:
      asset: impeccino-darwin-arm64
      sha256: \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"
    linux-x64:
      asset: impeccino-linux-x64
      sha256: \"fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210\"
";

    fn parse(text: &str) -> Result<Vec<BinaryRecord>, String> {
        let value: yaml_serde::Value = yaml_serde::from_str(text).expect("test YAML should parse");
        parse_declarations(SKILL_REF, &value)
    }

    /// A whole `SKILL.md` frontmatter with the given description and digest.
    fn frontmatter(description: &str, digest: &str) -> String {
        format!(
            "name: page-engine
description: {description}
binaries:
  impeccino:
    source: github-release
    repo: sebastian-software/impeccino
    tag: engine-v0.2.0
    availability: required
    assets:
      macos-arm64:
        asset: impeccino-darwin-arm64
        sha256: \"{digest}\"
      linux-x64:
        asset: impeccino-linux-x64
        sha256: \"fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210\"
"
        )
    }

    /// The binary records declared by one frontmatter document.
    fn declared(document: &str) -> Vec<BinaryRecord> {
        let mapping: yaml_serde::Mapping =
            yaml_serde::from_str(document).expect("frontmatter should parse");
        let value = mapping.get("binaries").expect("binaries key should exist");
        parse_declarations(SKILL_REF, value).expect("declaration should be valid")
    }

    #[test]
    fn valid_declaration_derives_identity_urls_and_a_pinned_contract_hash() {
        let records = parse(MAPPING).expect("declaration should be valid");
        assert_eq!(records.len(), 1);
        let binary = &records[0];
        assert_eq!(binary.id, "impeccino");
        assert_eq!(binary.source_ref, "team:page-engine#binary:impeccino");
        assert_eq!(binary.source, BinarySource::GithubRelease);
        assert_eq!(binary.availability, ToolAvailability::Required);
        assert_eq!(
            binary.assets.keys().copied().collect::<Vec<_>>(),
            [BinaryPlatform::MacosArm64, BinaryPlatform::LinuxX64]
        );
        assert_eq!(
            binary.assets[&BinaryPlatform::MacosArm64].url,
            "https://github.com/sebastian-software/impeccino/releases/download/engine-v0.2.0/impeccino-darwin-arm64"
        );
        assert_eq!(
            binary.contract_hash,
            "d651c9b99dadd5d94b9dd94e39b3d8c8d72efbc900d4b81641afd9b2e89db4c1"
        );
    }

    #[test]
    fn changing_one_asset_digest_changes_the_contract_hash() {
        let first = declared(&frontmatter("Audit pages", DIGEST))[0]
            .contract_hash
            .clone();
        let second = declared(&frontmatter(
            "Audit pages",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ))[0]
            .contract_hash
            .clone();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
    }

    #[test]
    fn a_description_only_change_keeps_the_contract_hash() {
        let before = declared(&frontmatter("Audit pages", DIGEST));
        let after = declared(&frontmatter("Audit rendered pages with the engine", DIGEST));
        assert_eq!(before[0].contract_hash, after[0].contract_hash);
    }

    #[test]
    fn records_are_sorted_by_id_and_availability_defaults_to_required() {
        let text = format!(
            "zeta:\n  source: github-release\n  repo: a/zeta\n  tag: v1\n  assets:\n    linux-x64:\n      asset: zeta\n      sha256: \"{DIGEST}\"\n{MAPPING}"
        );
        let records = parse(&text).expect("both declarations should be valid");
        let ids = records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["impeccino", "zeta"]);
        assert_eq!(records[1].availability, ToolAvailability::Required);
    }

    #[test]
    fn empty_mapping_declares_no_binaries() {
        assert_eq!(parse("{}"), Ok(Vec::new()));
    }

    #[test]
    fn rejects_each_declaration_rule_with_the_binary_id_in_the_message() {
        let cases: Vec<(&str, String)> = vec![
            ("bad id key", MAPPING.replace("impeccino:", "Impeccino:")),
            (
                "unknown source",
                MAPPING.replace("github-release", "http-download"),
            ),
            (
                "single-segment repo",
                MAPPING.replace("sebastian-software/impeccino", "sebastian-software"),
            ),
            (
                "dot-dot repo segment",
                MAPPING.replace("sebastian-software/impeccino", "sebastian-software/.."),
            ),
            (
                "tag starting with a dash",
                MAPPING.replace("engine-v0.2.0", "-engine"),
            ),
            (
                "tag with a leading slash",
                MAPPING.replace("engine-v0.2.0", "/v1"),
            ),
            (
                "tag with a trailing slash",
                MAPPING.replace("engine-v0.2.0", "v1/"),
            ),
            (
                "tag of a single slash",
                MAPPING.replace("engine-v0.2.0", "/"),
            ),
            (
                "unknown availability",
                MAPPING.replace("availability: required", "availability: always"),
            ),
            (
                "empty assets",
                MAPPING
                    .split("  assets:")
                    .next()
                    .expect("prefix")
                    .to_owned()
                    + "  assets: {}\n",
            ),
            (
                "unknown platform key",
                MAPPING.replace("macos-arm64", "windows-x64"),
            ),
            (
                "path-shaped asset name",
                MAPPING.replace("impeccino-linux-x64", "../impeccino"),
            ),
            (
                "63-character digest",
                MAPPING.replace(DIGEST, &DIGEST[..63]),
            ),
            (
                "unknown field",
                MAPPING.replace(
                    "  availability: required\n",
                    "  availability: required\n  expires: 1\n",
                ),
            ),
            (
                "unknown asset field",
                MAPPING.replace(
                    "asset: impeccino-linux-x64\n",
                    "asset: impeccino-linux-x64\n      expires: 1\n",
                ),
            ),
        ];
        for (label, text) in cases {
            let error = parse(&text).expect_err(label);
            assert!(error.starts_with("binary `"), "{label}: {error}");
        }
    }

    #[test]
    fn rejects_a_binaries_value_that_is_not_a_mapping() {
        for text in ["impeccino", "- impeccino", "~"] {
            assert!(parse(text).is_err(), "{text:?} must not be accepted");
        }
    }

    #[test]
    fn duplicate_binary_ids_are_a_yaml_error_before_validation() {
        let text = format!("impeccino:\n  tag: a\n{MAPPING}");
        assert!(yaml_serde::from_str::<yaml_serde::Value>(&text).is_err());
    }

    #[test]
    fn tag_rules_reject_empty_path_segments_at_both_ends() {
        for tag in ["/v1", "v1/", "/", "a//b", "a..b", "-x", ".x"] {
            assert!(!is_github_tag(tag), "{tag}");
        }
        for tag in ["engine-v0.2.0", "release/v1", "_x"] {
            assert!(is_github_tag(tag), "{tag}");
        }
    }

    #[test]
    fn host_platform_maps_only_the_declarable_os_and_architecture_pairs() {
        assert_eq!(
            host_platform_for("macos", "aarch64"),
            Some(BinaryPlatform::MacosArm64)
        );
        assert_eq!(
            host_platform_for("macos", "x86_64"),
            Some(BinaryPlatform::MacosX64)
        );
        assert_eq!(
            host_platform_for("linux", "aarch64"),
            Some(BinaryPlatform::LinuxArm64)
        );
        assert_eq!(
            host_platform_for("linux", "x86_64"),
            Some(BinaryPlatform::LinuxX64)
        );
        assert_eq!(host_platform_for("windows", "x86_64"), None);
        assert_eq!(host_platform_for("linux", "riscv64"), None);
    }

    #[test]
    fn identity_shape_requires_the_binary_namespace() {
        assert!(validate_identity_shape("team:page-engine#binary:impeccino").is_ok());
        for value in [
            "team:page-engine#tool:impeccino",
            "page-engine#binary:impeccino",
            "team:page-engine#binary:",
        ] {
            assert!(validate_identity_shape(value).is_err(), "{value}");
        }
    }

    const CONTRACT: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn declaration(platform: BinaryPlatform, sha256: &str) -> BinaryRecord {
        BinaryRecord {
            id: "impeccino".to_owned(),
            source_ref: "team:page-engine#binary:impeccino".to_owned(),
            source: BinarySource::GithubRelease,
            repo: "sebastian-software/impeccino".to_owned(),
            tag: "engine-v0.2.0".to_owned(),
            availability: ToolAvailability::Required,
            assets: BTreeMap::from([(
                platform,
                BinaryAsset {
                    asset: "impeccino-bin".to_owned(),
                    sha256: sha256.to_owned(),
                    url: "https://github.com/sebastian-software/impeccino/releases/download/engine-v0.2.0/impeccino-bin".to_owned(),
                },
            )]),
            contract_hash: CONTRACT.to_owned(),
        }
    }

    fn exact_approval() -> ApprovalRecord {
        ApprovalRecord {
            scope: APPROVAL_SCOPE.to_owned(),
            value: format!("team:page-engine#binary:impeccino@sha256:{CONTRACT}"),
            granted_at_unix: None,
        }
    }

    fn stage(paths: &StorePaths, digest: &str, bytes: &[u8]) {
        let directory = staged_root(paths, digest);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("impeccino"), bytes).unwrap();
    }

    #[test]
    fn undeclared_host_is_platform_unsupported_with_or_without_a_host() {
        let temp = TempDir::new().unwrap();
        let paths = StorePaths::new(temp.path().join("store"));
        let record = declaration(BinaryPlatform::LinuxX64, &"aa".repeat(32));

        let mismatched = evaluate(
            &paths,
            &record,
            &[exact_approval()],
            Some(BinaryPlatform::MacosArm64),
        );
        assert_eq!(mismatched.state, BinaryState::PlatformUnsupported);
        let unknown = evaluate(&paths, &record, &[], None);
        assert_eq!(unknown.state, BinaryState::PlatformUnsupported);
        assert!(unknown.staged_path.is_none());
    }

    #[test]
    fn undecided_contract_is_pending_and_a_changed_contract_drifts() {
        let temp = TempDir::new().unwrap();
        let paths = StorePaths::new(temp.path().join("store"));
        let record = declaration(BinaryPlatform::LinuxX64, &"aa".repeat(32));

        assert_eq!(
            evaluate(&paths, &record, &[], Some(BinaryPlatform::LinuxX64)).state,
            BinaryState::PendingApproval
        );
        let prior = ApprovalRecord {
            scope: APPROVAL_SCOPE.to_owned(),
            value: format!(
                "team:page-engine#binary:impeccino@sha256:{}",
                "00".repeat(32)
            ),
            granted_at_unix: None,
        };
        assert_eq!(
            evaluate(&paths, &record, &[prior], Some(BinaryPlatform::LinuxX64)).state,
            BinaryState::HashDrift
        );
    }

    #[test]
    fn approval_and_staged_bytes_decide_between_ready_audit_and_revocation() {
        let temp = TempDir::new().unwrap();
        let paths = StorePaths::new(temp.path().join("store"));
        let bytes = b"verified release bytes";
        let digest = hash_bytes(bytes);
        let record = declaration(BinaryPlatform::LinuxX64, &digest);
        let host = Some(BinaryPlatform::LinuxX64);
        let approvals = [exact_approval()];

        assert_eq!(
            evaluate(&paths, &record, &approvals, host).state,
            BinaryState::ApprovedNotStaged
        );

        stage(&paths, &digest, b"tampered");
        assert_eq!(
            evaluate(&paths, &record, &approvals, host).state,
            BinaryState::AuditFailure
        );
        assert_eq!(
            evaluate(&paths, &record, &[], host).state,
            BinaryState::Revoked
        );

        stage(&paths, &digest, bytes);
        let ready = evaluate(&paths, &record, &approvals, host);
        assert_eq!(ready.state, BinaryState::Ready);
        assert_eq!(
            ready.staged_path,
            Some(staged_root(&paths, &digest).join("impeccino"))
        );
    }
}
