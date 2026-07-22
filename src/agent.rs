//! Portable canonical agent packages and provider compilers.
//!
//! This module implements the read-only foundation of RFC 0004.  It discovers
//! only canonical `agents/<name>/AGENT.md` packages, validates their portable
//! contract, hashes their complete package tree, and deterministically compiles
//! safe Claude and Codex projections.  Materializing generated files is kept
//! separate so an incomplete compiler can never overwrite a provider file.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::inventory::SourceInventory;
use crate::source::{SourceConfig, SourceKind};
use crate::store::ApprovalRecord;

/// Canonical package entry-point filename.
pub const AGENT_FILE: &str = "AGENT.md";

const MAX_AGENT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;
const MAX_PACKAGE_ENTRIES: usize = 256;
const MAX_PACKAGE_DEPTH: usize = 16;
const MAX_PACKAGE_BYTES: u64 = 16 * 1024 * 1024;

type PackageFiles = Vec<(String, Vec<u8>)>;

/// One scan result for canonical packages below a source.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentInventory {
    /// Valid canonical agent packages.
    pub agents: Vec<AgentRecord>,
    /// Non-fatal malformed-package findings.
    pub warnings: Vec<AgentInventoryWarning>,
}

/// A valid canonical agent discovered in a source.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRecord {
    /// Source containing the package.
    pub source_id: String,
    /// Source-qualified reference, `<source-id>:<name>`.
    pub source_ref: String,
    /// Portable slot name.
    pub slot_name: String,
    /// Stable optional identity.
    pub id: Option<String>,
    /// Package directory.
    pub path: PathBuf,
    /// Canonical entry point.
    pub agent_file: PathBuf,
    /// Human-facing summary.
    pub description: String,
    /// Declared owners.
    pub owners: Vec<String>,
    /// Declared tags.
    pub tags: Vec<String>,
    /// Provider allowlist; `None` means every compatible provider, while an
    /// explicit empty vector targets no provider.
    pub targets: Option<Vec<String>>,
    /// Required skill references.
    pub skills: Vec<String>,
    /// Portable canonical document.
    pub agent: CanonicalAgent,
    /// SHA-256 of the complete canonical package tree.
    pub content_hash: String,
    /// Whether support files are present in addition to `AGENT.md`.
    pub has_support_files: bool,
}

/// A non-fatal canonical-package inventory warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentInventoryWarning {
    /// Stable machine-readable warning code.
    pub code: AgentInventoryWarningCode,
    /// Package or entry path.
    pub path: PathBuf,
    /// Actionable detail.
    pub message: String,
}

/// Canonical-package inventory warning codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInventoryWarningCode {
    /// The package has an invalid document or violates a safety bound.
    InvalidPackage,
    /// The package path is a symlink or contains an unsupported special file.
    UnsafePackageEntry,
    /// Two packages in one source use the same canonical name.
    DuplicateSlotName,
    /// A package directory could not be read.
    UnreadablePath,
}

impl AgentInventoryWarningCode {
    /// Stable snake-case label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidPackage => "invalid_agent_package",
            Self::UnsafePackageEntry => "unsafe_agent_package_entry",
            Self::DuplicateSlotName => "duplicate_agent_slot_name",
            Self::UnreadablePath => "unreadable_agent_path",
        }
    }
}

impl std::fmt::Display for AgentInventoryWarningCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A canonical, provider-neutral agent document.
#[derive(Debug, Clone, Serialize)]
pub struct CanonicalAgent {
    /// Version of the canonical schema.
    pub schema_version: u32,
    /// Portable agent slot name.
    pub name: String,
    /// Human-facing description.
    pub description: String,
    /// Stable optional identity.
    pub id: Option<String>,
    /// Declared owners.
    pub owners: Vec<String>,
    /// Declared tags.
    pub tags: Vec<String>,
    /// Provider allowlist. Omitted in source means every compatible provider.
    pub targets: Option<Vec<String>>,
    /// Model intent independent from provider model IDs.
    pub model: ModelIntent,
    /// Required skill references.
    pub skills: Vec<String>,
    /// Optional portable hard tool boundary.
    pub tools: Option<ToolBoundary>,
    /// Optional portable hard filesystem boundary.
    pub filesystem: Option<FilesystemBoundary>,
    /// Optional portable hard network boundary.
    pub network: Option<NetworkBoundary>,
    /// Provider-specific overlays retained with the canonical source.
    #[serde(serialize_with = "serialize_redacted_providers")]
    pub providers: BTreeMap<String, serde_json::Value>,
    /// Canonical Markdown system prompt.
    pub prompt: String,
}

/// Model intent used by canonical agents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIntent {
    /// Provider-neutral preference.
    #[serde(default)]
    pub profile: ModelProfile,
}

/// Provider-neutral model preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProfile {
    /// Keep the provider default model and reasoning effort.
    #[default]
    Inherit,
    /// Prefer a faster provider model or lower reasoning effort.
    Fast,
    /// Prefer the provider's balanced option.
    Balanced,
    /// Prefer deeper reasoning.
    Deep,
}

/// An explicit portable tool allowlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBoundary {
    /// Allowed portable tool capabilities.  An empty vector denies all tools.
    pub allow: Vec<ToolCapability>,
}

/// Portable tool capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolCapability {
    /// Read files.
    ReadFiles,
    /// Write or edit files.
    WriteFiles,
    /// Search files.
    SearchFiles,
    /// Run a shell command.
    RunShell,
    /// Fetch a known web resource.
    FetchWeb,
    /// Search the web.
    SearchWeb,
    /// Use configured MCP tools.
    UseMcp,
    /// Delegate to another agent.
    Delegate,
}

impl ToolCapability {
    fn claude_tools(self) -> &'static [&'static str] {
        match self {
            Self::ReadFiles => &["Read"],
            Self::WriteFiles => &["Edit", "Write"],
            Self::SearchFiles => &["Glob", "Grep"],
            Self::RunShell => &["Bash"],
            Self::FetchWeb => &["WebFetch"],
            Self::SearchWeb => &["WebSearch"],
            Self::UseMcp => &[],
            Self::Delegate => &["Task"],
        }
    }
}

/// An explicit portable filesystem boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemBoundary {
    /// Readable `workspace` scopes.
    #[serde(default)]
    pub read: Vec<String>,
    /// Writable `workspace` scopes.
    #[serde(default)]
    pub write: Vec<String>,
}

/// An explicit portable network boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkBoundary {
    /// Allowed exact host names, or the lone wildcard `*`.
    pub allow: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentFrontmatter {
    schema_version: u32,
    name: String,
    description: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    owners: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    targets: Option<Vec<String>>,
    #[serde(default)]
    model: ModelIntent,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    tools: Option<ToolBoundary>,
    #[serde(default)]
    filesystem: Option<FilesystemBoundary>,
    #[serde(default)]
    network: Option<NetworkBoundary>,
    #[serde(default)]
    providers: BTreeMap<String, serde_json::Value>,
}

/// Scan only `agents/<name>/AGENT.md` packages in one source.
#[must_use]
pub fn scan_source_agents(source_id: &str, source_root: &Path) -> AgentInventory {
    let agents_root = source_root.join("agents");
    match fs::symlink_metadata(&agents_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return AgentInventory::default();
        }
        Err(error) => {
            return AgentInventory {
                agents: Vec::new(),
                warnings: vec![warning(
                    AgentInventoryWarningCode::UnreadablePath,
                    agents_root,
                    error.to_string(),
                )],
            };
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return AgentInventory {
                agents: Vec::new(),
                warnings: vec![warning(
                    AgentInventoryWarningCode::UnsafePackageEntry,
                    agents_root,
                    "the source `agents` directory must not be a symlink".to_owned(),
                )],
            };
        }
        Ok(metadata) if !metadata.is_dir() => {
            return AgentInventory {
                agents: Vec::new(),
                warnings: vec![warning(
                    AgentInventoryWarningCode::InvalidPackage,
                    agents_root,
                    "the source `agents` path must be a directory".to_owned(),
                )],
            };
        }
        Ok(_) => {}
    }

    let entries = match fs::read_dir(&agents_root) {
        Ok(entries) => entries,
        Err(error) => {
            return AgentInventory {
                agents: Vec::new(),
                warnings: vec![warning(
                    AgentInventoryWarningCode::UnreadablePath,
                    agents_root,
                    error.to_string(),
                )],
            };
        }
    };

    let mut packages = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(warning(
                    AgentInventoryWarningCode::UnreadablePath,
                    agents_root.clone(),
                    error.to_string(),
                ));
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                warnings.push(warning(
                    AgentInventoryWarningCode::UnreadablePath,
                    path,
                    error.to_string(),
                ));
                continue;
            }
        };
        if file_type.is_symlink() {
            warnings.push(warning(
                AgentInventoryWarningCode::UnsafePackageEntry,
                path,
                "agent package directories must not be symlinks".to_owned(),
            ));
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        if path.join(AGENT_FILE).exists() {
            packages.push(path);
        }
    }
    packages.sort();

    let mut agents = Vec::new();
    for package_path in packages {
        match scan_package(source_id, &package_path) {
            Ok(agent) => agents.push(agent),
            Err(error) => warnings.push(warning(
                if error.contains("symlink") || error.contains("special file") {
                    AgentInventoryWarningCode::UnsafePackageEntry
                } else {
                    AgentInventoryWarningCode::InvalidPackage
                },
                package_path,
                error,
            )),
        }
    }

    let mut paths_by_slot = BTreeMap::<String, Vec<PathBuf>>::new();
    for agent in &agents {
        paths_by_slot
            .entry(agent.slot_name.clone())
            .or_default()
            .push(agent.path.clone());
    }
    let duplicated = paths_by_slot
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect::<Vec<_>>();
    for (slot_name, paths) in duplicated {
        for path in paths {
            warnings.push(warning(
                AgentInventoryWarningCode::DuplicateSlotName,
                path,
                format!("source `{source_id}` contains multiple agents named `{slot_name}`"),
            ));
        }
        agents.retain(|agent| agent.slot_name != slot_name);
    }
    agents.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });

    AgentInventory { agents, warnings }
}

fn warning(
    code: AgentInventoryWarningCode,
    path: PathBuf,
    message: String,
) -> AgentInventoryWarning {
    AgentInventoryWarning {
        code,
        path,
        message,
    }
}

fn scan_package(source_id: &str, package_path: &Path) -> Result<AgentRecord, String> {
    let package_name = package_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "agent package directory name must be valid UTF-8".to_owned())?;
    let (entries, total_bytes) = collect_package_files(package_path)?;
    let content_hash = hash_package_files(&entries);
    let agent_file = package_path.join(AGENT_FILE);
    let agent_bytes = entries
        .iter()
        .find_map(|(path, bytes)| (path == AGENT_FILE).then_some(bytes.as_slice()))
        .ok_or_else(|| format!("package is missing {AGENT_FILE}"))?;
    if agent_bytes.len() as u64 > MAX_AGENT_FILE_BYTES {
        return Err(format!(
            "{AGENT_FILE} exceeds the {MAX_AGENT_FILE_BYTES}-byte safety limit"
        ));
    }
    let document = std::str::from_utf8(agent_bytes)
        .map_err(|_| format!("{AGENT_FILE} must be valid UTF-8"))?;
    let agent = parse_agent_document(document, package_name)?;

    Ok(AgentRecord {
        source_id: source_id.to_owned(),
        source_ref: format!("{source_id}:{}", agent.name),
        slot_name: agent.name.clone(),
        id: agent.id.clone(),
        path: package_path.to_path_buf(),
        agent_file,
        description: agent.description.clone(),
        owners: agent.owners.clone(),
        tags: agent.tags.clone(),
        targets: agent.targets.clone(),
        skills: agent.skills.clone(),
        agent,
        content_hash,
        has_support_files: entries.len() > 1 || total_bytes > agent_bytes.len() as u64,
    })
}

fn collect_package_files(package_path: &Path) -> Result<(PackageFiles, u64), String> {
    let mut entries = Vec::new();
    let mut entry_count = 0;
    let mut total_bytes = 0_u64;
    collect_package_files_inner(
        package_path,
        package_path,
        0,
        &mut entries,
        &mut entry_count,
        &mut total_bytes,
    )?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok((entries, total_bytes))
}

fn collect_package_files_inner(
    package_root: &Path,
    current: &Path,
    depth: usize,
    files: &mut PackageFiles,
    entry_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), String> {
    if depth > MAX_PACKAGE_DEPTH {
        return Err(format!(
            "package exceeds the {MAX_PACKAGE_DEPTH}-directory-level safety limit"
        ));
    }
    let entries = fs::read_dir(current).map_err(|error| error.to_string())?;
    let mut entries = entries
        .map(|entry| entry.map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        *entry_count += 1;
        if *entry_count > MAX_PACKAGE_ENTRIES {
            return Err(format!(
                "package exceeds the {MAX_PACKAGE_ENTRIES}-entry safety limit"
            ));
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err(format!("package contains symlink `{}`", path.display()));
        }
        if file_type.is_dir() {
            collect_package_files_inner(
                package_root,
                &path,
                depth + 1,
                files,
                entry_count,
                total_bytes,
            )?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!(
                "package contains unsupported special file `{}`",
                path.display()
            ));
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        *total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| "package byte count overflowed".to_owned())?;
        if *total_bytes > MAX_PACKAGE_BYTES {
            return Err(format!(
                "package exceeds the {MAX_PACKAGE_BYTES}-byte safety limit"
            ));
        }
        files.push((relative_package_path(package_root, &path)?, bytes));
    }
    Ok(())
}

fn relative_package_path(package_root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(package_root)
        .map_err(|_| "package entry escaped its package root".to_owned())?;
    let mut parts = Vec::new();
    for part in relative.components() {
        let std::path::Component::Normal(part) = part else {
            return Err("package contains a non-normal path component".to_owned());
        };
        parts.push(
            part.to_str()
                .ok_or_else(|| "package entry path must be valid UTF-8".to_owned())?,
        );
    }
    Ok(parts.join("/"))
}

fn hash_package_files(entries: &PackageFiles) -> String {
    let mut hash = Sha256::new();
    hash.update(b"dalo-agent-package-v1\0");
    for (path, bytes) in entries {
        hash.update((path.len() as u64).to_be_bytes());
        hash.update(path.as_bytes());
        hash.update(*b"f");
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse_agent_document(document: &str, package_name: &str) -> Result<CanonicalAgent, String> {
    let rest = document
        .strip_prefix("---\n")
        .or_else(|| document.strip_prefix("---\r\n"))
        .ok_or_else(|| format!("{AGENT_FILE} must begin with an exact `---` frontmatter line"))?;
    let (frontmatter, prompt) = split_frontmatter(rest)?;
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        return Err(format!(
            "frontmatter exceeds the {MAX_FRONTMATTER_BYTES}-byte safety limit"
        ));
    }
    reject_unsafe_yaml(frontmatter)?;
    let parsed: AgentFrontmatter = yaml_serde::from_str(frontmatter)
        .map_err(|error| format!("invalid YAML frontmatter: {error}"))?;
    if parsed.schema_version != 1 {
        return Err(format!(
            "unsupported agent schema_version {}; expected 1",
            parsed.schema_version
        ));
    }
    if parsed.name != package_name {
        return Err(format!(
            "frontmatter name `{}` must exactly match package directory `{package_name}`",
            parsed.name
        ));
    }
    if !is_valid_agent_name(&parsed.name) {
        return Err(format!(
            "agent name `{}` is not a portable agent slot name",
            parsed.name
        ));
    }
    if parsed.description.trim().is_empty() {
        return Err("agent description must not be empty".to_owned());
    }
    if prompt.trim().is_empty() {
        return Err("agent Markdown prompt must contain non-whitespace text".to_owned());
    }
    validate_frontmatter(&parsed)?;
    Ok(CanonicalAgent {
        schema_version: parsed.schema_version,
        name: parsed.name,
        description: parsed.description,
        id: parsed.id,
        owners: parsed.owners,
        tags: parsed.tags,
        targets: parsed.targets,
        model: parsed.model,
        skills: parsed.skills,
        tools: parsed.tools,
        filesystem: parsed.filesystem,
        network: parsed.network,
        providers: parsed.providers,
        prompt: prompt.to_owned(),
    })
}

fn split_frontmatter(rest: &str) -> Result<(&str, &str), String> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let bare = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or(line.strip_suffix('\n').unwrap_or(line));
        if bare == "---" {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err("frontmatter start marker has no matching closing `---` marker".to_owned())
}

fn reject_unsafe_yaml(frontmatter: &str) -> Result<(), String> {
    // `yaml_serde` handles syntax and duplicate map keys.  These lexical guards
    // reject features whose expansion/indirection semantics do not belong in a
    // portable declarative package before deserializing into owned data.
    for line in frontmatter.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<<:") {
            return Err("YAML merge keys are not allowed".to_owned());
        }
        if contains_unquoted_yaml_token(line, '&') {
            return Err("YAML anchors are not allowed".to_owned());
        }
        if contains_unquoted_yaml_token(line, '*') {
            return Err("YAML aliases are not allowed".to_owned());
        }
        if contains_unquoted_yaml_token(line, '!') {
            return Err("YAML custom tags are not allowed".to_owned());
        }
    }
    Ok(())
}

fn contains_unquoted_yaml_token(line: &str, token: char) -> bool {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if double {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                double = false;
            }
            continue;
        }
        if single {
            if character == '\'' {
                single = false;
            }
            continue;
        }
        match character {
            '\'' => single = true,
            '"' => double = true,
            '#' => break,
            _ if character == token && is_yaml_indicator_position(line, index) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn is_yaml_indicator_position(line: &str, index: usize) -> bool {
    let prefix = line[..index].trim_end();
    if prefix.is_empty()
        || prefix.ends_with(':')
        || prefix.ends_with('[')
        || prefix.ends_with('{')
        || prefix.ends_with(',')
    {
        return true;
    }
    prefix
        .strip_suffix('-')
        .is_some_and(|before_dash| before_dash.trim().is_empty())
}

fn serialize_redacted_providers<S>(
    providers: &BTreeMap<String, serde_json::Value>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let redacted = providers
        .iter()
        .map(|(provider, value)| (provider.clone(), redact_provider_value(value, None)))
        .collect::<BTreeMap<_, _>>();
    redacted.serialize(serializer)
}

fn redact_provider_value(value: &serde_json::Value, key: Option<&str>) -> serde_json::Value {
    if key.is_some_and(is_secret_like_key) {
        return serde_json::Value::String("[REDACTED]".to_owned());
    }
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        redact_provider_value(value, Some(key.as_str())),
                    )
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .iter()
                .map(|value| redact_provider_value(value, None))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn is_secret_like_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    normalized.contains("secret")
        || normalized.contains("token")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("apikey")
        || normalized.contains("privatekey")
        || normalized.contains("accesskey")
        || normalized == "auth"
        || normalized.contains("authorization")
}

fn validate_frontmatter(frontmatter: &AgentFrontmatter) -> Result<(), String> {
    if frontmatter
        .id
        .as_deref()
        .is_some_and(|id| id.trim().is_empty())
    {
        return Err("agent id must not be empty when specified".to_owned());
    }
    validate_strings("owners", &frontmatter.owners)?;
    validate_strings("tags", &frontmatter.tags)?;
    validate_strings("skills", &frontmatter.skills)?;
    if let Some(targets) = &frontmatter.targets {
        validate_strings("targets", targets)?;
        for target in targets {
            if !is_valid_agent_name(target) {
                return Err(format!("target `{target}` is not a valid provider ID"));
            }
        }
    }
    if let Some(tools) = &frontmatter.tools {
        let mut unique = BTreeSet::new();
        for tool in &tools.allow {
            if !unique.insert(*tool) {
                return Err("tools.allow must not contain duplicates".to_owned());
            }
        }
    }
    if let Some(filesystem) = &frontmatter.filesystem {
        validate_filesystem_scopes("filesystem.read", &filesystem.read)?;
        validate_filesystem_scopes("filesystem.write", &filesystem.write)?;
    }
    if let Some(network) = &frontmatter.network {
        if network
            .allow
            .iter()
            .filter(|host| host.as_str() == "*")
            .count()
            > 0
            && network.allow.len() != 1
        {
            return Err("network.allow may use `*` only as its lone value".to_owned());
        }
        for host in &network.allow {
            if host != "*" && !is_valid_hostname(host) {
                return Err(format!(
                    "network host `{host}` is not a valid exact host name"
                ));
            }
        }
    }
    for (provider, overlay) in &frontmatter.providers {
        if !is_valid_agent_name(provider) {
            return Err(format!(
                "provider overlay `{provider}` is not a valid provider ID"
            ));
        }
        let object = overlay
            .as_object()
            .ok_or_else(|| format!("providers.{provider} must be a mapping"))?;
        if object.keys().any(|key| {
            matches!(
                key.as_str(),
                "name" | "description" | "prompt" | "developer_instructions" | "skills"
            )
        }) {
            return Err(format!(
                "providers.{provider} may not replace canonical identity, prompt, or skill fields"
            ));
        }
    }
    Ok(())
}

fn validate_strings(field: &str, values: &[String]) -> Result<(), String> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(format!("{field} must not contain empty values"));
    }
    Ok(())
}

fn validate_filesystem_scopes(field: &str, scopes: &[String]) -> Result<(), String> {
    for scope in scopes {
        let valid = scope == "workspace"
            || scope.strip_prefix("workspace/").is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix.split('/').all(|part| {
                        !part.is_empty() && part != "." && part != ".." && !part.contains('\\')
                    })
            });
        if !valid {
            return Err(format!(
                "{field} scope `{scope}` must be `workspace` or a normalized path below it"
            ));
        }
    }
    Ok(())
}

fn is_valid_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

fn is_valid_agent_name(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

/// Provider adapters implemented by this RFC's initial slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    /// Anthropic Claude Code's global subagent directory.
    Claude,
    /// OpenAI Codex's global subagent directory.
    Codex,
}

impl AgentProvider {
    /// Parse a provider ID.
    pub fn parse(value: &str) -> DaloResult<Self> {
        match value {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            _ => Err(DaloError::InvalidArgument {
                reason: format!("unsupported agent provider `{value}`; expected claude or codex"),
            }),
        }
    }

    /// Canonical provider ID.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Generated native filename for an agent slot.
    #[must_use]
    pub fn filename(self, name: &str) -> String {
        match self {
            Self::Claude => format!("{name}.md"),
            Self::Codex => format!("{name}.toml"),
        }
    }
}

/// Field-level portability outcome, ordered by severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityResult {
    /// Native output retains the field directly.
    Exact,
    /// Native output enforces the same value through a deterministic mapping.
    Mapped,
    /// The provider receives instructions but cannot enforce the field.
    GuidanceOnly,
    /// The provider does not consume this non-safety field.
    Unsupported,
    /// The requested projection would be unsafe or invalid.
    Blocked,
}

/// One field-level provider compatibility finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompatibilityFinding {
    /// Canonical field path.
    pub field: String,
    /// Translation fidelity.
    pub result: CompatibilityResult,
    /// Explanation and remediation where relevant.
    pub message: String,
    /// Native value or field mapping when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_mapping: Option<String>,
}

/// Result of compiling one canonical agent for one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCompilation {
    /// Destination provider.
    pub provider: AgentProvider,
    /// Whether this provider is excluded by the canonical `targets` allowlist.
    pub not_targeted: bool,
    /// Complete generated native preview when safe to materialize.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>,
    /// Ordered field-level compatibility report.
    pub findings: Vec<CompatibilityFinding>,
    /// Greatest finding severity.
    pub overall: CompatibilityResult,
}

/// One agent selected, pending approval, or shadowed by deterministic source
/// precedence.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedAgent {
    /// Canonical package record.
    #[serde(flatten)]
    pub agent: AgentRecord,
    /// Source kind used for agent approval semantics.
    pub source_kind: SourceKind,
    /// Source precedence; smaller values win.
    pub source_priority: i32,
}

/// A selected agent that lost its global slot to another selected agent.
#[derive(Debug, Clone, Serialize)]
pub struct ShadowedAgent {
    /// Agent that was not selected.
    pub agent: ResolvedAgent,
    /// Source-qualified selected winner.
    pub shadowed_by: String,
}

/// Deterministic agent resolution independent from skill resolution.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentResolution {
    /// Selected global winners, one per agent slot.
    pub active_agents: Vec<ResolvedAgent>,
    /// Candidates requiring an explicit agent-scoped approval.
    pub pending_approval_agents: Vec<ResolvedAgent>,
    /// Approved candidates shadowed by selected global winners.
    pub shadowed_agents: Vec<ShadowedAgent>,
}

/// Read-only report used by `dalo agent list`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentListReport {
    /// Agent resolution across currently enabled sources.
    pub resolution: AgentResolution,
    /// Invalid package findings retained from source inventories.
    pub inventory_warnings: Vec<AgentInventoryWarning>,
    /// Sources whose inventory could not be read.
    pub source_errors: Vec<String>,
}

/// Read-only report used by `dalo agent show`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentShowReport {
    /// Canonical package selected by its source-qualified reference.
    pub agent: AgentRecord,
    /// Deterministic provider previews. They never write target files.
    pub compilations: Vec<AgentCompilation>,
}

#[derive(Debug, Clone)]
struct ResolutionCandidate {
    agent: ResolvedAgent,
}

/// Resolve agent slots from source inventories.
///
/// Existing skill, source, author, and organization approvals never match an
/// agent.  Local agents are immediately eligible; every other source needs an
/// explicit `scope = "agent"` record whose value is the source-qualified slot
/// or stable ID.
#[must_use]
pub fn resolve_agents(
    sources: &[SourceConfig],
    inventories: &[SourceInventory],
    approvals: &[ApprovalRecord],
) -> AgentResolution {
    let sources_by_id = sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| (source.id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = Vec::new();
    for inventory in inventories {
        let Some(source) = sources_by_id.get(inventory.source_id.as_str()) else {
            continue;
        };
        for record in &inventory.agents {
            candidates.push(ResolutionCandidate {
                agent: ResolvedAgent {
                    agent: record.clone(),
                    source_kind: source.kind,
                    source_priority: source.priority,
                },
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.agent
            .source_priority
            .cmp(&right.agent.source_priority)
            .then_with(|| left.agent.agent.source_id.cmp(&right.agent.agent.source_id))
            .then_with(|| {
                left.agent
                    .agent
                    .source_ref
                    .cmp(&right.agent.agent.source_ref)
            })
    });

    let mut groups = BTreeMap::<String, Vec<ResolutionCandidate>>::new();
    for candidate in candidates {
        groups
            .entry(candidate.agent.agent.slot_name.clone())
            .or_default()
            .push(candidate);
    }
    let mut resolution = AgentResolution::default();
    for group in groups.into_values() {
        for candidate in &group {
            if !agent_is_approved(&candidate.agent, approvals) {
                resolution
                    .pending_approval_agents
                    .push(candidate.agent.clone());
            }
        }
        let Some(winner_index) = group
            .iter()
            .position(|candidate| agent_is_approved(&candidate.agent, approvals))
        else {
            continue;
        };
        let winner = group[winner_index].agent.clone();
        for candidate in group.iter().skip(winner_index + 1) {
            if agent_is_approved(&candidate.agent, approvals) {
                resolution.shadowed_agents.push(ShadowedAgent {
                    agent: candidate.agent.clone(),
                    shadowed_by: winner.agent.source_ref.clone(),
                });
            }
        }
        resolution.active_agents.push(winner);
    }
    resolution
        .active_agents
        .sort_by(|left, right| left.agent.source_ref.cmp(&right.agent.source_ref));
    resolution
        .pending_approval_agents
        .sort_by(|left, right| left.agent.source_ref.cmp(&right.agent.source_ref));
    resolution.shadowed_agents.sort_by(|left, right| {
        left.agent
            .agent
            .source_ref
            .cmp(&right.agent.agent.source_ref)
    });
    resolution
}

fn agent_is_approved(candidate: &ResolvedAgent, approvals: &[ApprovalRecord]) -> bool {
    candidate.source_kind == SourceKind::Local
        || approvals.iter().any(|approval| {
            approval.scope == "agent"
                && (approval.value == candidate.agent.source_ref
                    || candidate.agent.id.as_ref().is_some_and(|id| {
                        approval.value == format!("{}:{id}", candidate.agent.source_id)
                    }))
        })
}

/// Find a canonical agent by a source-qualified slot or stable ID.
pub fn find_agent(
    sources: &[SourceConfig],
    inventories: &[SourceInventory],
    reference: &str,
) -> DaloResult<AgentRecord> {
    let (source_id, selector) = reference
        .split_once(':')
        .filter(|(source_id, selector)| !source_id.is_empty() && !selector.is_empty())
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: "agent references must use `<source>:<name>`, for example `local:reviewer`"
                .to_owned(),
        })?;
    if !sources.iter().any(|source| source.id == source_id) {
        return Err(DaloError::unknown_source(
            source_id,
            sources.iter().map(|source| source.id.clone()).collect(),
        ));
    }
    let candidates = inventories
        .iter()
        .find(|inventory| inventory.source_id == source_id)
        .map(|inventory| {
            inventory
                .agents
                .iter()
                .filter(|agent| {
                    agent.slot_name == selector || agent.id.as_deref() == Some(selector)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match candidates.as_slice() {
        [agent] => Ok(agent.clone()),
        [] => Err(DaloError::InvalidArgument {
            reason: format!(
                "agent `{reference}` was not found; use `dalo agent list` to inspect discovered packages"
            ),
        }),
        _ => Err(DaloError::InvalidArgument {
            reason: format!("agent reference `{reference}` is ambiguous"),
        }),
    }
}

/// Compile one canonical agent without writing a provider file.
#[must_use]
pub fn compile(agent: &CanonicalAgent, provider: AgentProvider) -> AgentCompilation {
    if agent
        .targets
        .as_ref()
        .is_some_and(|targets| !targets.iter().any(|target| target == provider.id()))
    {
        return AgentCompilation {
            provider,
            not_targeted: true,
            bytes: None,
            findings: Vec::new(),
            overall: CompatibilityResult::Exact,
        };
    }

    let mut findings = baseline_findings(agent, provider);
    match provider {
        AgentProvider::Claude => compile_claude(agent, &mut findings),
        AgentProvider::Codex => compile_codex(agent, &mut findings),
    }
    findings.sort_by(|left, right| left.field.cmp(&right.field));
    let overall = findings
        .iter()
        .map(|finding| finding.result)
        .max()
        .unwrap_or(CompatibilityResult::Exact);
    let bytes = (overall != CompatibilityResult::Blocked).then(|| match provider {
        AgentProvider::Claude => render_claude(agent),
        AgentProvider::Codex => render_codex(agent),
    });
    AgentCompilation {
        provider,
        not_targeted: false,
        bytes,
        findings,
        overall,
    }
}

/// Compile a discovered package while reporting non-projectable support files.
///
/// The canonical entry point alone is sufficient for native rendering, but a
/// provider projection must not imply that Claude or Codex can read arbitrary
/// neighboring package files.  The finding is intentionally non-blocking: the
/// canonical package and its hash still retain those files for audit and future
/// adapters.
#[must_use]
pub fn compile_record(record: &AgentRecord, provider: AgentProvider) -> AgentCompilation {
    let mut compilation = compile(&record.agent, provider);
    if !compilation.not_targeted && record.has_support_files {
        compilation.findings.push(unsupported(
            "additional_package_files",
            "native provider projections do not expose canonical package support files; move reusable content into a required skill",
        ));
        compilation
            .findings
            .sort_by(|left, right| left.field.cmp(&right.field));
        compilation.overall = compilation
            .findings
            .iter()
            .map(|finding| finding.result)
            .max()
            .unwrap_or(CompatibilityResult::Exact);
    }
    compilation
}

fn baseline_findings(agent: &CanonicalAgent, provider: AgentProvider) -> Vec<CompatibilityFinding> {
    let mut findings = vec![
        exact("name", "native agent name"),
        exact("description", "native description"),
        exact("prompt", "native system prompt"),
        exact("targets", "Dalo projection allowlist"),
        mapped(
            "model.profile",
            model_mapping(provider, agent.model.profile),
        ),
    ];
    for field in ["id", "owners", "tags"] {
        findings.push(unsupported(
            field,
            "retained by Dalo but not emitted as native metadata",
        ));
    }
    findings
}

fn compile_claude(agent: &CanonicalAgent, findings: &mut Vec<CompatibilityFinding>) {
    if !agent.skills.is_empty() {
        findings.push(exact("skills", "Claude native skill preload metadata"));
    }
    if let Some(tools) = &agent.tools {
        if tools.allow.contains(&ToolCapability::UseMcp) {
            findings.push(blocked(
                "tools.allow",
                "Claude has no portable exact allowlist representation for use-mcp",
            ));
        } else {
            findings.push(mapped("tools.allow", "Claude tool allowlist"));
        }
    }
    add_unsupported_safety_boundaries(agent, findings, "Claude");
    validate_overlay(agent, AgentProvider::Claude, &["model"], findings);
}

fn compile_codex(agent: &CanonicalAgent, findings: &mut Vec<CompatibilityFinding>) {
    if !agent.skills.is_empty() {
        findings.push(CompatibilityFinding {
            field: "skills".to_owned(),
            result: CompatibilityResult::GuidanceOnly,
            message: "Codex receives deterministic skill-use guidance; it cannot preload a required skill".to_owned(),
            native_mapping: Some("developer_instructions".to_owned()),
        });
    }
    if agent.tools.is_some() {
        findings.push(blocked(
            "tools.allow",
            "Codex cannot enforce a portable hard tool allowlist",
        ));
    }
    add_unsupported_safety_boundaries(agent, findings, "Codex");
    validate_overlay(
        agent,
        AgentProvider::Codex,
        &["model", "model_reasoning_effort"],
        findings,
    );
}

fn add_unsupported_safety_boundaries(
    agent: &CanonicalAgent,
    findings: &mut Vec<CompatibilityFinding>,
    provider: &str,
) {
    if agent.filesystem.is_some() {
        findings.push(blocked(
            "filesystem",
            &format!("{provider} has no verified equivalent for this portable filesystem boundary"),
        ));
    }
    if agent.network.is_some() {
        findings.push(blocked(
            "network.allow",
            &format!("{provider} has no verified equivalent for this portable network boundary"),
        ));
    }
}

fn validate_overlay(
    agent: &CanonicalAgent,
    provider: AgentProvider,
    permitted: &[&str],
    findings: &mut Vec<CompatibilityFinding>,
) {
    let Some(value) = agent.providers.get(provider.id()) else {
        return;
    };
    let Some(overlay) = value.as_object() else {
        findings.push(blocked(
            &format!("providers.{}", provider.id()),
            "provider overlay must be a mapping",
        ));
        return;
    };
    for (key, value) in overlay {
        let field = format!("providers.{}.{}", provider.id(), key);
        if !permitted.contains(&key.as_str()) {
            findings.push(blocked(
                &field,
                "this provider overlay key cannot be represented without changing canonical safety semantics",
            ));
        } else if value.as_str().is_none() {
            findings.push(blocked(&field, "provider overlay values must be strings"));
        } else {
            findings.push(exact(&field, "exact provider-native overlay"));
        }
    }
}

fn render_claude(agent: &CanonicalAgent) -> String {
    let mut output = String::from("---\n");
    output.push_str("name: ");
    output.push_str(&yaml_string(&agent.name));
    output.push('\n');
    output.push_str("description: ");
    output.push_str(&yaml_string(&agent.description));
    output.push('\n');
    if let Some(model) = overlay_string(agent, AgentProvider::Claude, "model")
        .map(str::to_owned)
        .or_else(|| claude_model(agent.model.profile).map(str::to_owned))
    {
        output.push_str("model: ");
        output.push_str(&yaml_string(&model));
        output.push('\n');
    }
    if !agent.skills.is_empty() {
        output.push_str("skills:\n");
        for skill in &agent.skills {
            output.push_str("  - ");
            output.push_str(&yaml_string(skill));
            output.push('\n');
        }
    }
    if let Some(tools) = &agent.tools {
        let mut native = BTreeSet::new();
        for tool in &tools.allow {
            native.extend(tool.claude_tools().iter().copied());
        }
        output.push_str("tools:\n");
        for tool in native {
            output.push_str("  - ");
            output.push_str(&yaml_string(tool));
            output.push('\n');
        }
    }
    output.push_str("---\n");
    output.push_str(&agent.prompt);
    output
}

fn render_codex(agent: &CanonicalAgent) -> String {
    let mut output = String::new();
    output.push_str("name = ");
    output.push_str(&toml_string(&agent.name));
    output.push('\n');
    output.push_str("description = ");
    output.push_str(&toml_string(&agent.description));
    output.push('\n');
    if let Some(model) = overlay_string(agent, AgentProvider::Codex, "model") {
        output.push_str("model = ");
        output.push_str(&toml_string(model));
        output.push('\n');
    }
    if let Some(reasoning_effort) =
        overlay_string(agent, AgentProvider::Codex, "model_reasoning_effort")
            .or_else(|| codex_reasoning_effort(agent.model.profile))
    {
        output.push_str("model_reasoning_effort = ");
        output.push_str(&toml_string(reasoning_effort));
        output.push('\n');
    }
    let mut instructions = agent.prompt.clone();
    if !agent.skills.is_empty() {
        if !instructions.ends_with('\n') {
            instructions.push('\n');
        }
        instructions
            .push_str("\nRequired skills (must be available in the Codex skill directory):\n");
        for skill in &agent.skills {
            instructions.push_str("- Use `");
            instructions.push_str(skill);
            instructions.push_str("` when it applies.\n");
        }
    }
    output.push_str("developer_instructions = ");
    output.push_str(&toml_string(&instructions));
    output.push('\n');
    output
}

fn overlay_string<'a>(
    agent: &'a CanonicalAgent,
    provider: AgentProvider,
    key: &str,
) -> Option<&'a str> {
    agent.providers.get(provider.id())?.get(key)?.as_str()
}

fn claude_model(intent: ModelProfile) -> Option<&'static str> {
    match intent {
        ModelProfile::Inherit => None,
        ModelProfile::Fast => Some("haiku"),
        ModelProfile::Balanced => Some("sonnet"),
        ModelProfile::Deep => Some("opus"),
    }
}

fn codex_reasoning_effort(intent: ModelProfile) -> Option<&'static str> {
    match intent {
        ModelProfile::Inherit => None,
        ModelProfile::Fast => Some("low"),
        ModelProfile::Balanced => Some("medium"),
        ModelProfile::Deep => Some("high"),
    }
}

fn model_mapping(provider: AgentProvider, intent: ModelProfile) -> &'static str {
    match (provider, intent) {
        (AgentProvider::Claude, ModelProfile::Inherit) => "Claude inherited/default model",
        (AgentProvider::Claude, ModelProfile::Fast) => "Claude model haiku",
        (AgentProvider::Claude, ModelProfile::Balanced) => "Claude model sonnet",
        (AgentProvider::Claude, ModelProfile::Deep) => "Claude model opus",
        (AgentProvider::Codex, ModelProfile::Inherit) => {
            "Codex inherited model and reasoning effort"
        }
        (AgentProvider::Codex, ModelProfile::Fast) => {
            "Codex inherited model with low reasoning effort"
        }
        (AgentProvider::Codex, ModelProfile::Balanced) => {
            "Codex inherited model with medium reasoning effort"
        }
        (AgentProvider::Codex, ModelProfile::Deep) => {
            "Codex inherited model with high reasoning effort"
        }
    }
}

fn exact(field: &str, mapping: &str) -> CompatibilityFinding {
    CompatibilityFinding {
        field: field.to_owned(),
        result: CompatibilityResult::Exact,
        message: "preserved directly by the provider adapter".to_owned(),
        native_mapping: Some(mapping.to_owned()),
    }
}

fn mapped(field: &str, mapping: &str) -> CompatibilityFinding {
    CompatibilityFinding {
        field: field.to_owned(),
        result: CompatibilityResult::Mapped,
        message: "preserved through a deterministic provider mapping".to_owned(),
        native_mapping: Some(mapping.to_owned()),
    }
}

fn unsupported(field: &str, message: &str) -> CompatibilityFinding {
    CompatibilityFinding {
        field: field.to_owned(),
        result: CompatibilityResult::Unsupported,
        message: message.to_owned(),
        native_mapping: None,
    }
}

fn blocked(field: &str, message: &str) -> CompatibilityFinding {
    CompatibilityFinding {
        field: field.to_owned(),
        result: CompatibilityResult::Blocked,
        message: message.to_owned(),
        native_mapping: None,
    }
}

fn yaml_string(value: &str) -> String {
    // JSON strings are valid quoted YAML scalars and give us deterministic
    // escaping without relying on a serializer's map ordering.
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_agent(root: &Path, name: &str, contents: &str) -> PathBuf {
        let package = root.join("agents").join(name);
        fs::create_dir_all(&package).expect("package directory should be created");
        fs::write(package.join(AGENT_FILE), contents).expect("agent file should be written");
        package
    }

    #[test]
    fn scan_should_discover_and_hash_a_valid_canonical_package() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        let package = write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\nmodel:\n  profile: balanced\nskills: [pr-review]\n---\nReview the requested change.\n",
        );
        fs::write(package.join("notes.txt"), "support").expect("support file should be written");

        let inventory = scan_source_agents("local", temp.path());

        assert!(inventory.warnings.is_empty());
        assert_eq!(inventory.agents.len(), 1);
        let agent = &inventory.agents[0];
        assert_eq!(agent.source_ref, "local:reviewer");
        assert_eq!(agent.content_hash.len(), 64);
        assert!(agent.has_support_files);
    }

    #[test]
    fn scan_should_reject_noncanonical_names_and_missing_prompt() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: Reviewer\ndescription: Reviews code\n---\n",
        );
        let inventory = scan_source_agents("local", temp.path());
        assert!(inventory.agents.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            AgentInventoryWarningCode::InvalidPackage
        );
    }

    #[test]
    fn scan_should_reject_symlinks_anywhere_in_a_package() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        let package = write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\n---\nReview.\n",
        );
        std::os::unix::fs::symlink("AGENT.md", package.join("linked.md"))
            .expect("symlink should be created");
        let inventory = scan_source_agents("local", temp.path());
        assert!(inventory.agents.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            AgentInventoryWarningCode::UnsafePackageEntry
        );
    }

    #[test]
    fn compilation_should_be_deterministic_and_preserve_safe_mappings() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\nmodel:\n  profile: balanced\nskills:\n  - pr-review\ntools:\n  allow: [read-files, search-files]\nproviders:\n  claude:\n    model: sonnet-4\n---\nReview carefully.\n",
        );
        let agent = scan_source_agents("local", temp.path())
            .agents
            .remove(0)
            .agent;
        let first = compile(&agent, AgentProvider::Claude);
        let second = compile(&agent, AgentProvider::Claude);
        assert_eq!(first.bytes, second.bytes);
        assert_eq!(first.overall, CompatibilityResult::Unsupported);
        let output = first.bytes.expect("safe compilation");
        assert!(output.contains("model: \"sonnet-4\""));
        assert!(output.contains("skills:"));
    }

    #[test]
    fn scan_should_allow_punctuation_in_plain_yaml_scalars() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Review bugs!\nowners: [R&D reviewer]\n---\nReview bugs!\n",
        );

        let inventory = scan_source_agents("local", temp.path());

        assert!(inventory.warnings.is_empty());
        assert_eq!(inventory.agents[0].description, "Review bugs!");
        assert_eq!(inventory.agents[0].owners, ["R&D reviewer"]);
    }

    #[test]
    fn unsafe_yaml_indicators_are_rejected_at_scalar_boundaries() {
        assert!(reject_unsafe_yaml("description: !secret\n").is_err());
        assert!(reject_unsafe_yaml("description: &anchor value\n").is_err());
        assert!(reject_unsafe_yaml("description: *alias\n").is_err());
        assert!(reject_unsafe_yaml("description: ordinary! punctuation\n").is_ok());
    }

    #[test]
    fn json_serialization_redacts_secret_like_provider_overlays() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\nproviders:\n  claude:\n    model: sonnet-4\n    api_key: super-secret\n    nested:\n      access_token: nested-secret\n---\nReview carefully.\n",
        );
        let agent = scan_source_agents("local", temp.path())
            .agents
            .remove(0)
            .agent;

        let json = serde_json::to_value(agent).expect("agent should serialize");

        assert_eq!(json["providers"]["claude"]["model"], "sonnet-4");
        assert_eq!(json["providers"]["claude"]["api_key"], "[REDACTED]");
        assert_eq!(
            json["providers"]["claude"]["nested"]["access_token"],
            "[REDACTED]"
        );
    }

    #[test]
    fn package_support_files_should_be_visible_as_unsupported() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        let package = write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\n---\nReview carefully.\n",
        );
        fs::write(package.join("checklist.md"), "Check the diff.")
            .expect("support file should be written");
        let record = scan_source_agents("local", temp.path()).agents.remove(0);

        let compilation = compile_record(&record, AgentProvider::Claude);

        assert!(
            compilation
                .findings
                .iter()
                .any(|finding| finding.field == "additional_package_files")
        );
    }

    #[test]
    fn codex_should_block_explicit_tool_boundaries() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\ntools:\n  allow: []\n---\nReview carefully.\n",
        );
        let agent = scan_source_agents("local", temp.path())
            .agents
            .remove(0)
            .agent;
        let compilation = compile(&agent, AgentProvider::Codex);
        assert_eq!(compilation.overall, CompatibilityResult::Blocked);
        assert!(compilation.bytes.is_none());
    }

    #[test]
    fn empty_target_allowlist_should_not_compile_for_any_provider() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Reviews code\ntargets: []\n---\nReview carefully.\n",
        );
        let agent = scan_source_agents("local", temp.path())
            .agents
            .remove(0)
            .agent;
        assert!(compile(&agent, AgentProvider::Claude).not_targeted);
        assert!(compile(&agent, AgentProvider::Codex).not_targeted);
    }

    #[test]
    fn resolution_should_not_let_unapproved_team_agent_suppress_local_winner() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        let team_root = temp.path().join("team");
        let local_root = temp.path().join("local");
        write_agent(
            &team_root,
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Team reviewer\n---\nReview.\n",
        );
        write_agent(
            &local_root,
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Local reviewer\n---\nReview locally.\n",
        );
        let sources = vec![
            source("team", SourceKind::Team, &team_root, 0),
            source("local", SourceKind::Local, &local_root, 10),
        ];
        let inventories = vec![
            source_inventory("team", &team_root),
            source_inventory("local", &local_root),
        ];

        let resolution = resolve_agents(&sources, &inventories, &[]);

        assert_eq!(
            resolution.active_agents[0].agent.source_ref,
            "local:reviewer"
        );
        assert_eq!(
            resolution.pending_approval_agents[0].agent.source_ref,
            "team:reviewer"
        );
    }

    #[test]
    fn agent_approval_scope_should_not_match_skill_approvals() {
        let temp = tempfile::tempdir().expect("temp directory should be created");
        write_agent(
            temp.path(),
            "reviewer",
            "---\nschema_version: 1\nname: reviewer\ndescription: Team reviewer\n---\nReview.\n",
        );
        let sources = vec![source("team", SourceKind::Team, temp.path(), 0)];
        let inventories = vec![source_inventory("team", temp.path())];
        let skill_approval = ApprovalRecord {
            scope: "skill".to_owned(),
            value: "team:reviewer".to_owned(),
        };
        let agent_approval = ApprovalRecord {
            scope: "agent".to_owned(),
            value: "team:reviewer".to_owned(),
        };

        assert!(
            resolve_agents(&sources, &inventories, &[skill_approval])
                .active_agents
                .is_empty()
        );
        assert_eq!(
            resolve_agents(&sources, &inventories, &[agent_approval]).active_agents[0]
                .agent
                .source_ref,
            "team:reviewer"
        );
    }

    fn source(id: &str, kind: SourceKind, path: &Path, priority: i32) -> SourceConfig {
        SourceConfig {
            id: id.to_owned(),
            kind,
            path: path.to_path_buf(),
            priority,
            enabled: true,
            trusted: false,
            url: None,
            branch: None,
            update_policy: None,
            selection: Vec::new(),
            declared_by: None,
            declared_ref: None,
        }
    }

    fn source_inventory(source_id: &str, root: &Path) -> SourceInventory {
        let agent_inventory = scan_source_agents(source_id, root);
        SourceInventory {
            source_id: source_id.to_owned(),
            skills: Vec::new(),
            agents: agent_inventory.agents,
            warnings: Vec::new(),
            agent_warnings: agent_inventory.warnings,
        }
    }
}
