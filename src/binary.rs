//! Verified release binaries declared by skills, and their read-only inventory.
//!
//! A skill declares binaries in its `SKILL.md` frontmatter under `binaries`.
//! This module owns the declaration types, their validation, and the
//! content-bound contract hash, and joins each declaration with local approval,
//! staging, and exposure state for `dalo binary list|show`.
//!
//! Inspection never touches the network. The only path that downloads is
//! [`approve`], which fetches the host platform's asset, verifies its pinned
//! SHA-256 digest before anything is renamed into place, stages it read-only
//! under `binaries/<sha256>/`, links it at `bin/<id>`, and only then records
//! the exact approval. Dalo never executes the staged bytes.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::process;
#[cfg(test)]
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use ureq::ResponseExt;
use ureq::http::Uri;
use ureq::http::header::{CONTENT_LENGTH, LOCATION};

use crate::error::{DaloError, DaloResult};
use crate::inventory::{InventoryWarning, InventoryWarningCode, SkillRecord, SourceInventory};
use crate::plugin::{self, ToolAvailability};
use crate::source::{SourceConfig, SourceHeadCache, SourceProvenance};
use crate::store::{self, ApprovalRecord, StorePaths};
use crate::tool::make_directories_read_only;

/// Stable approval scope for exact release-binary contracts.
pub const APPROVAL_SCOPE: &str = "binary";

/// Largest release asset Dalo will download or accept as staged bytes.
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;

/// Global timeout for one download, including every redirect hop.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Redirects followed before a download is abandoned.
const MAX_REDIRECTS: u32 = 5;

/// Prefix of temporary download files, so `doctor` can find interrupted ones.
pub(crate) const DOWNLOAD_TEMPORARY_PREFIX: &str = ".binary-download-";

/// Test-only switch that lets the download policy accept plain-HTTP loopback
/// URLs, so unit tests can serve assets from a `127.0.0.1` fixture server.
///
/// It exists behind `cfg(test)`, so it is compiled out of the shipped binary
/// and is not part of the CLI surface, the same way `update.rs` overrides its
/// release endpoint for tests.
#[cfg(test)]
static TEST_ALLOW_INSECURE_LOOPBACK: Mutex<bool> = Mutex::new(false);

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
    /// Exposure link at `<store>/bin/<id>`, reported when it resolves to the staged file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposed_path: Option<PathBuf>,
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
    /// Approval exists, but the host asset has not been staged, or the verified bytes are not linked at `bin/<id>`.
    ApprovedNotStaged,
    /// The staged file no longer re-hashes to its pinned digest.
    AuditFailure,
    /// Exact approval, staged bytes that match the pinned digest, and the exposure link to them.
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
        // Provenance can cost a Git lookup, so it is resolved only for a source
        // that actually declares a binary; doctor and status pay nothing otherwise.
        let mut provenance: Option<SourceProvenance> = None;
        for skill in &inventory.skills {
            for binary in &skill.binaries {
                let provenance = provenance.get_or_insert_with(|| {
                    crate::source::source_provenance_with_head_cache(
                        source,
                        source_lock.as_ref(),
                        &mut head_cache,
                    )
                });
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
        exposed_path: evaluation.exposed_path,
        diagnostic: evaluation.diagnostic,
    }
}

struct Evaluation {
    state: BinaryState,
    staged_path: Option<PathBuf>,
    exposed_path: Option<PathBuf>,
    diagnostic: String,
}

/// Decide the readiness state from the declaration, approvals, staged bytes, and exposure link.
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
    let exposure = paths.bin_dir.join(&binary.id);
    let linked = staged_path
        .as_deref()
        .is_some_and(|path| link_resolves_to(&exposure, path));
    let exposed_path = linked.then(|| exposure.clone());
    let (state, diagnostic) = if host_asset.is_none() {
        let diagnostic = match host_platform {
            Some(platform) => format!("no asset is declared for host platform `{platform}`"),
            None => "this host is not one of the platforms a binary can declare".to_owned(),
        };
        (BinaryState::PlatformUnsupported, diagnostic)
    } else if exact_approval && staged && verified && linked {
        (
            BinaryState::Ready,
            "exact binary contract approved, the staged bytes match the pinned digest, and the exposure link points at them"
                .to_owned(),
        )
    } else if exact_approval && staged && verified {
        (
            BinaryState::ApprovedNotStaged,
            format!(
                "verified bytes are staged but `{}` is not linked to them; rerun `dalo approve binary {}`",
                exposure.display(),
                binary.source_ref
            ),
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
            format!(
                "exact binary contract has no approval; run `dalo approve binary {}`",
                binary.source_ref
            ),
        )
    };
    Evaluation {
        state,
        staged_path,
        exposed_path,
        diagnostic,
    }
}

/// Whether `link` is a symlink that resolves to `staged_path`.
fn link_resolves_to(link: &Path, staged_path: &Path) -> bool {
    fs::symlink_metadata(link).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && matches!(
            (fs::canonicalize(link), fs::canonicalize(staged_path)),
            (Ok(linked), Ok(staged)) if linked == staged
        )
}

/// Re-hash one staged file by streaming it, refusing anything that is not a regular file.
fn staged_bytes_match(path: &Path, expected_sha256: &str) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
        && fs::File::open(path)
            .and_then(|mut file| hash_reader(&mut file))
            .is_ok_and(|digest| digest == expected_sha256)
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

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// SHA-256 of everything `reader` yields, read in fixed-size chunks.
fn hash_reader(reader: &mut impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(hex_digest(hasher.finalize()))
}

/// Result of granting, planning, or revoking one exact release binary approval.
#[derive(Debug, Clone, Serialize)]
pub struct BinaryApprovalReport {
    /// Exact skill-qualified binary identity, `<source>:<skill>#binary:<id>`.
    pub binary: String,
    /// Exact content-bound approval value.
    pub approval_value: String,
    /// `planned` for a dry run, `granted`, `unchanged`, or `revoked`.
    pub action: String,
    /// Verified staged file for the host asset; for a dry run, where it would be staged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged_path: Option<PathBuf>,
    /// Exposure link at `<store>/bin/<id>`; for a dry run, where it would be created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposed_path: Option<PathBuf>,
    /// Whether mutation was suppressed.
    pub dry_run: bool,
}

/// Status of every binary one skill declares, for `approve skill` output.
pub(crate) fn skill_binary_statuses(
    paths: &StorePaths,
    skill: &SkillRecord,
    approvals: &[ApprovalRecord],
    provenance: &SourceProvenance,
) -> Vec<BinaryStatusReport> {
    let host_platform = host_platform();
    skill
        .binaries
        .iter()
        .map(|binary| {
            status_for(
                paths,
                binary.clone(),
                skill.source_ref.clone(),
                skill.path.clone(),
                provenance.clone(),
                approvals,
                host_platform,
            )
        })
        .collect()
}

/// Download, verify, stage, and expose the host asset, then grant the exact approval.
///
/// This is the only operation in this module that reaches the network. A dry
/// run reports the plan and touches nothing. A real run verifies the bytes
/// against the pinned digest before anything is renamed into place, so a
/// mismatch leaves neither a staged file nor an approval record.
pub fn approve(paths: &StorePaths, value: &str, dry_run: bool) -> DaloResult<BinaryApprovalReport> {
    let status = show(paths, value)?;
    let host_asset = status.host_platform.and_then(|platform| {
        status
            .binary
            .assets
            .get(&platform)
            .map(|asset| (platform, asset))
    });
    let Some((platform, asset)) = host_asset else {
        return Err(DaloError::StateError {
            reason: format!(
                "binary `{}` cannot be approved on this host: {}; nothing was downloaded",
                status.binary.source_ref, status.diagnostic
            ),
        });
    };
    let planned_staged = staged_root(paths, &asset.sha256).join(&status.binary.id);
    let planned_exposed = paths.bin_dir.join(&status.binary.id);
    let mut approvals = store::read_approvals(paths)?;
    let record = ApprovalRecord::granted(APPROVAL_SCOPE.to_owned(), status.approval_value.clone());
    let exists = approvals
        .approvals
        .iter()
        .any(|approval| approval.matches(&record));
    if dry_run {
        return Ok(BinaryApprovalReport {
            binary: status.binary.source_ref,
            approval_value: status.approval_value,
            action: "planned".to_owned(),
            staged_path: Some(planned_staged),
            exposed_path: Some(planned_exposed),
            dry_run,
        });
    }
    let staged_path = fetch_and_stage(paths, &status.binary, platform)?;
    let exposed_path = expose(paths, &status.binary, &staged_path)?;
    if !exists {
        approvals.approvals.push(record);
        approvals.approvals.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then(left.value.cmp(&right.value))
        });
        store::write_approvals(paths, &approvals)?;
    }
    Ok(BinaryApprovalReport {
        binary: status.binary.source_ref,
        approval_value: status.approval_value,
        action: if exists { "unchanged" } else { "granted" }.to_owned(),
        staged_path: Some(staged_path),
        exposed_path: Some(exposed_path),
        dry_run,
    })
}

/// Remove every approval for one exact binary identity.
///
/// Staged bytes and the exposure link stay in place; the state becomes
/// `revoked` until a later cleanup removes them.
pub fn revoke(paths: &StorePaths, value: &str, dry_run: bool) -> DaloResult<BinaryApprovalReport> {
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
    Ok(BinaryApprovalReport {
        binary: value.to_owned(),
        approval_value: removed.unwrap_or(prefix),
        action: if changed { "revoked" } else { "unchanged" }.to_owned(),
        staged_path: None,
        exposed_path: None,
        dry_run,
    })
}

/// Stage the verified host asset of one binary, downloading it only when needed.
///
/// Bytes already staged under the pinned digest are reused without any network
/// access, which also lets two skills that pin the same bytes share them. Otherwise
/// the asset is streamed into a temporary file in the store and renamed into
/// `binaries/<sha256>/<id>` only once its digest matches; a mismatch deletes the
/// temporary file and stages nothing.
pub(crate) fn fetch_and_stage(
    paths: &StorePaths,
    binary: &BinaryRecord,
    platform: BinaryPlatform,
) -> DaloResult<PathBuf> {
    let asset = binary
        .assets
        .get(&platform)
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: format!(
                "binary `{}` declares no asset for platform `{platform}`",
                binary.source_ref
            ),
        })?;
    let root = staged_root(paths, &asset.sha256);
    let destination = root.join(&binary.id);
    if staged_bytes_match(&destination, &asset.sha256) {
        return Ok(destination);
    }
    let temporary = download_verified(paths, binary, asset)?;
    // Another process may have staged the same digest while this one downloaded it.
    if staged_bytes_match(&destination, &asset.sha256) {
        return Ok(destination);
    }
    prepare_staging_directory(&root)?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o555))?;
    if let Err(error) = temporary.persist(&destination) {
        if staged_bytes_match(&destination, &asset.sha256) {
            return Ok(destination);
        }
        return Err(error.error.into());
    }
    make_directories_read_only(&root)?;
    Ok(destination)
}

/// Make `root` a writable directory so a verified file can be renamed into it.
///
/// A symlink or other non-directory at this path is refused rather than followed,
/// so permissions are never changed outside the store.
fn prepare_staging_directory(root: &Path) -> DaloResult<()> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::set_permissions(root, fs::Permissions::from_mode(0o755))?;
            Ok(())
        }
        Ok(_) => Err(DaloError::StateError {
            reason: format!(
                "content-addressed binary path `{}` is not a directory",
                root.display()
            ),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(root)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

/// Download the host asset into a temporary file in the store and verify its digest.
///
/// The temporary file is returned only when its digest matches the pin. Every
/// other outcome, including a policy refusal, a size refusal, or a digest
/// mismatch, drops it, which deletes it.
fn download_verified(
    paths: &StorePaths,
    binary: &BinaryRecord,
    asset: &BinaryAsset,
) -> DaloResult<NamedTempFile> {
    let mut response = request_asset(asset, &binary.source_ref)?;
    let declared = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if let Some(length) = declared.filter(|length| *length > MAX_BINARY_BYTES) {
        return Err(DaloError::StateError {
            reason: format!(
                "binary `{}` declares {length} bytes, above the 256 MiB limit; refusing to download it",
                binary.source_ref
            ),
        });
    }
    fs::create_dir_all(&paths.binaries_dir)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(DOWNLOAD_TEMPORARY_PREFIX)
        .tempfile_in(&paths.binaries_dir)?;
    // One byte past the limit, so an oversized body reaches the explicit check in
    // `stream_and_hash` instead of surfacing as a reader error.
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_BINARY_BYTES + 1)
        .reader();
    let downloaded = stream_and_hash(
        &mut reader,
        temporary.as_file_mut(),
        MAX_BINARY_BYTES,
        &binary.source_ref,
    )?;
    if downloaded != asset.sha256 {
        return Err(DaloError::StateError {
            reason: format!(
                "binary verification failed for `{}`: expected sha256:{}, downloaded sha256:{downloaded}",
                binary.source_ref, asset.sha256
            ),
        });
    }
    temporary.as_file().sync_all()?;
    Ok(temporary)
}

/// Copy `reader` into `writer` while hashing it, refusing more than `limit` bytes.
fn stream_and_hash(
    reader: &mut impl Read,
    writer: &mut impl Write,
    limit: u64,
    identity: &str,
) -> DaloResult<String> {
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(DaloError::Io(io::Error::other(format!(
                    "could not download the asset for `{identity}`: {error}"
                ))));
            }
        };
        total += read as u64;
        if total > limit {
            return Err(DaloError::StateError {
                reason: format!(
                    "binary `{identity}` is larger than {limit} bytes; refusing to stage it"
                ),
            });
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
    }
    Ok(hex_digest(hasher.finalize()))
}

/// Send the asset request, following at most [`MAX_REDIRECTS`] redirects.
///
/// Each target is checked before Dalo connects to it, so a redirect outside
/// GitHub is refused without a request reaching that host. No credentials are
/// ever attached. The final response URI is checked again before its body is read.
fn request_asset(
    asset: &BinaryAsset,
    identity: &str,
) -> DaloResult<ureq::http::Response<ureq::Body>> {
    let agent = download_agent();
    let mut url = asset.url.clone();
    let mut redirects = 0_u32;
    loop {
        let uri: Uri = url.parse().map_err(|_| DaloError::StateError {
            reason: format!("binary `{identity}` has an invalid download URL"),
        })?;
        check_download_uri(&uri, identity)?;
        let response = agent
            .get(url.as_str())
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|error| {
                DaloError::Io(io::Error::other(format!(
                    "could not download the asset for `{identity}`: {error}"
                )))
            })?;
        let status = response.status();
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            if redirects == MAX_REDIRECTS {
                return Err(DaloError::StateError {
                    reason: format!(
                        "binary download for `{identity}` followed more than {MAX_REDIRECTS} redirects"
                    ),
                });
            }
            redirects += 1;
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| DaloError::StateError {
                    reason: format!(
                        "binary download for `{identity}` redirected without a usable Location header"
                    ),
                })?;
            url = location.to_owned();
            continue;
        }
        if !status.is_success() {
            return Err(DaloError::StateError {
                reason: format!(
                    "binary download for `{identity}` returned HTTP status {}",
                    status.as_u16()
                ),
            });
        }
        check_download_uri(response.get_uri(), identity)?;
        return Ok(response);
    }
}

/// Refuse any download target that is not HTTPS on GitHub or its release CDN.
fn check_download_uri(uri: &Uri, identity: &str) -> DaloResult<()> {
    if loopback_http_allowed(uri) {
        return Ok(());
    }
    if uri.scheme_str() != Some("https") {
        return Err(DaloError::StateError {
            reason: format!("binary download for `{identity}` must use https; refusing `{uri}`"),
        });
    }
    let host = uri.host().unwrap_or_default().to_ascii_lowercase();
    if host == "github.com" || host.ends_with(".githubusercontent.com") {
        return Ok(());
    }
    Err(DaloError::StateError {
        reason: format!(
            "binary download for `{identity}` may only come from github.com or *.githubusercontent.com; refusing `{host}`"
        ),
    })
}

/// Whether a test may fetch a plain-HTTP loopback URL.
#[cfg(test)]
fn loopback_http_allowed(uri: &Uri) -> bool {
    uri.scheme_str() == Some("http")
        && uri.host() == Some("127.0.0.1")
        && *TEST_ALLOW_INSECURE_LOOPBACK
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
}

/// Production builds never accept a plain-HTTP download target.
#[cfg(not(test))]
fn loopback_http_allowed(_uri: &Uri) -> bool {
    false
}

/// HTTP agent for one download: a global timeout, no automatic redirects, and
/// Dalo's own user agent. Proxies follow `ureq`'s standard environment handling.
fn download_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .max_redirects(0)
            .max_redirects_will_error(false)
            .user_agent(concat!("dalo/", env!("CARGO_PKG_VERSION")))
            .build(),
    )
}

/// Expose one verified binary at `<store>/bin/<id>` as a relative symlink.
///
/// A link that already resolves to `staged_path` is left alone. A symlink into
/// `binaries/` for another digest, or a dangling one, is replaced atomically through
/// a temporary link. Anything else at that path (a real file, a directory, or a
/// foreign symlink) is refused and never removed.
pub(crate) fn expose(
    paths: &StorePaths,
    binary: &BinaryRecord,
    staged_path: &Path,
) -> DaloResult<PathBuf> {
    fs::create_dir_all(&paths.bin_dir)?;
    let link = paths.bin_dir.join(&binary.id);
    if link_resolves_to(&link, staged_path) {
        return Ok(link);
    }
    let staged_relative =
        staged_path
            .strip_prefix(&paths.root)
            .map_err(|_| DaloError::StateError {
                reason: format!(
                    "staged binary `{}` is outside the store",
                    staged_path.display()
                ),
            })?;
    let target = Path::new("..").join(staged_relative);
    match fs::symlink_metadata(&link) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                && link_targets_binaries(&link, &paths.binaries_dir) => {}
        Ok(_) => {
            return Err(DaloError::StateError {
                reason: format!(
                    "`{}` already exists and is not a Dalo link to a verified binary; refusing to replace it",
                    link.display()
                ),
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    replace_link(&link, &target, &binary.id)?;
    Ok(link)
}

/// Atomically point `link` at `target` by renaming a temporary symlink over it.
fn replace_link(link: &Path, target: &Path, id: &str) -> DaloResult<()> {
    let temporary = link.with_file_name(format!("{id}.tmp-{}", process::id()));
    if fs::symlink_metadata(&temporary).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        fs::remove_file(&temporary)?;
    }
    symlink(target, &temporary)?;
    if let Err(error) = fs::rename(&temporary, link) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

/// Whether the symlink at `link` lexically targets a path inside `binaries_dir`.
fn link_targets_binaries(link: &Path, binaries_dir: &Path) -> bool {
    let Ok(target) = fs::read_link(link) else {
        return false;
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        link.parent().unwrap_or(Path::new("")).join(target)
    };
    lexical_normalize(&resolved).starts_with(lexical_normalize(binaries_dir))
}

/// Resolve `.` and `..` without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
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
        let digest = sha256_hex(bytes);
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
        let unlinked = evaluate(&paths, &record, &approvals, host);
        assert_eq!(unlinked.state, BinaryState::ApprovedNotStaged);
        assert!(unlinked.exposed_path.is_none());
        assert!(
            unlinked
                .diagnostic
                .contains("is not linked to them; rerun `dalo approve binary team:page-engine#binary:impeccino`"),
            "{}",
            unlinked.diagnostic
        );

        let staged = staged_root(&paths, &digest).join("impeccino");
        expose(&paths, &record, &staged).unwrap();
        let ready = evaluate(&paths, &record, &approvals, host);
        assert_eq!(ready.state, BinaryState::Ready);
        assert_eq!(ready.staged_path, Some(staged));
        assert_eq!(ready.exposed_path, Some(paths.bin_dir.join("impeccino")));
    }

    /// Serializes the tests that toggle the loopback download override.
    static LOOPBACK_POLICY: Mutex<()> = Mutex::new(());

    /// Run `body` with plain-HTTP loopback downloads allowed or refused.
    fn with_loopback_policy<T>(allowed: bool, body: impl FnOnce() -> T) -> T {
        let _serial = LOOPBACK_POLICY
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *TEST_ALLOW_INSECURE_LOOPBACK
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = allowed;
        let result = body();
        *TEST_ALLOW_INSECURE_LOOPBACK
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = false;
        result
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        hex_digest(Sha256::digest(bytes))
    }

    /// A plain-HTTP fixture on `127.0.0.1` that answers each accepted connection
    /// with the next canned response, repeating the last one, and counts connections.
    struct Loopback {
        port: u16,
        accepted: Arc<AtomicUsize>,
    }

    impl Loopback {
        /// Start serving the responses that `responses` builds for this port.
        fn serve(responses: impl FnOnce(u16) -> Vec<Vec<u8>>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("loopback port should bind");
            let port = listener.local_addr().expect("local address").port();
            let responses = responses(port);
            let accepted = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&accepted);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let index = counter.fetch_add(1, Ordering::SeqCst);
                    read_request_head(&mut stream);
                    let response = &responses[index.min(responses.len() - 1)];
                    let _ = stream.write_all(response);
                }
            });
            Self { port, accepted }
        }

        fn url(&self, path: &str) -> String {
            format!("http://127.0.0.1:{}{path}", self.port)
        }

        fn connections(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }
    }

    fn read_request_head(stream: &mut TcpStream) {
        let mut seen = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !seen.windows(4).any(|window| window == b"\r\n\r\n") && seen.len() < 16 * 1024 {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => seen.extend_from_slice(&chunk[..read]),
            }
        }
    }

    /// One complete `Connection: close` HTTP/1.1 response.
    fn response(status: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
        let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn ok_with(body: &[u8]) -> Vec<u8> {
        response(
            "200 OK",
            &[("Content-Length", body.len().to_string())],
            body,
        )
    }

    /// A store whose read-only staging is made writable again on drop, so the
    /// temporary directory can be removed.
    struct TestStore {
        _temp: TempDir,
        paths: StorePaths,
    }

    impl TestStore {
        fn new() -> Self {
            let temp = TempDir::new().unwrap();
            let paths = StorePaths::new(temp.path().join("store"));
            fs::create_dir_all(&paths.root).unwrap();
            Self { _temp: temp, paths }
        }

        fn leftover_entries(&self) -> usize {
            fs::read_dir(&self.paths.binaries_dir).map_or(0, Iterator::count)
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            if let Ok(entries) = fs::read_dir(&self.paths.binaries_dir) {
                for entry in entries.flatten() {
                    let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o755));
                }
            }
            let _ =
                fs::set_permissions(&self.paths.binaries_dir, fs::Permissions::from_mode(0o755));
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// A linux-x64 declaration whose asset URL points at a test server.
    fn downloadable(url: &str, sha256: &str) -> BinaryRecord {
        let mut record = declaration(BinaryPlatform::LinuxX64, sha256);
        record
            .assets
            .get_mut(&BinaryPlatform::LinuxX64)
            .expect("linux-x64 asset")
            .url = url.to_owned();
        record
    }

    fn fetch(store: &TestStore, record: &BinaryRecord) -> DaloResult<PathBuf> {
        fetch_and_stage(&store.paths, record, BinaryPlatform::LinuxX64)
    }

    #[test]
    fn fetch_should_stage_a_matching_asset_read_only_at_its_digest_path() {
        let bytes = b"verified release bytes";
        let digest = sha256_hex(bytes);
        let server = Loopback::serve(|_| vec![ok_with(bytes)]);
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let staged = with_loopback_policy(true, || fetch(&store, &record)).unwrap();

        assert_eq!(staged, staged_root(&store.paths, &digest).join("impeccino"));
        assert_eq!(fs::read(&staged).unwrap(), bytes);
        assert_eq!(mode(&staged), 0o555);
        assert_eq!(mode(&staged_root(&store.paths, &digest)), 0o555);
        assert_eq!(server.connections(), 1);
        assert_eq!(store.leftover_entries(), 1, "only the digest directory");
    }

    #[test]
    fn a_second_fetch_reuses_the_staged_bytes_without_any_request() {
        let bytes = b"verified release bytes";
        let digest = sha256_hex(bytes);
        let server = Loopback::serve(|_| vec![ok_with(bytes)]);
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let first = with_loopback_policy(true, || fetch(&store, &record)).unwrap();
        let second = with_loopback_policy(true, || fetch(&store, &record)).unwrap();

        assert_eq!(first, second);
        assert_eq!(server.connections(), 1);
    }

    #[test]
    fn a_tampered_download_fails_verification_and_leaves_nothing_behind() {
        let digest = sha256_hex(b"the pinned release bytes");
        let server = Loopback::serve(|_| vec![ok_with(b"tampered bytes")]);
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let error = with_loopback_policy(true, || fetch(&store, &record)).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("binary verification failed"), "{message}");
        assert!(
            message.contains(&format!("expected sha256:{digest}")),
            "{message}"
        );
        assert!(!staged_root(&store.paths, &digest).exists());
        assert_eq!(
            store.leftover_entries(),
            0,
            "no temporary download may remain"
        );
    }

    #[test]
    fn a_content_length_above_the_limit_is_refused_before_the_body_is_read() {
        let digest = sha256_hex(b"unused");
        let oversized = (MAX_BINARY_BYTES + 1).to_string();
        let server = Loopback::serve(|_| {
            vec![response(
                "200 OK",
                &[("Content-Length", oversized.clone())],
                b"",
            )]
        });
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let error = with_loopback_policy(true, || fetch(&store, &record)).unwrap_err();

        assert!(
            error.to_string().contains("above the 256 MiB limit"),
            "{error}"
        );
        assert_eq!(store.leftover_entries(), 0);
    }

    #[test]
    fn a_body_past_the_size_limit_is_refused_while_streaming() {
        let mut input: &[u8] = b"0123456789";
        let mut sink = Vec::new();

        let error =
            stream_and_hash(&mut input, &mut sink, 4, "team:page-engine#binary:x").unwrap_err();

        assert!(error.to_string().contains("larger than 4 bytes"), "{error}");
        let mut input: &[u8] = b"0123456789";
        let digest = stream_and_hash(&mut input, &mut Vec::new(), 10, "x").unwrap();
        assert_eq!(digest, sha256_hex(b"0123456789"));
    }

    #[test]
    fn a_redirect_is_followed_only_to_an_allowed_target() {
        let bytes = b"verified release bytes";
        let digest = sha256_hex(bytes);
        let server = Loopback::serve(|port| {
            vec![
                response(
                    "302 Found",
                    &[("Location", format!("http://127.0.0.1:{port}/final"))],
                    b"",
                ),
                ok_with(bytes),
            ]
        });
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let staged = with_loopback_policy(true, || fetch(&store, &record)).unwrap();

        assert_eq!(fs::read(staged).unwrap(), bytes);
        assert_eq!(server.connections(), 2);
    }

    #[test]
    fn a_redirect_outside_github_is_refused_before_it_is_contacted() {
        let digest = sha256_hex(b"unused");
        let server = Loopback::serve(|_| {
            vec![response(
                "302 Found",
                &[("Location", "http://example.invalid/impeccino".to_owned())],
                b"",
            )]
        });
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let error = with_loopback_policy(true, || fetch(&store, &record)).unwrap_err();

        assert!(error.to_string().contains("example.invalid"), "{error}");
        assert_eq!(
            server.connections(),
            1,
            "the redirect target is never requested"
        );
        assert_eq!(store.leftover_entries(), 0);
    }

    #[test]
    fn a_plain_http_loopback_asset_is_refused_without_the_test_override() {
        let digest = sha256_hex(b"unused");
        let server = Loopback::serve(|_| vec![ok_with(b"unused")]);
        let store = TestStore::new();
        let record = downloadable(&server.url("/impeccino-linux-x64"), &digest);

        let error = with_loopback_policy(false, || fetch(&store, &record)).unwrap_err();

        assert!(error.to_string().contains("must use https"), "{error}");
        assert_eq!(server.connections(), 0);
    }

    #[test]
    fn download_policy_accepts_only_https_github_hosts() {
        let accepted = [
            "https://github.com/sebastian-software/impeccino/releases/download/v1/a",
            "https://objects.githubusercontent.com/a",
            "https://release-assets.githubusercontent.com/a",
        ];
        let refused = [
            "http://github.com/a",
            "https://api.github.com/a",
            "https://github.com.evil.example/a",
            "https://evil.githubusercontent.com.example/a",
            "https://evil.example/github.com",
            "https://user@evil.example/github.com",
        ];
        for url in accepted {
            let uri: Uri = url.parse().unwrap();
            assert!(check_download_uri(&uri, "x").is_ok(), "{url}");
        }
        for url in refused {
            let uri: Uri = url.parse().unwrap();
            assert!(check_download_uri(&uri, "x").is_err(), "{url}");
        }
    }

    #[test]
    fn expose_links_a_staged_binary_with_a_relative_target_and_is_idempotent() {
        let store = TestStore::new();
        let bytes = b"verified release bytes";
        let digest = sha256_hex(bytes);
        stage(&store.paths, &digest, bytes);
        let staged = staged_root(&store.paths, &digest).join("impeccino");
        let record = declaration(BinaryPlatform::LinuxX64, &digest);

        let link = expose(&store.paths, &record, &staged).unwrap();
        assert_eq!(link, store.paths.bin_dir.join("impeccino"));
        assert_eq!(
            fs::read_link(&link).unwrap(),
            PathBuf::from(format!("../binaries/{digest}/impeccino"))
        );
        expose(&store.paths, &record, &staged).unwrap();
        assert_eq!(fs::read(&link).unwrap(), bytes);
    }

    #[test]
    fn expose_replaces_a_stale_link_into_the_binaries_directory() {
        let store = TestStore::new();
        let old_digest = sha256_hex(b"old release bytes");
        let new_bytes = b"new release bytes";
        let new_digest = sha256_hex(new_bytes);
        stage(&store.paths, &old_digest, b"old release bytes");
        stage(&store.paths, &new_digest, new_bytes);
        let old_record = declaration(BinaryPlatform::LinuxX64, &old_digest);
        let new_record = declaration(BinaryPlatform::LinuxX64, &new_digest);
        expose(
            &store.paths,
            &old_record,
            &staged_root(&store.paths, &old_digest).join("impeccino"),
        )
        .unwrap();

        let link = expose(
            &store.paths,
            &new_record,
            &staged_root(&store.paths, &new_digest).join("impeccino"),
        )
        .unwrap();

        assert_eq!(
            fs::read_link(&link).unwrap(),
            PathBuf::from(format!("../binaries/{new_digest}/impeccino"))
        );
        assert_eq!(fs::read(&link).unwrap(), new_bytes);
        let leftovers = fs::read_dir(&store.paths.bin_dir).unwrap().count();
        assert_eq!(leftovers, 1, "the temporary link must not remain");
    }

    #[test]
    fn expose_refuses_a_real_file_or_a_foreign_link_and_never_removes_it() {
        let store = TestStore::new();
        let digest = sha256_hex(b"release bytes");
        stage(&store.paths, &digest, b"release bytes");
        let staged = staged_root(&store.paths, &digest).join("impeccino");
        let record = declaration(BinaryPlatform::LinuxX64, &digest);
        fs::create_dir_all(&store.paths.bin_dir).unwrap();
        let link = store.paths.bin_dir.join("impeccino");

        fs::write(&link, "user content").unwrap();
        let error = expose(&store.paths, &record, &staged).unwrap_err();
        assert!(
            error.to_string().contains("refusing to replace it"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&link).unwrap(), "user content");

        fs::remove_file(&link).unwrap();
        let foreign = store.paths.root.parent().unwrap().join("foreign-tool");
        symlink(&foreign, &link).unwrap();
        let error = expose(&store.paths, &record, &staged).unwrap_err();
        assert!(
            error.to_string().contains("refusing to replace it"),
            "{error}"
        );
        assert_eq!(fs::read_link(&link).unwrap(), foreign);
    }
}
