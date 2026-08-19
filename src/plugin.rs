//! Passive portable plugin package discovery and validation.
//!
//! A plugin package is inert inventory. Discovery never executes provider
//! overlays or reserved active component descriptors.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::AgentResolution;
use crate::config::{PluginPolicyDecision, PluginPolicyLayer, UserConfig};
use crate::hook::{self, HookBindingType, HookDescriptorV1};
use crate::inventory::SourceInventory;
use crate::resolver::Resolution;
use crate::source::SourceConfig;
use crate::team_manifest::{self, StackRequirement, TEAM_MANIFEST_FILE};

/// Canonical plugin manifest filename.
pub const PLUGIN_FILE: &str = "PLUGIN.toml";

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_STRING_BYTES: usize = 16 * 1024;
const MAX_LIST_ENTRIES: usize = 1024;
const MAX_TABLE_DEPTH: usize = 32;
const MAX_PACKAGE_ENTRIES: usize = 4096;
const MAX_PACKAGE_DEPTH: usize = 32;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

type PackageFiles = Vec<PackageFile>;

#[derive(Debug, Clone)]
struct PackageFile {
    path: String,
    executable: bool,
    bytes: Vec<u8>,
}

/// Passive plugin inventory for one source.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PluginInventory {
    /// Valid passive plugin packages.
    pub plugins: Vec<PluginRecord>,
    /// Non-fatal malformed-package findings.
    pub warnings: Vec<PluginInventoryWarning>,
}

/// One validated passive plugin package.
#[derive(Debug, Clone, Serialize)]
pub struct PluginRecord {
    /// Source containing the package.
    pub source_id: String,
    /// Canonical identity, `<source-id>:<slot-name>`.
    pub source_ref: String,
    /// Plugin slot name.
    pub slot_name: String,
    /// Optional stable identity.
    pub id: Option<String>,
    /// Human-facing description.
    pub description: String,
    /// Optional authored version.
    pub version: Option<String>,
    /// Package directory.
    pub path: PathBuf,
    /// Canonical manifest path.
    pub manifest_file: PathBuf,
    /// Ordered passive component declarations.
    pub members: Vec<PluginMember>,
    /// Validated active local-tool descriptors. Discovery remains inert.
    pub tools: Vec<ToolRecord>,
    /// Validated hook contracts bound to same-plugin local tools.
    pub hooks: Vec<HookRecord>,
    /// Ordered plugin dependency declarations.
    pub requires: Vec<PluginDependency>,
    /// Inert provider overlays retained for adapter-specific validation.
    pub providers: BTreeMap<String, toml::Value>,
    /// SHA-256 over the complete bounded package tree.
    pub package_hash: String,
}

/// One inert plugin-local hook contract with its exact approval identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookRecord {
    /// Source-qualified identity, `<source>:<plugin>#hook:<id>`.
    pub source_ref: String,
    /// Closed portable hook descriptor.
    pub descriptor: HookDescriptorV1,
    /// Exact same-plugin tool identity.
    pub tool_source_ref: String,
    /// Referenced invocation-contract hash from #499.
    pub tool_contract_hash: String,
    /// Complete security-relevant hook contract hash.
    pub contract_hash: String,
}

/// One validated plugin-local executable contract. Inventory never executes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRecord {
    /// Descriptor schema version.
    pub schema_version: u32,
    /// Plugin-local ID.
    pub id: String,
    /// Source-qualified identity, `<source>:<plugin>#tool:<id>`.
    pub source_ref: String,
    /// Plugin-root-relative executable entry path.
    pub entry: String,
    /// Runtime used to invoke the entry.
    pub runtime: ToolRuntime,
    /// Optional authored runtime version requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    /// Explicit supported operating systems; empty means every Dalo platform.
    pub platforms: Vec<ToolPlatform>,
    /// Event-independent named input schema.
    pub inputs: Vec<ToolInput>,
    /// Canonical exec-style argument template.
    pub argv: Vec<String>,
    /// Working-directory policy.
    pub cwd: ToolCwd,
    /// Environment variable names admitted into the invocation.
    pub env: Vec<String>,
    /// Portable capability claims.
    pub capabilities: Vec<ToolCapability>,
    /// Whether missing availability blocks the plugin.
    pub availability: ToolAvailability,
    /// Files in the bounded security-relevant closure.
    pub files: Vec<ToolFileRecord>,
    /// Deterministic invocation-contract hash; excludes whole-package provenance.
    pub contract_hash: String,
}

/// Supported local-tool runtime kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRuntime {
    /// Execute the immutable entry directly, without a shell.
    Executable,
    /// Invoke the immutable entry with `python3`.
    Python,
    /// Invoke the immutable entry with `node`.
    Node,
}

impl ToolRuntime {
    /// Runtime executable required on PATH, if any.
    #[must_use]
    pub const fn executable(self) -> Option<&'static str> {
        match self {
            Self::Executable => None,
            Self::Python => Some("python3"),
            Self::Node => Some("node"),
        }
    }
}

/// Portable operating-system requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPlatform {
    /// Apple macOS.
    Macos,
    /// Linux.
    Linux,
}

/// One named input admitted by the tool-owned argv template.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInput {
    /// Stable lower-snake input name.
    pub name: String,
    /// Primitive value type.
    #[serde(rename = "type")]
    pub kind: ToolInputType,
    /// Whether callers must supply a value.
    #[serde(default = "default_true")]
    pub required: bool,
}

/// Primitive named-input types supported by descriptor v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInputType {
    /// UTF-8 text.
    String,
    /// Filesystem path passed as opaque argument data.
    Path,
    /// Signed integer.
    Integer,
    /// Boolean rendered as `true` or `false`.
    Boolean,
}

/// Working directory used for execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCwd {
    /// Root of the immutable staged tool closure.
    ToolRoot,
}

/// Portable active-code capability claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    /// Read files outside the immutable closure.
    FilesystemRead,
    /// Write files outside the immutable closure.
    FilesystemWrite,
    /// Start child processes.
    Subprocess,
    /// Access the network.
    Network,
}

/// Required versus optional tool availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAvailability {
    /// Unavailable or unapproved blocks coherent activation.
    Required,
    /// Unavailable or unapproved is a visible omission.
    Optional,
}

/// One regular file participating in the tool contract closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFileRecord {
    /// Plugin-root-relative path.
    pub path: String,
    /// Whether the source file had any executable bit.
    pub executable: bool,
    /// SHA-256 of the exact bytes.
    pub content_hash: String,
}

/// One passive plugin member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginMember {
    /// Authored portable reference.
    #[serde(rename = "ref")]
    pub reference: ComponentReference,
    /// Required, optional, or instruction-only recommended membership.
    pub requirement: MemberRequirement,
    /// Optional agent-to-required-skill inline fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<InlineFallback>,
}

/// One plugin dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginDependency {
    /// Authored plugin reference.
    #[serde(rename = "ref")]
    pub reference: ComponentReference,
    /// Dependency strength.
    pub requirement: DependencyRequirement,
}

/// Parsed portable component reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ComponentReference {
    /// Component namespace.
    pub kind: ComponentKind,
    /// Exact source ID for a cross-source reference, otherwise `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Slot or stable-ID selector.
    pub selector: String,
}

impl ComponentReference {
    /// Render the canonical authored grammar.
    #[must_use]
    pub fn as_string(&self) -> String {
        self.source_id.as_ref().map_or_else(
            || format!("{}:{}", self.kind.as_str(), self.selector),
            |source_id| format!("{}:{source_id}:{}", self.kind.as_str(), self.selector),
        )
    }
}

/// Passive component namespaces accepted by schema version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    /// Another plugin package.
    Plugin,
    /// Managed skill.
    Skill,
    /// Canonical agent package.
    Agent,
    /// Instruction pack.
    Instruction,
}

impl ComponentKind {
    /// Stable reference prefix.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Skill => "skill",
            Self::Agent => "agent",
            Self::Instruction => "instruction",
        }
    }
}

/// Membership requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRequirement {
    /// Missing or inactive blocks plugin coherence.
    Required,
    /// Omission is visible but non-blocking.
    Optional,
    /// Instruction-only recommendation that never activates the pack.
    Recommended,
}

/// Plugin dependency requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRequirement {
    /// Missing or blocked dependency blocks the dependent.
    Required,
    /// Missing dependency is a visible non-blocking omission.
    Optional,
}

/// Authored inline fallback for an agent member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InlineFallback {
    /// Required skill member used as canonical inline behavior.
    pub skill: ComponentReference,
}

/// One non-fatal plugin inventory warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginInventoryWarning {
    /// Stable machine-readable warning code.
    pub code: PluginInventoryWarningCode,
    /// Package or entry path.
    pub path: PathBuf,
    /// Actionable detail.
    pub message: String,
}

/// Plugin inventory warning codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginInventoryWarningCode {
    /// Invalid schema, identity, reference, or a violated safety bound.
    InvalidPackage,
    /// Symlink or unsupported special file in the package boundary.
    UnsafePackageEntry,
    /// Two valid packages declare the same stable identity.
    DuplicateStableId,
    /// A source path could not be read.
    UnreadablePath,
    /// Reserved active-code descriptors are not accepted by the passive slice.
    UnsupportedActiveComponentSchema,
}

impl PluginInventoryWarningCode {
    /// Stable snake-case label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidPackage => "invalid_plugin_package",
            Self::UnsafePackageEntry => "unsafe_plugin_package_entry",
            Self::DuplicateStableId => "duplicate_plugin_stable_id",
            Self::UnreadablePath => "unreadable_plugin_path",
            Self::UnsupportedActiveComponentSchema => "unsupported_active_component_schema",
        }
    }
}

impl std::fmt::Display for PluginInventoryWarningCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    plugin: ManifestPlugin,
    #[serde(default)]
    providers: BTreeMap<String, toml::Value>,
    #[serde(default, rename = "tool")]
    tools: Vec<ManifestTool>,
    #[serde(default, rename = "hook")]
    hooks: Vec<HookDescriptorV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestPlugin {
    name: String,
    #[serde(default)]
    id: Option<String>,
    description: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    members: Vec<ManifestMember>,
    #[serde(default)]
    requires: Vec<ManifestDependency>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestMember {
    #[serde(rename = "ref")]
    reference: String,
    requirement: MemberRequirement,
    #[serde(default)]
    fallback: Option<ManifestFallback>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDependency {
    #[serde(rename = "ref")]
    reference: String,
    requirement: DependencyRequirement,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFallback {
    kind: String,
    skill: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestTool {
    schema_version: u32,
    id: String,
    entry: String,
    runtime: ToolRuntime,
    #[serde(default)]
    runtime_version: Option<String>,
    #[serde(default)]
    platforms: Vec<ToolPlatform>,
    #[serde(default)]
    inputs: Vec<ToolInput>,
    argv: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    cwd: ToolCwd,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    capabilities: Vec<ToolCapability>,
    availability: ToolAvailability,
}

const fn default_true() -> bool {
    true
}

/// Scan only exact `plugins/<name>/PLUGIN.toml` packages in one source.
#[must_use]
pub fn scan_source_plugins(source_id: &str, source_root: &Path) -> PluginInventory {
    let plugins_root = source_root.join("plugins");
    let metadata = match fs::symlink_metadata(&plugins_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return PluginInventory::default();
        }
        Err(error) => {
            return inventory_warning(
                PluginInventoryWarningCode::UnreadablePath,
                plugins_root,
                error.to_string(),
            );
        }
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() {
        return inventory_warning(
            PluginInventoryWarningCode::UnsafePackageEntry,
            plugins_root,
            "the source `plugins` directory must not be a symlink".to_owned(),
        );
    }
    if !metadata.is_dir() {
        return inventory_warning(
            PluginInventoryWarningCode::InvalidPackage,
            plugins_root,
            "the source `plugins` path must be a directory".to_owned(),
        );
    }

    let entries = match fs::read_dir(&plugins_root) {
        Ok(entries) => entries,
        Err(error) => {
            return inventory_warning(
                PluginInventoryWarningCode::UnreadablePath,
                plugins_root,
                error.to_string(),
            );
        }
    };
    let mut packages = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(warning(
                    PluginInventoryWarningCode::UnreadablePath,
                    plugins_root.clone(),
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
                    PluginInventoryWarningCode::UnreadablePath,
                    path,
                    error.to_string(),
                ));
                continue;
            }
        };
        if file_type.is_symlink() {
            warnings.push(warning(
                PluginInventoryWarningCode::UnsafePackageEntry,
                path,
                "plugin package directories must not be symlinks".to_owned(),
            ));
        } else if file_type.is_dir() && path.join(PLUGIN_FILE).exists() {
            packages.push(path);
        }
    }
    packages.sort();

    let mut plugins = Vec::new();
    for package_path in packages {
        match scan_package(source_id, &package_path) {
            Ok(plugin) => plugins.push(plugin),
            Err((code, error)) => warnings.push(warning(code, package_path, error)),
        }
    }
    reject_duplicate_stable_ids(source_id, &mut plugins, &mut warnings);
    plugins.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });
    PluginInventory { plugins, warnings }
}

fn inventory_warning(
    code: PluginInventoryWarningCode,
    path: PathBuf,
    message: String,
) -> PluginInventory {
    PluginInventory {
        plugins: Vec::new(),
        warnings: vec![warning(code, path, message)],
    }
}

fn warning(
    code: PluginInventoryWarningCode,
    path: PathBuf,
    message: String,
) -> PluginInventoryWarning {
    PluginInventoryWarning {
        code,
        path,
        message,
    }
}

fn scan_package(
    source_id: &str,
    package_path: &Path,
) -> Result<PluginRecord, (PluginInventoryWarningCode, String)> {
    let package_name = package_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("plugin package directory name must be valid UTF-8"))?;
    if !is_plugin_name(package_name) {
        return Err(invalid(
            "plugin package directory must use lower kebab-case",
        ));
    }
    let entries = collect_package_files(package_path)?;
    let manifest_file = package_path.join(PLUGIN_FILE);
    let manifest_bytes = entries
        .iter()
        .find_map(|entry| (entry.path == PLUGIN_FILE).then_some(entry.bytes.as_slice()))
        .ok_or_else(|| invalid(format!("package is missing {PLUGIN_FILE}")))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid(format!(
            "{PLUGIN_FILE} exceeds the {MAX_MANIFEST_BYTES}-byte safety limit"
        )));
    }
    let document = std::str::from_utf8(manifest_bytes)
        .map_err(|_| invalid(format!("{PLUGIN_FILE} must be valid UTF-8")))?;
    let value: toml::Value =
        toml::from_str(document).map_err(|error| invalid(format!("invalid TOML: {error}")))?;
    validate_toml_bounds(&value, 0)?;
    let manifest: Manifest = value
        .try_into()
        .map_err(|error| invalid(format!("invalid plugin schema: {error}")))?;
    if manifest.schema_version != 1 {
        return Err(invalid(format!(
            "unsupported plugin schema version {} (supported: 1)",
            manifest.schema_version
        )));
    }
    validate_manifest(package_name, &manifest)?;
    let members = manifest
        .plugin
        .members
        .iter()
        .map(parse_member)
        .collect::<Result<Vec<_>, _>>()?;
    let requires = manifest
        .plugin
        .requires
        .iter()
        .map(parse_dependency)
        .collect::<Result<Vec<_>, _>>()?;
    validate_member_closure(&members)?;
    let plugin_ref = format!("{source_id}:{package_name}");
    let tools = parse_tools(&plugin_ref, &manifest.tools, &entries)?;
    let hooks = parse_hooks(&plugin_ref, &manifest.hooks, &tools)?;
    let package_hash = hash_package_files(&entries);

    Ok(PluginRecord {
        source_id: source_id.to_owned(),
        source_ref: plugin_ref,
        slot_name: package_name.to_owned(),
        id: manifest.plugin.id,
        description: manifest.plugin.description,
        version: manifest.plugin.version,
        path: package_path.to_path_buf(),
        manifest_file,
        members,
        tools,
        hooks,
        requires,
        providers: manifest.providers,
        package_hash,
    })
}

fn parse_hooks(
    plugin_ref: &str,
    descriptors: &[HookDescriptorV1],
    tools: &[ToolRecord],
) -> Result<Vec<HookRecord>, (PluginInventoryWarningCode, String)> {
    if descriptors.len() > MAX_LIST_ENTRIES {
        return Err(invalid(format!(
            "plugin declares more than {MAX_LIST_ENTRIES} hooks"
        )));
    }
    let mut ids = BTreeSet::new();
    let mut hooks = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        if descriptor.schema_version != 1 {
            return Err((
                PluginInventoryWarningCode::UnsupportedActiveComponentSchema,
                format!(
                    "hook `{}` uses unsupported descriptor schema {} (supported: 1)",
                    descriptor.id, descriptor.schema_version
                ),
            ));
        }
        hook::validate_descriptor(descriptor).map_err(invalid)?;
        if !ids.insert(descriptor.id.as_str()) {
            return Err(invalid(format!(
                "plugin contains duplicate hook ID `{}`",
                descriptor.id
            )));
        }
        let tool = tools
            .iter()
            .find(|candidate| candidate.id == descriptor.tool)
            .ok_or_else(|| {
                invalid(format!(
                    "hook `{}` references missing same-plugin tool `{}`",
                    descriptor.id, descriptor.tool
                ))
            })?;
        let tool_inputs = tool
            .inputs
            .iter()
            .map(|input| (input.name.as_str(), input))
            .collect::<BTreeMap<_, _>>();
        for binding in &descriptor.bindings {
            let input = tool_inputs.get(binding.input.as_str()).ok_or_else(|| {
                invalid(format!(
                    "hook `{}` binds unknown tool input `{}`",
                    descriptor.id, binding.input
                ))
            })?;
            let expected = match input.kind {
                ToolInputType::String => Some(HookBindingType::String),
                ToolInputType::Path => Some(HookBindingType::Path),
                ToolInputType::Boolean => Some(HookBindingType::Boolean),
                ToolInputType::Integer => None,
            };
            if expected != Some(binding.field.input_type()) {
                return Err(invalid(format!(
                    "hook `{}` field `{}` is incompatible with tool input `{}`",
                    descriptor.id,
                    binding.field.as_str(),
                    binding.input
                )));
            }
        }
        for input in tool.inputs.iter().filter(|input| input.required) {
            if !descriptor
                .bindings
                .iter()
                .any(|binding| binding.input == input.name)
            {
                return Err(invalid(format!(
                    "hook `{}` does not bind required tool input `{}`",
                    descriptor.id, input.name
                )));
            }
        }
        let source_ref = format!("{plugin_ref}#hook:{}", descriptor.id);
        let contract_hash = hook::contract_hash(
            &source_ref,
            descriptor,
            &tool.source_ref,
            &tool.contract_hash,
        );
        hooks.push(HookRecord {
            source_ref,
            descriptor: descriptor.clone(),
            tool_source_ref: tool.source_ref.clone(),
            tool_contract_hash: tool.contract_hash.clone(),
            contract_hash,
        });
    }
    hooks.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    Ok(hooks)
}

fn invalid(message: impl Into<String>) -> (PluginInventoryWarningCode, String) {
    (PluginInventoryWarningCode::InvalidPackage, message.into())
}

fn validate_manifest(
    package_name: &str,
    manifest: &Manifest,
) -> Result<(), (PluginInventoryWarningCode, String)> {
    let plugin = &manifest.plugin;
    if plugin.name != package_name {
        return Err(invalid(format!(
            "plugin.name `{}` must equal package directory `{package_name}`",
            plugin.name
        )));
    }
    if plugin.description.trim().is_empty() {
        return Err(invalid("plugin.description must not be empty"));
    }
    if let Some(id) = &plugin.id
        && !is_stable_id(id)
    {
        return Err(invalid(
            "plugin.id must be 1-128 ASCII lower-case characters using dots or hyphens as separators",
        ));
    }
    if plugin.members.len() > MAX_LIST_ENTRIES || plugin.requires.len() > MAX_LIST_ENTRIES {
        return Err(invalid(format!(
            "plugin lists may contain at most {MAX_LIST_ENTRIES} entries"
        )));
    }
    let member_refs = plugin
        .members
        .iter()
        .map(|member| member.reference.as_str())
        .collect::<BTreeSet<_>>();
    if member_refs.len() != plugin.members.len() {
        return Err(invalid("plugin contains duplicate member declarations"));
    }
    let dependency_refs = plugin
        .requires
        .iter()
        .map(|dependency| dependency.reference.as_str())
        .collect::<BTreeSet<_>>();
    if dependency_refs.len() != plugin.requires.len() {
        return Err(invalid("plugin contains duplicate dependency declarations"));
    }
    Ok(())
}

fn parse_member(
    member: &ManifestMember,
) -> Result<PluginMember, (PluginInventoryWarningCode, String)> {
    let reference = parse_reference(&member.reference)?;
    if reference.kind == ComponentKind::Plugin {
        return Err(invalid(
            "plugin members may reference only skill, agent, or instruction components",
        ));
    }
    if member.requirement == MemberRequirement::Recommended
        && reference.kind != ComponentKind::Instruction
    {
        return Err(invalid(
            "recommended membership is valid only for instructions",
        ));
    }
    let fallback = member
        .fallback
        .as_ref()
        .map(|fallback| {
            if reference.kind != ComponentKind::Agent {
                return Err(invalid("fallback is valid only for agent members"));
            }
            if fallback.kind != "inline" {
                return Err(invalid("fallback.kind must be `inline`"));
            }
            let skill = parse_reference(&fallback.skill)?;
            if skill.kind != ComponentKind::Skill {
                return Err(invalid("inline fallback must reference a skill"));
            }
            Ok(InlineFallback { skill })
        })
        .transpose()?;
    Ok(PluginMember {
        reference,
        requirement: member.requirement,
        fallback,
    })
}

fn parse_dependency(
    dependency: &ManifestDependency,
) -> Result<PluginDependency, (PluginInventoryWarningCode, String)> {
    let reference = parse_reference(&dependency.reference)?;
    if reference.kind != ComponentKind::Plugin {
        return Err(invalid(
            "plugin.requires entries must use plugin references",
        ));
    }
    Ok(PluginDependency {
        reference,
        requirement: dependency.requirement,
    })
}

fn parse_reference(
    reference: &str,
) -> Result<ComponentReference, (PluginInventoryWarningCode, String)> {
    let parts = reference.split(':').collect::<Vec<_>>();
    if !(parts.len() == 2 || parts.len() == 3) || parts.iter().any(|part| part.is_empty()) {
        return Err(invalid(format!("invalid portable reference `{reference}`")));
    }
    let kind = match parts[0] {
        "plugin" => ComponentKind::Plugin,
        "skill" => ComponentKind::Skill,
        "agent" => ComponentKind::Agent,
        "instruction" => ComponentKind::Instruction,
        _ => return Err(invalid(format!("unknown reference kind in `{reference}`"))),
    };
    let (source_id, selector) = if parts.len() == 3 {
        if !is_reference_atom(parts[1]) {
            return Err(invalid(format!("invalid source ID in `{reference}`")));
        }
        (Some(parts[1].to_owned()), parts[2])
    } else {
        (None, parts[1])
    };
    if !is_reference_atom(selector) {
        return Err(invalid(format!("invalid selector in `{reference}`")));
    }
    Ok(ComponentReference {
        kind,
        source_id,
        selector: selector.to_owned(),
    })
}

fn validate_member_closure(
    members: &[PluginMember],
) -> Result<(), (PluginInventoryWarningCode, String)> {
    let required_skills = members
        .iter()
        .filter(|member| {
            member.reference.kind == ComponentKind::Skill
                && member.requirement == MemberRequirement::Required
        })
        .map(|member| &member.reference)
        .collect::<BTreeSet<_>>();
    for fallback in members.iter().filter_map(|member| member.fallback.as_ref()) {
        if !required_skills.contains(&fallback.skill) {
            return Err(invalid(format!(
                "inline fallback `{}` must also be a required skill member",
                fallback.skill.as_string()
            )));
        }
    }
    Ok(())
}

fn parse_tools(
    plugin_ref: &str,
    descriptors: &[ManifestTool],
    package_files: &PackageFiles,
) -> Result<Vec<ToolRecord>, (PluginInventoryWarningCode, String)> {
    if descriptors.len() > MAX_LIST_ENTRIES {
        return Err(invalid(format!(
            "plugin declares more than {MAX_LIST_ENTRIES} tools"
        )));
    }
    let mut ids = BTreeSet::new();
    let mut tools = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        if descriptor.schema_version != 1 {
            return Err((
                PluginInventoryWarningCode::UnsupportedActiveComponentSchema,
                format!(
                    "tool `{}` uses unsupported descriptor schema {} (supported: 1)",
                    descriptor.id, descriptor.schema_version
                ),
            ));
        }
        if !is_plugin_name(&descriptor.id) {
            return Err(invalid("tool.id must use lower kebab-case"));
        }
        if !ids.insert(descriptor.id.as_str()) {
            return Err(invalid(format!(
                "plugin contains duplicate tool ID `{}`",
                descriptor.id
            )));
        }
        let entry = validate_tool_path(&descriptor.entry)?;
        if descriptor.files.len() > MAX_LIST_ENTRIES {
            return Err(invalid(format!(
                "tool `{}` declares too many closure files",
                descriptor.id
            )));
        }
        if descriptor.argv.len() > MAX_LIST_ENTRIES {
            return Err(invalid(format!(
                "tool `{}` argv template is too large",
                descriptor.id
            )));
        }
        let mut input_names = BTreeSet::new();
        for input in &descriptor.inputs {
            if !is_input_name(&input.name) || !input_names.insert(input.name.as_str()) {
                return Err(invalid(format!(
                    "tool `{}` input names must be unique lower_snake_case values",
                    descriptor.id
                )));
            }
        }
        for argument in &descriptor.argv {
            if argument.len() > MAX_STRING_BYTES {
                return Err(invalid(format!(
                    "tool `{}` contains an oversized argv element",
                    descriptor.id
                )));
            }
            if let Some(name) = argument
                .strip_prefix("${input.")
                .and_then(|value| value.strip_suffix('}'))
            {
                if !input_names.contains(name) {
                    return Err(invalid(format!(
                        "tool `{}` argv references unknown input `{name}`",
                        descriptor.id
                    )));
                }
            } else if argument.contains("${") {
                return Err(invalid(format!(
                    "tool `{}` uses an unsafe placeholder; v1 accepts only whole-token `${{input.name}}` values",
                    descriptor.id
                )));
            }
        }
        if let Some(version) = &descriptor.runtime_version
            && (version.is_empty()
                || version.len() > 128
                || !version.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'.' | b'-' | b'+' | b'<' | b'>' | b'=' | b' ')
                }))
        {
            return Err(invalid(format!(
                "tool `{}` has an invalid runtime_version requirement",
                descriptor.id
            )));
        }
        let mut platforms = descriptor.platforms.clone();
        platforms.sort();
        platforms.dedup();
        let mut env = descriptor.env.clone();
        env.sort();
        env.dedup();
        if env.len() != descriptor.env.len() || env.iter().any(|name| !is_env_name(name)) {
            return Err(invalid(format!(
                "tool `{}` env values must be unique portable environment names",
                descriptor.id
            )));
        }
        let mut capabilities = descriptor.capabilities.clone();
        capabilities.sort();
        capabilities.dedup();

        let mut closure_paths = BTreeSet::from([entry.clone()]);
        for path in &descriptor.files {
            let path = validate_tool_path(path)?;
            if !closure_paths.insert(path) {
                return Err(invalid(format!(
                    "tool `{}` contains a duplicate closure path",
                    descriptor.id
                )));
            }
        }
        let mut files = Vec::with_capacity(closure_paths.len());
        for path in closure_paths {
            let package_file = package_files
                .iter()
                .find(|candidate| candidate.path == path)
                .ok_or_else(|| {
                    invalid(format!(
                        "tool `{}` references missing regular file `{path}`",
                        descriptor.id
                    ))
                })?;
            files.push(ToolFileRecord {
                path,
                executable: package_file.executable,
                content_hash: hash_bytes(&package_file.bytes),
            });
        }
        if descriptor.runtime == ToolRuntime::Executable
            && !files
                .iter()
                .find(|file| file.path == entry)
                .is_some_and(|file| file.executable)
        {
            return Err(invalid(format!(
                "tool `{}` executable entry is not marked executable",
                descriptor.id
            )));
        }
        let source_ref = format!("{plugin_ref}#tool:{}", descriptor.id);
        let mut record = ToolRecord {
            schema_version: descriptor.schema_version,
            id: descriptor.id.clone(),
            source_ref,
            entry,
            runtime: descriptor.runtime,
            runtime_version: descriptor.runtime_version.clone(),
            platforms,
            inputs: descriptor.inputs.clone(),
            argv: descriptor.argv.clone(),
            cwd: descriptor.cwd,
            env,
            capabilities,
            availability: descriptor.availability,
            files,
            contract_hash: String::new(),
        };
        record.contract_hash = hash_tool_contract(&record);
        tools.push(record);
    }
    tools.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(tools)
}

fn validate_tool_path(path: &str) -> Result<String, (PluginInventoryWarningCode, String)> {
    if path.is_empty() || path.len() > MAX_STRING_BYTES || path.contains('\\') {
        return Err(invalid(
            "tool paths must be non-empty portable relative paths",
        ));
    }
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|part| {
            !matches!(part, std::path::Component::Normal(_)) || part.as_os_str().to_str().is_none()
        })
    {
        return Err(invalid(format!(
            "tool path `{path}` must stay lexically inside the plugin package"
        )));
    }
    Ok(candidate
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn is_input_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && value.as_bytes()[0].is_ascii_lowercase()
        && !value.ends_with('_')
        && !value.contains("__")
}

fn is_env_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && (value.as_bytes()[0].is_ascii_uppercase() || value.as_bytes()[0] == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_tool_contract(tool: &ToolRecord) -> String {
    let mut hash = Sha256::new();
    hash.update(b"dalo-tool-contract-v1\0");
    hash_contract_value(&mut hash, &tool.source_ref);
    hash_contract_value(&mut hash, &tool.schema_version.to_string());
    hash_contract_value(&mut hash, &tool.entry);
    hash_contract_value(&mut hash, &format!("{:?}", tool.runtime));
    hash_contract_value(&mut hash, tool.runtime_version.as_deref().unwrap_or(""));
    for platform in &tool.platforms {
        hash_contract_value(&mut hash, &format!("platform:{platform:?}"));
    }
    for input in &tool.inputs {
        hash_contract_value(
            &mut hash,
            &format!("input:{}:{:?}:{}", input.name, input.kind, input.required),
        );
    }
    for argument in &tool.argv {
        hash_contract_value(&mut hash, &format!("argv:{argument}"));
    }
    hash_contract_value(&mut hash, &format!("cwd:{:?}", tool.cwd));
    for name in &tool.env {
        hash_contract_value(&mut hash, &format!("env:{name}"));
    }
    for capability in &tool.capabilities {
        hash_contract_value(&mut hash, &format!("capability:{capability:?}"));
    }
    hash_contract_value(&mut hash, &format!("availability:{:?}", tool.availability));
    for file in &tool.files {
        hash_contract_value(
            &mut hash,
            &format!(
                "file:{}:{}:{}",
                file.path, file.executable, file.content_hash
            ),
        );
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_contract_value(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
}

fn validate_toml_bounds(
    value: &toml::Value,
    depth: usize,
) -> Result<(), (PluginInventoryWarningCode, String)> {
    if depth > MAX_TABLE_DEPTH {
        return Err(invalid(format!(
            "TOML nesting exceeds {MAX_TABLE_DEPTH} levels"
        )));
    }
    match value {
        toml::Value::String(value) if value.len() > MAX_STRING_BYTES => Err(invalid(format!(
            "TOML string exceeds {MAX_STRING_BYTES} bytes"
        ))),
        toml::Value::Array(values) => {
            if values.len() > MAX_LIST_ENTRIES {
                return Err(invalid(format!(
                    "TOML list exceeds {MAX_LIST_ENTRIES} entries"
                )));
            }
            for value in values {
                validate_toml_bounds(value, depth + 1)?;
            }
            Ok(())
        }
        toml::Value::Table(values) => {
            if values.len() > MAX_LIST_ENTRIES {
                return Err(invalid(format!(
                    "TOML table exceeds {MAX_LIST_ENTRIES} entries"
                )));
            }
            for (key, value) in values {
                if key.len() > MAX_STRING_BYTES {
                    return Err(invalid(format!(
                        "TOML key exceeds {MAX_STRING_BYTES} bytes"
                    )));
                }
                validate_toml_bounds(value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn collect_package_files(
    package_path: &Path,
) -> Result<PackageFiles, (PluginInventoryWarningCode, String)> {
    let mut files = Vec::new();
    let mut entry_count = 0;
    let mut total_bytes = 0_u64;
    collect_package_files_inner(
        package_path,
        package_path,
        0,
        &mut files,
        &mut entry_count,
        &mut total_bytes,
    )?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_package_files_inner(
    package_root: &Path,
    current: &Path,
    depth: usize,
    files: &mut PackageFiles,
    entry_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), (PluginInventoryWarningCode, String)> {
    if depth > MAX_PACKAGE_DEPTH {
        return Err(invalid(format!(
            "package exceeds {MAX_PACKAGE_DEPTH} directory levels"
        )));
    }
    let mut entries = fs::read_dir(current)
        .map_err(|error| {
            (
                PluginInventoryWarningCode::UnreadablePath,
                error.to_string(),
            )
        })?
        .map(|entry| {
            entry.map_err(|error| {
                (
                    PluginInventoryWarningCode::UnreadablePath,
                    error.to_string(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *entry_count += 1;
        if *entry_count > MAX_PACKAGE_ENTRIES {
            return Err(invalid(format!(
                "package exceeds {MAX_PACKAGE_ENTRIES} entries"
            )));
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            (
                PluginInventoryWarningCode::UnreadablePath,
                error.to_string(),
            )
        })?;
        if file_type.is_symlink() {
            return Err((
                PluginInventoryWarningCode::UnsafePackageEntry,
                format!("package contains symlink `{}`", path.display()),
            ));
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
            return Err((
                PluginInventoryWarningCode::UnsafePackageEntry,
                format!(
                    "package contains unsupported special file `{}`",
                    path.display()
                ),
            ));
        }
        let metadata = entry.metadata().map_err(|error| {
            (
                PluginInventoryWarningCode::UnreadablePath,
                error.to_string(),
            )
        })?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(invalid(format!(
                "file `{}` exceeds {MAX_FILE_BYTES} bytes",
                path.display()
            )));
        }
        let relative = relative_package_path(package_root, &path)?;
        let executable = metadata.permissions().mode() & 0o111 != 0;
        let bytes = fs::read(&path).map_err(|error| {
            (
                PluginInventoryWarningCode::UnreadablePath,
                error.to_string(),
            )
        })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(format!(
                "file `{}` exceeded {MAX_FILE_BYTES} bytes while being read",
                path.display()
            )));
        }
        *total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("package byte count overflowed"))?;
        if *total_bytes > MAX_PACKAGE_BYTES {
            return Err(invalid(format!(
                "package exceeds {MAX_PACKAGE_BYTES} total bytes"
            )));
        }
        files.push(PackageFile {
            path: relative,
            executable,
            bytes,
        });
    }
    Ok(())
}

fn relative_package_path(
    package_root: &Path,
    path: &Path,
) -> Result<String, (PluginInventoryWarningCode, String)> {
    let relative = path
        .strip_prefix(package_root)
        .map_err(|_| invalid("package entry escaped its package root"))?;
    let mut parts = Vec::new();
    for part in relative.components() {
        let std::path::Component::Normal(part) = part else {
            return Err(invalid("package contains a non-normal path component"));
        };
        parts.push(
            part.to_str()
                .ok_or_else(|| invalid("package entry path must be valid UTF-8"))?,
        );
    }
    Ok(parts.join("/"))
}

fn hash_package_files(entries: &PackageFiles) -> String {
    let mut hash = Sha256::new();
    hash.update(b"dalo-plugin-package-v1\0");
    for entry in entries {
        hash.update((entry.path.len() as u64).to_be_bytes());
        hash.update(entry.path.as_bytes());
        hash.update(*b"f");
        hash.update([u8::from(entry.executable)]);
        hash.update((entry.bytes.len() as u64).to_be_bytes());
        hash.update(&entry.bytes);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reject_duplicate_stable_ids(
    source_id: &str,
    plugins: &mut Vec<PluginRecord>,
    warnings: &mut Vec<PluginInventoryWarning>,
) {
    let mut paths_by_id = BTreeMap::<String, Vec<PathBuf>>::new();
    for plugin in plugins.iter() {
        if let Some(id) = &plugin.id {
            paths_by_id
                .entry(id.clone())
                .or_default()
                .push(plugin.path.clone());
        }
    }
    let duplicated = paths_by_id
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect::<Vec<_>>();
    for (id, paths) in duplicated {
        for path in paths {
            warnings.push(warning(
                PluginInventoryWarningCode::DuplicateStableId,
                path,
                format!("source `{source_id}` contains multiple plugins with stable ID `{id}`"),
            ));
        }
        plugins.retain(|plugin| plugin.id.as_deref() != Some(id.as_str()));
    }
}

fn is_plugin_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_stable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && !value.contains("..")
        && !value.contains("--")
        && !value.contains(".-")
        && !value.contains("-.")
}

fn is_reference_atom(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_STRING_BYTES && !value.contains([':', '#', '/', '\\'])
}

/// Source of selected plugin intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionOriginKind {
    /// Authored team stack manifest.
    Stack,
    /// Additive local user selection.
    Direct,
    /// Reachable dependency from another selected plugin.
    Dependency,
}

/// Strength retained on one selection origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrength {
    /// Optional dependency intent.
    Optional,
    /// Recommended authored-stack intent.
    Recommended,
    /// Required stack, direct, or dependency intent.
    Required,
}

/// Provenance for one selected plugin identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SelectionOrigin {
    /// Origin namespace.
    pub kind: SelectionOriginKind,
    /// Manifest source, local config, or declaring plugin identity.
    pub declared_by: String,
    /// Strength contributed by this origin.
    pub requirement: SelectionStrength,
}

/// Canonical plugin state before target adapters run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Selected passive package with a coherent required closure.
    Selected,
    /// Required dependency or component is not coherent.
    Blocked,
    /// Selected intent suppressed by an explicit local policy.
    Declined,
    /// Selected candidate lost its plugin slot to a higher-precedence candidate.
    Shadowed,
}

/// Passive component availability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentState {
    /// Component passed its independent activation boundary.
    Active,
    /// Component exists but still needs its scoped approval.
    PendingApproval,
    /// Component exists but lost its own namespace slot.
    Shadowed,
    /// Component exists but its own closure is blocked.
    Blocked,
    /// Canonical component is present in the exact requested source.
    Available,
    /// Instruction is present but remains independently inactive.
    Inactive,
    /// Reference matched no component.
    Missing,
    /// Slot and stable-ID lookup matched different components.
    Ambiguous,
}

/// Evaluated passive member in canonical structured output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPluginMember {
    /// Authored portable reference.
    pub reference: String,
    /// Membership requirement.
    pub requirement: MemberRequirement,
    /// Availability without granting activation.
    pub state: PluginComponentState,
    /// Canonical source-qualified component identity when resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_ref: Option<String>,
    /// Authored required-skill inline fallback for an agent member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// Canonical dependency outcome retained for planning and display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPluginDependency {
    /// Authored plugin reference.
    pub reference: String,
    /// Dependency requirement.
    pub requirement: DependencyRequirement,
    /// Exact dependency outcome.
    pub state: PluginDependencyState,
    /// Canonical identity when reference lookup succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_ref: Option<String>,
}

/// Canonical plugin dependency state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDependencyState {
    /// Dependency is the selected winner of its plugin slot.
    Selected,
    /// Dependency candidate exists but lost its plugin slot.
    Shadowed,
    /// Dependency is suppressed by explicit local policy.
    Declined,
    /// Exact source contained no matching plugin.
    Missing,
    /// Slot and stable-ID lookup selected different packages.
    Ambiguous,
}

/// One canonical selected or shadowed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedPlugin {
    /// Canonical `<source-id>:<slot-name>` identity.
    pub source_ref: String,
    /// Plugin slot name.
    pub slot_name: String,
    /// Stable optional identity.
    pub id: Option<String>,
    /// Source priority used for deterministic winner selection.
    pub source_priority: i32,
    /// Complete package-tree hash.
    pub package_hash: String,
    /// Effective passive closure hash, excluding selection provenance.
    pub closure_hash: String,
    /// All retained selection origins.
    pub origins: Vec<SelectionOrigin>,
    /// Applied policy decisions, retained outside the closure hash.
    pub policies: Vec<AppliedPluginPolicy>,
    /// Canonical state.
    pub state: PluginState,
    /// Winning identity for a shadowed candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadowed_by: Option<String>,
    /// Passive component findings.
    pub members: Vec<ResolvedPluginMember>,
    /// Canonical dependency outcomes.
    pub dependencies: Vec<ResolvedPluginDependency>,
    /// Deterministic blocking reasons.
    pub blocking_reasons: Vec<String>,
}

/// One applied local policy decision with its audit provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedPluginPolicy {
    /// Persisted local layer.
    pub layer: PluginPolicyLayer,
    /// Stable rule identity.
    pub rule_id: String,
    /// Version-2 policy decision.
    pub decision: PluginPolicyDecision,
    /// Human audit context; excluded from closure hashing.
    pub reason: String,
}

/// Machine-readable plugin resolution diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginDiagnostic {
    /// Stable diagnostic code.
    pub code: PluginDiagnosticCode,
    /// Affected authored reference or canonical identity.
    pub subject: String,
    /// Actionable detail.
    pub message: String,
}

/// Plugin resolution diagnostic codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticCode {
    /// Authored stack could not be parsed or did not identify its source.
    InvalidStackSelection,
    /// Selected plugin or dependency does not exist in the exact source.
    MissingPlugin,
    /// Slot and stable identity select different packages.
    AmbiguousPluginReference,
    /// Required plugin dependencies contain a cycle.
    RequiredDependencyCycle,
    /// A selected candidate lost its global plugin slot.
    ShadowedPlugin,
    /// Required member is missing or inactive.
    RequiredMemberBlocked,
    /// Explicit local decline retained selection intent.
    DeclinedPlugin,
}

/// Deterministic target-independent passive plugin resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PluginResolution {
    /// Reachable selected and shadowed plugin candidates.
    pub plugins: Vec<ResolvedPlugin>,
    /// Typed graph and coherence diagnostics.
    pub diagnostics: Vec<PluginDiagnostic>,
}

/// Safe read-only candidate summary used by plugin list/show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginCandidate {
    /// Canonical source-qualified identity.
    pub source_ref: String,
    /// Plugin slot name.
    pub slot_name: String,
    /// Optional stable identity.
    pub id: Option<String>,
    /// Human-facing description.
    pub description: String,
    /// Optional authored version.
    pub version: Option<String>,
    /// Package directory.
    pub path: PathBuf,
    /// Complete bounded package hash.
    pub package_hash: String,
    /// Passive members.
    pub members: Vec<PluginMember>,
    /// Inert validated local-tool contracts.
    pub tools: Vec<ToolRecord>,
    /// Inert validated hook contracts.
    pub hooks: Vec<HookRecord>,
    /// Plugin dependencies.
    pub requires: Vec<PluginDependency>,
    /// Provider overlay names; overlay values remain adapter-private.
    pub provider_overlays: Vec<String>,
}

/// Read-only plugin list report including unselected offers and selected state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginListReport {
    /// Every valid candidate in enabled source order, then identity order.
    pub candidates: Vec<PluginCandidate>,
    /// Canonical selected graph.
    pub resolution: PluginResolution,
}

/// Read-only detail for one exact candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginShowReport {
    /// Candidate package metadata.
    pub candidate: PluginCandidate,
    /// Selected state when the candidate is reachable, otherwise `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<ResolvedPlugin>,
}

/// Build a deterministic safe candidate list without exposing provider overlay
/// values that may contain source-authored secrets.
#[must_use]
pub fn list_report(
    sources: &[SourceConfig],
    inventories: &[SourceInventory],
    resolution: PluginResolution,
) -> PluginListReport {
    let priorities = sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| (source.id.as_str(), source.priority))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = inventories
        .iter()
        .flat_map(|inventory| &inventory.plugins)
        .map(candidate_summary)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        priorities
            .get(
                left.source_ref
                    .split_once(':')
                    .map_or("", |(source, _)| source),
            )
            .unwrap_or(&i32::MAX)
            .cmp(
                priorities
                    .get(
                        right
                            .source_ref
                            .split_once(':')
                            .map_or("", |(source, _)| source),
                    )
                    .unwrap_or(&i32::MAX),
            )
            .then_with(|| left.source_ref.cmp(&right.source_ref))
    });
    PluginListReport {
        candidates,
        resolution,
    }
}

/// Build read-only detail for a normalized candidate identity.
#[must_use]
pub fn show_report(
    identity: &str,
    inventories: &[SourceInventory],
    resolution: &PluginResolution,
) -> Option<PluginShowReport> {
    let record = inventories
        .iter()
        .flat_map(|inventory| &inventory.plugins)
        .find(|plugin| plugin.source_ref == identity)?;
    Some(PluginShowReport {
        candidate: candidate_summary(record),
        selected: resolution
            .plugins
            .iter()
            .find(|plugin| plugin.source_ref == identity)
            .cloned(),
    })
}

fn candidate_summary(record: &PluginRecord) -> PluginCandidate {
    PluginCandidate {
        source_ref: record.source_ref.clone(),
        slot_name: record.slot_name.clone(),
        id: record.id.clone(),
        description: record.description.clone(),
        version: record.version.clone(),
        path: record.path.clone(),
        package_hash: record.package_hash.clone(),
        members: record.members.clone(),
        tools: record.tools.clone(),
        hooks: record.hooks.clone(),
        requires: record.requires.clone(),
        provider_overlays: record.providers.keys().cloned().collect(),
    }
}

#[derive(Debug, Clone)]
struct CandidateIndex<'a> {
    by_source: BTreeMap<&'a str, Vec<&'a PluginRecord>>,
    source_priority: BTreeMap<&'a str, i32>,
}

/// Resolve authored stack and direct-user plugin intent without target input.
#[must_use]
pub fn resolve_plugins(config: &UserConfig, inventories: &[SourceInventory]) -> PluginResolution {
    let index = candidate_index(&config.sources, inventories);
    let mut diagnostics = Vec::new();
    let mut roots = BTreeSet::new();
    let mut origins = BTreeMap::<String, BTreeSet<SelectionOrigin>>::new();

    for reference in &config.plugins.direct {
        add_root(
            reference,
            SelectionOrigin {
                kind: SelectionOriginKind::Direct,
                declared_by: "user_config".to_owned(),
                requirement: SelectionStrength::Required,
            },
            &index,
            &mut roots,
            &mut origins,
            &mut diagnostics,
        );
    }
    for source in config.sources.iter().filter(|source| source.enabled) {
        let path = source.path.join(TEAM_MANIFEST_FILE);
        let manifest = match team_manifest::read_manifest(&path) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => continue,
            Err(error) => {
                diagnostics.push(PluginDiagnostic {
                    code: PluginDiagnosticCode::InvalidStackSelection,
                    subject: source.id.clone(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let Some(selection) = manifest.selection else {
            continue;
        };
        if manifest
            .source
            .as_ref()
            .and_then(|metadata| metadata.id.as_deref())
            != Some(source.id.as_str())
        {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::InvalidStackSelection,
                subject: source.id.clone(),
                message: "[source].id must match the configured source when [selection] is present"
                    .to_owned(),
            });
            continue;
        }
        let mut seen = BTreeSet::new();
        for selected in selection.plugins {
            if !seen.insert(selected.reference.clone()) {
                diagnostics.push(PluginDiagnostic {
                    code: PluginDiagnosticCode::InvalidStackSelection,
                    subject: selected.reference,
                    message: "duplicate plugin selection in one manifest".to_owned(),
                });
                continue;
            }
            add_root(
                &selected.reference,
                SelectionOrigin {
                    kind: SelectionOriginKind::Stack,
                    declared_by: source.id.clone(),
                    requirement: match selected.requirement {
                        StackRequirement::Required => SelectionStrength::Required,
                        StackRequirement::Recommended => SelectionStrength::Recommended,
                    },
                },
                &index,
                &mut roots,
                &mut origins,
                &mut diagnostics,
            );
        }
    }

    let mut reachable = roots.clone();
    loop {
        let winners = winner_map(&reachable, &index);
        let mut next = roots.clone();
        let mut queue = winners.values().cloned().collect::<Vec<_>>();
        queue.sort();
        while let Some(identity) = queue.pop() {
            let Some(plugin) = plugin_by_identity(&identity, &index) else {
                continue;
            };
            for dependency in &plugin.requires {
                match resolve_component_reference(&dependency.reference, &plugin.source_id, &index)
                {
                    ReferenceMatch::One(candidate) => {
                        let dependency_id = candidate.source_ref.clone();
                        origins
                            .entry(dependency_id.clone())
                            .or_default()
                            .insert(SelectionOrigin {
                                kind: SelectionOriginKind::Dependency,
                                declared_by: plugin.source_ref.clone(),
                                requirement: match dependency.requirement {
                                    DependencyRequirement::Required => SelectionStrength::Required,
                                    DependencyRequirement::Optional => SelectionStrength::Optional,
                                },
                            });
                        if next.insert(dependency_id.clone()) {
                            queue.push(dependency_id);
                        }
                    }
                    ReferenceMatch::Missing
                        if dependency.requirement == DependencyRequirement::Required =>
                    {
                        diagnostics.push(PluginDiagnostic {
                            code: PluginDiagnosticCode::MissingPlugin,
                            subject: plugin.source_ref.clone(),
                            message: format!(
                                "required dependency `{}` is missing",
                                dependency.reference.as_string()
                            ),
                        });
                    }
                    ReferenceMatch::Ambiguous
                        if dependency.requirement == DependencyRequirement::Required =>
                    {
                        diagnostics.push(PluginDiagnostic {
                            code: PluginDiagnosticCode::AmbiguousPluginReference,
                            subject: plugin.source_ref.clone(),
                            message: format!(
                                "required dependency `{}` is ambiguous",
                                dependency.reference.as_string()
                            ),
                        });
                    }
                    _ => {}
                }
            }
        }
        if next == reachable {
            break;
        }
        reachable = next;
    }

    let winners = winner_map(&reachable, &index);
    let cycles = required_cycle_nodes(&winners, &index);
    for cycle in &cycles {
        diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::RequiredDependencyCycle,
            subject: cycle.join(","),
            message: format!("required plugin dependency cycle: {}", cycle.join(" -> ")),
        });
    }
    let cycle_nodes = cycles.iter().flatten().cloned().collect::<BTreeSet<_>>();
    let mut policies = applied_policies(config, &index, &mut diagnostics);
    let declined = policies.keys().cloned().collect::<BTreeSet<_>>();
    let mut resolved = Vec::new();
    for identity in &reachable {
        let Some(plugin) = plugin_by_identity(identity, &index) else {
            continue;
        };
        let winner = winners.get(&plugin.slot_name).cloned();
        let shadowed_by = winner
            .as_ref()
            .filter(|winner| *winner != identity)
            .cloned();
        let mut blocking_reasons = Vec::new();
        let members = evaluate_members(plugin, &config.sources, inventories, &mut blocking_reasons);
        for reason in &blocking_reasons {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::RequiredMemberBlocked,
                subject: identity.clone(),
                message: reason.clone(),
            });
        }
        let dependencies = plugin
            .requires
            .iter()
            .map(|dependency| {
                let (state, resolved_ref) = match resolve_component_reference(
                    &dependency.reference,
                    &plugin.source_id,
                    &index,
                ) {
                    ReferenceMatch::One(target) => {
                        let state = if winners.get(&target.slot_name) != Some(&target.source_ref) {
                            PluginDependencyState::Shadowed
                        } else if declined.contains(&target.source_ref) {
                            PluginDependencyState::Declined
                        } else {
                            PluginDependencyState::Selected
                        };
                        (state, Some(target.source_ref.clone()))
                    }
                    ReferenceMatch::Missing => (PluginDependencyState::Missing, None),
                    ReferenceMatch::Ambiguous => (PluginDependencyState::Ambiguous, None),
                };
                if dependency.requirement == DependencyRequirement::Required
                    && state != PluginDependencyState::Selected
                {
                    blocking_reasons.push(
                        format!(
                            "required dependency `{}` is {state:?}",
                            dependency.reference.as_string()
                        )
                        .to_lowercase(),
                    );
                }
                ResolvedPluginDependency {
                    reference: dependency.reference.as_string(),
                    requirement: dependency.requirement,
                    state,
                    resolved_ref,
                }
            })
            .collect::<Vec<_>>();
        if cycle_nodes.contains(identity) {
            blocking_reasons.push("required dependency cycle".to_owned());
        }
        let state = if shadowed_by.is_some() {
            PluginState::Shadowed
        } else if policies.contains_key(identity) {
            PluginState::Declined
        } else if blocking_reasons.is_empty() {
            PluginState::Selected
        } else {
            PluginState::Blocked
        };
        if let Some(winner) = &shadowed_by {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::ShadowedPlugin,
                subject: identity.clone(),
                message: format!("selected plugin is shadowed by `{winner}`"),
            });
        }
        let closure_hash = closure_hash(
            plugin,
            &members,
            &dependencies,
            &blocking_reasons,
            state,
            &winners,
            &index,
        );
        resolved.push(ResolvedPlugin {
            source_ref: identity.clone(),
            slot_name: plugin.slot_name.clone(),
            id: plugin.id.clone(),
            source_priority: *index
                .source_priority
                .get(plugin.source_id.as_str())
                .unwrap_or(&i32::MAX),
            package_hash: plugin.package_hash.clone(),
            closure_hash,
            origins: origins
                .remove(identity)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            policies: policies.remove(identity).unwrap_or_default(),
            state,
            shadowed_by,
            members,
            dependencies,
            blocking_reasons,
        });
    }
    resolved.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    diagnostics.sort_by(|left, right| {
        left.subject
            .cmp(&right.subject)
            .then_with(|| format!("{:?}", left.code).cmp(&format!("{:?}", right.code)))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics.dedup();
    PluginResolution {
        plugins: resolved,
        diagnostics,
    }
}

/// Normalize a source-qualified slot or stable-ID selector to canonical plugin
/// identity without changing selection state.
pub fn normalize_plugin_reference(
    config: &UserConfig,
    inventories: &[SourceInventory],
    reference: &str,
) -> Result<String, String> {
    let index = candidate_index(&config.sources, inventories);
    match resolve_qualified(reference, &index) {
        ReferenceMatch::One(plugin) => Ok(plugin.source_ref.clone()),
        ReferenceMatch::Missing => Err(format!(
            "plugin `{reference}` does not exist in the exact configured source"
        )),
        ReferenceMatch::Ambiguous => Err(format!(
            "plugin `{reference}` is ambiguous between a slot and stable ID"
        )),
    }
}

/// Project existing component activation facts onto the canonical plugin graph.
/// This operation never grants approval or enables an instruction pack.
pub fn apply_component_resolution(
    plugins: &mut PluginResolution,
    skills: &Resolution,
    agents: &AgentResolution,
    active_instructions: &BTreeSet<String>,
) {
    for plugin in &mut plugins.plugins {
        let previous_member_states = plugin
            .members
            .iter()
            .map(|member| member.state)
            .collect::<Vec<_>>();
        let previous_state = plugin.state;
        plugin
            .blocking_reasons
            .retain(|reason| !reason.starts_with("required member `"));
        for member in &mut plugin.members {
            let Some(identity) = member.resolved_ref.as_deref() else {
                continue;
            };
            member.state = if member.reference.starts_with("skill:") {
                if skills
                    .active_skills
                    .iter()
                    .any(|skill| skill.source_ref == identity)
                {
                    PluginComponentState::Active
                } else if skills
                    .pending_approval_skills
                    .iter()
                    .any(|skill| skill.source_ref == identity)
                {
                    PluginComponentState::PendingApproval
                } else if skills
                    .blocked_skills
                    .iter()
                    .any(|skill| skill.skill.source_ref == identity)
                {
                    PluginComponentState::Blocked
                } else if skills
                    .unlinked_skills
                    .iter()
                    .any(|skill| skill.skill.source_ref == identity)
                {
                    PluginComponentState::Shadowed
                } else {
                    PluginComponentState::Available
                }
            } else if member.reference.starts_with("agent:") {
                if agents
                    .active_agents
                    .iter()
                    .any(|agent| agent.agent.source_ref == identity)
                {
                    PluginComponentState::Active
                } else if agents
                    .pending_approval_agents
                    .iter()
                    .any(|agent| agent.agent.source_ref == identity)
                {
                    PluginComponentState::PendingApproval
                } else if agents
                    .shadowed_agents
                    .iter()
                    .any(|agent| agent.agent.agent.source_ref == identity)
                {
                    PluginComponentState::Shadowed
                } else {
                    PluginComponentState::Available
                }
            } else if active_instructions.contains(identity) {
                PluginComponentState::Active
            } else {
                PluginComponentState::Inactive
            };
            if member.requirement == MemberRequirement::Required
                && member.state != PluginComponentState::Active
            {
                plugin.blocking_reasons.push(
                    format!(
                        "required member `{}` is {:?}",
                        member.reference, member.state
                    )
                    .to_lowercase(),
                );
            }
        }
        plugin.blocking_reasons.sort();
        plugin.blocking_reasons.dedup();
        if !matches!(plugin.state, PluginState::Shadowed | PluginState::Declined) {
            plugin.state = if plugin.blocking_reasons.is_empty() {
                PluginState::Selected
            } else {
                PluginState::Blocked
            };
        }
        let changed = previous_state != plugin.state
            || previous_member_states
                .iter()
                .zip(&plugin.members)
                .any(|(previous, member)| *previous != member.state);
        if !changed {
            continue;
        }
        let mut hash = Sha256::new();
        hash.update(b"dalo-plugin-activation-closure-v1\0");
        hash.update(&plugin.closure_hash);
        for member in &plugin.members {
            hash.update([0]);
            hash.update(&member.reference);
            hash.update(format!("{:?}{:?}", member.requirement, member.state));
            if let Some(identity) = &member.resolved_ref {
                hash.update(identity);
            }
        }
        plugin.closure_hash = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
    }
}

fn candidate_index<'a>(
    sources: &'a [SourceConfig],
    inventories: &'a [SourceInventory],
) -> CandidateIndex<'a> {
    let enabled = sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| (source.id.as_str(), source.priority))
        .collect::<BTreeMap<_, _>>();
    let mut by_source = BTreeMap::<&str, Vec<&PluginRecord>>::new();
    for inventory in inventories {
        if enabled.contains_key(inventory.source_id.as_str()) {
            by_source
                .entry(inventory.source_id.as_str())
                .or_default()
                .extend(&inventory.plugins);
        }
    }
    for plugins in by_source.values_mut() {
        plugins.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    }
    CandidateIndex {
        by_source,
        source_priority: enabled,
    }
}

enum ReferenceMatch<'a> {
    One(&'a PluginRecord),
    Missing,
    Ambiguous,
}

fn resolve_qualified<'a>(reference: &str, index: &'a CandidateIndex<'a>) -> ReferenceMatch<'a> {
    let mut parts = reference.split(':');
    let (Some(source), Some(selector), None) = (parts.next(), parts.next(), parts.next()) else {
        return ReferenceMatch::Missing;
    };
    resolve_selector(source, selector, index)
}

fn resolve_component_reference<'a>(
    reference: &ComponentReference,
    declaring_source: &str,
    index: &'a CandidateIndex<'a>,
) -> ReferenceMatch<'a> {
    let source = reference.source_id.as_deref().unwrap_or(declaring_source);
    resolve_selector(source, &reference.selector, index)
}

fn resolve_selector<'a>(
    source: &str,
    selector: &str,
    index: &'a CandidateIndex<'a>,
) -> ReferenceMatch<'a> {
    let Some(candidates) = index.by_source.get(source) else {
        return ReferenceMatch::Missing;
    };
    let slot = candidates
        .iter()
        .copied()
        .find(|plugin| plugin.slot_name == selector);
    let stable = candidates
        .iter()
        .copied()
        .find(|plugin| plugin.id.as_deref() == Some(selector));
    match (slot, stable) {
        (Some(left), Some(right)) if left.source_ref != right.source_ref => {
            ReferenceMatch::Ambiguous
        }
        (Some(plugin), _) | (_, Some(plugin)) => ReferenceMatch::One(plugin),
        _ => ReferenceMatch::Missing,
    }
}

fn add_root(
    reference: &str,
    origin: SelectionOrigin,
    index: &CandidateIndex<'_>,
    roots: &mut BTreeSet<String>,
    origins: &mut BTreeMap<String, BTreeSet<SelectionOrigin>>,
    diagnostics: &mut Vec<PluginDiagnostic>,
) {
    match resolve_qualified(reference, index) {
        ReferenceMatch::One(plugin) => {
            roots.insert(plugin.source_ref.clone());
            origins
                .entry(plugin.source_ref.clone())
                .or_default()
                .insert(origin);
        }
        ReferenceMatch::Missing => diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::MissingPlugin,
            subject: reference.to_owned(),
            message: "selected plugin does not exist in the exact source".to_owned(),
        }),
        ReferenceMatch::Ambiguous => diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::AmbiguousPluginReference,
            subject: reference.to_owned(),
            message: "selector matches different slot and stable-ID candidates".to_owned(),
        }),
    }
}

fn plugin_by_identity<'a>(
    identity: &str,
    index: &'a CandidateIndex<'a>,
) -> Option<&'a PluginRecord> {
    index
        .by_source
        .values()
        .flatten()
        .copied()
        .find(|plugin| plugin.source_ref == identity)
}

fn winner_map(
    reachable: &BTreeSet<String>,
    index: &CandidateIndex<'_>,
) -> BTreeMap<String, String> {
    let mut groups = BTreeMap::<String, Vec<&PluginRecord>>::new();
    for identity in reachable {
        if let Some(plugin) = plugin_by_identity(identity, index) {
            groups
                .entry(plugin.slot_name.clone())
                .or_default()
                .push(plugin);
        }
    }
    groups
        .into_iter()
        .map(|(slot, mut plugins)| {
            plugins.sort_by(|left, right| {
                index
                    .source_priority
                    .get(left.source_id.as_str())
                    .unwrap_or(&i32::MAX)
                    .cmp(
                        index
                            .source_priority
                            .get(right.source_id.as_str())
                            .unwrap_or(&i32::MAX),
                    )
                    .then_with(|| left.source_id.cmp(&right.source_id))
            });
            (slot, plugins[0].source_ref.clone())
        })
        .collect()
}

fn required_cycle_nodes(
    winners: &BTreeMap<String, String>,
    index: &CandidateIndex<'_>,
) -> Vec<Vec<String>> {
    let winner_ids = winners.values().cloned().collect::<BTreeSet<_>>();
    let mut graph = BTreeMap::<String, Vec<String>>::new();
    for identity in &winner_ids {
        let Some(plugin) = plugin_by_identity(identity, index) else {
            continue;
        };
        let mut edges = Vec::new();
        for dependency in plugin
            .requires
            .iter()
            .filter(|dependency| dependency.requirement == DependencyRequirement::Required)
        {
            if let ReferenceMatch::One(target) =
                resolve_component_reference(&dependency.reference, &plugin.source_id, index)
                && winner_ids.contains(&target.source_ref)
            {
                edges.push(target.source_ref.clone());
            }
        }
        edges.sort();
        graph.insert(identity.clone(), edges);
    }
    let mut cycles = BTreeSet::<Vec<String>>::new();
    for start in graph.keys() {
        find_cycles(
            start,
            start,
            &graph,
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut cycles,
        );
    }
    cycles.into_iter().collect()
}

fn find_cycles(
    start: &str,
    current: &str,
    graph: &BTreeMap<String, Vec<String>>,
    path: &mut Vec<String>,
    visiting: &mut BTreeSet<String>,
    cycles: &mut BTreeSet<Vec<String>>,
) {
    if !visiting.insert(current.to_owned()) {
        return;
    }
    path.push(current.to_owned());
    for next in graph.get(current).into_iter().flatten() {
        if next == start {
            let mut cycle = path.clone();
            cycle.sort();
            cycle.dedup();
            cycles.insert(cycle);
        } else {
            find_cycles(start, next, graph, path, visiting, cycles);
        }
    }
    path.pop();
    visiting.remove(current);
}

fn applied_policies(
    config: &UserConfig,
    index: &CandidateIndex<'_>,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> BTreeMap<String, Vec<AppliedPluginPolicy>> {
    let mut applied = BTreeMap::<String, Vec<AppliedPluginPolicy>>::new();
    for policy in &config.plugin_policy {
        if policy.decision == PluginPolicyDecision::Decline
            && let ReferenceMatch::One(plugin) = resolve_qualified(&policy.plugin, index)
        {
            applied
                .entry(plugin.source_ref.clone())
                .or_default()
                .push(AppliedPluginPolicy {
                    layer: policy.layer,
                    rule_id: policy.rule_id.clone(),
                    decision: policy.decision,
                    reason: policy.reason.clone(),
                });
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::DeclinedPlugin,
                subject: plugin.source_ref.clone(),
                message: policy.reason.clone(),
            });
        }
    }
    for policies in applied.values_mut() {
        policies.sort_by(|left, right| left.rule_id.cmp(&right.rule_id));
    }
    applied
}

fn evaluate_members(
    plugin: &PluginRecord,
    sources: &[SourceConfig],
    inventories: &[SourceInventory],
    blocking: &mut Vec<String>,
) -> Vec<ResolvedPluginMember> {
    plugin
        .members
        .iter()
        .map(|member| {
            let source = member
                .reference
                .source_id
                .as_deref()
                .unwrap_or(&plugin.source_id);
            let (state, resolved_ref) = match member.reference.kind {
                ComponentKind::Skill => resolve_asset(
                    source,
                    &member.reference.selector,
                    inventories,
                    |inventory| {
                        inventory
                            .skills
                            .iter()
                            .map(|skill| {
                                (
                                    &skill.slot_name,
                                    skill.id.as_ref(),
                                    skill.source_ref.as_str(),
                                )
                            })
                            .collect()
                    },
                ),
                ComponentKind::Agent => resolve_asset(
                    source,
                    &member.reference.selector,
                    inventories,
                    |inventory| {
                        inventory
                            .agents
                            .iter()
                            .map(|agent| {
                                (
                                    &agent.slot_name,
                                    agent.id.as_ref(),
                                    agent.source_ref.as_str(),
                                )
                            })
                            .collect()
                    },
                ),
                ComponentKind::Instruction => {
                    let available = sources
                        .iter()
                        .find(|candidate| candidate.enabled && candidate.id == source)
                        .map(|candidate| {
                            candidate
                                .path
                                .join("instructions")
                                .join(format!("{}.md", member.reference.selector))
                                .is_file()
                        })
                        .unwrap_or(false);
                    if available {
                        (
                            PluginComponentState::Inactive,
                            Some(format!("{source}:{}", member.reference.selector)),
                        )
                    } else {
                        (PluginComponentState::Missing, None)
                    }
                }
                ComponentKind::Plugin => (PluginComponentState::Missing, None),
            };
            if member.requirement == MemberRequirement::Required
                && matches!(
                    state,
                    PluginComponentState::Missing
                        | PluginComponentState::Ambiguous
                        | PluginComponentState::Inactive
                )
            {
                blocking.push(
                    format!(
                        "required member `{}` is {:?}",
                        member.reference.as_string(),
                        state
                    )
                    .to_lowercase(),
                );
            }
            ResolvedPluginMember {
                reference: member.reference.as_string(),
                requirement: member.requirement,
                state,
                resolved_ref,
                fallback: member
                    .fallback
                    .as_ref()
                    .map(|fallback| fallback.skill.as_string()),
            }
        })
        .collect()
}

fn resolve_asset<'a, F>(
    source: &str,
    selector: &str,
    inventories: &'a [SourceInventory],
    collect: F,
) -> (PluginComponentState, Option<String>)
where
    F: Fn(&'a SourceInventory) -> Vec<(&'a String, Option<&'a String>, &'a str)>,
{
    let Some(inventory) = inventories
        .iter()
        .find(|inventory| inventory.source_id == source)
    else {
        return (PluginComponentState::Missing, None);
    };
    let candidates = collect(inventory);
    let slot = candidates
        .iter()
        .find(|(name, _, _)| name.as_str() == selector)
        .map(|(_, _, identity)| *identity);
    let stable = candidates
        .iter()
        .find(|(_, id, _)| id.is_some_and(|id| id == selector))
        .map(|(_, _, identity)| *identity);
    match (slot, stable) {
        (Some(left), Some(right)) if left != right => (PluginComponentState::Ambiguous, None),
        (Some(identity), _) | (_, Some(identity)) => {
            (PluginComponentState::Available, Some(identity.to_owned()))
        }
        _ => (PluginComponentState::Missing, None),
    }
}

fn closure_hash(
    plugin: &PluginRecord,
    members: &[ResolvedPluginMember],
    dependencies: &[ResolvedPluginDependency],
    blocking: &[String],
    state: PluginState,
    winners: &BTreeMap<String, String>,
    index: &CandidateIndex<'_>,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"dalo-plugin-closure-v1\0");
    hash.update(&plugin.source_ref);
    hash.update([0]);
    hash.update(&plugin.package_hash);
    for dependency in &plugin.requires {
        hash.update([0]);
        hash.update(dependency.reference.as_string());
        hash.update(format!("{:?}", dependency.requirement));
        if let ReferenceMatch::One(target) =
            resolve_component_reference(&dependency.reference, &plugin.source_id, index)
        {
            hash.update(&target.source_ref);
            if winners.get(&target.slot_name) == Some(&target.source_ref) {
                hash.update(b"winner");
            }
        }
    }
    for member in members {
        hash.update([0]);
        hash.update(&member.reference);
        hash.update(format!("{:?}{:?}", member.requirement, member.state));
        if let Some(identity) = &member.resolved_ref {
            hash.update(identity);
        }
    }
    for dependency in dependencies {
        hash.update([0]);
        hash.update(&dependency.reference);
        hash.update(format!(
            "{:?}{:?}",
            dependency.requirement, dependency.state
        ));
        if let Some(identity) = &dependency.resolved_ref {
            hash.update(identity);
        }
    }
    for reason in blocking {
        hash.update([0]);
        hash.update(reason);
    }
    hash.update(format!("{:?}", state));
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PluginConfig, Settings};
    use crate::inventory;
    use crate::source::{SourceConfig, SourceKind};
    use tempfile::TempDir;

    fn write_plugin(root: &Path, name: &str, manifest: &str) -> PathBuf {
        let package = root.join("plugins").join(name);
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join(PLUGIN_FILE), manifest).unwrap();
        package
    }

    fn valid_manifest(name: &str) -> String {
        format!(
            r#"schema_version = 1
[plugin]
name = "{name}"
id = "dev.example.{name}"
description = "Example"

[[plugin.members]]
ref = "skill:core"
requirement = "required"

[[plugin.members]]
ref = "agent:reviewer"
requirement = "optional"
[plugin.members.fallback]
kind = "inline"
skill = "skill:core"

[[plugin.requires]]
ref = "plugin:base"
requirement = "optional"
"#
        )
    }

    fn source(id: &str, path: &Path, priority: i32) -> SourceConfig {
        SourceConfig {
            id: id.to_owned(),
            kind: SourceKind::Team,
            path: path.to_path_buf(),
            priority,
            namespace: None,
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

    fn config(sources: Vec<SourceConfig>, direct: &[&str]) -> UserConfig {
        UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources,
            plugins: PluginConfig {
                direct: direct.iter().map(|value| (*value).to_owned()).collect(),
            },
            plugin_policy: Vec::new(),
        }
    }

    #[test]
    fn discovers_exact_valid_packages_and_hashes_support_files() {
        let temp = TempDir::new().unwrap();
        let package = write_plugin(temp.path(), "example", &valid_manifest("example"));
        fs::write(package.join("README.md"), "one").unwrap();
        write_plugin(
            &temp.path().join("nested"),
            "ignored",
            &valid_manifest("ignored"),
        );
        let first = scan_source_plugins("team", temp.path());
        assert_eq!(first.plugins.len(), 1);
        assert!(first.warnings.is_empty());
        fs::write(package.join("README.md"), "two").unwrap();
        let second = scan_source_plugins("team", temp.path());
        assert_ne!(
            first.plugins[0].package_hash,
            second.plugins[0].package_hash
        );
    }

    #[test]
    fn plugin_support_files_are_not_discovered_as_standalone_skills() {
        let temp = TempDir::new().unwrap();
        let package = write_plugin(temp.path(), "example", &valid_manifest("example"));
        let embedded = package.join("embedded");
        fs::create_dir_all(&embedded).unwrap();
        fs::write(embedded.join("SKILL.md"), "# Inert support file\n").unwrap();

        let inventory = inventory::scan_source("team", temp.path()).unwrap();

        assert!(inventory.skills.is_empty());
        assert_eq!(inventory.plugins.len(), 1);
    }

    #[test]
    fn invalid_sibling_does_not_hide_valid_package() {
        let temp = TempDir::new().unwrap();
        write_plugin(temp.path(), "valid", &valid_manifest("valid"));
        write_plugin(temp.path(), "broken", "schema_version = 99\n");
        let inventory = scan_source_plugins("team", temp.path());
        assert_eq!(inventory.plugins.len(), 1);
        assert_eq!(inventory.warnings.len(), 1);
        assert_eq!(
            inventory.warnings[0].code,
            PluginInventoryWarningCode::InvalidPackage
        );
    }

    #[test]
    fn unsupported_hook_descriptor_versions_block_package() {
        let temp = TempDir::new().unwrap();
        let manifest = format!(
            "{}\n[[hook]]\nschema_version = 99\nid = \"future\"\ntool = \"detector\"\nsubject = \"tool_call\"\nphase = \"before\"\neffect = \"allow_deny\"\nrequirement = \"required\"\ntimeout_ms = 2000\nfailure_policy = \"fail_closed\"\nretry = \"never\"\nerror_visibility = \"model_and_user\"\nblocking_scope = \"matched_event\"\n",
            valid_manifest("example")
        );
        write_plugin(temp.path(), "example", &manifest);
        let inventory = scan_source_plugins("team", temp.path());
        assert!(inventory.plugins.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            PluginInventoryWarningCode::UnsupportedActiveComponentSchema
        );
    }

    #[test]
    fn hook_descriptor_binds_an_exact_same_plugin_tool_contract() {
        let temp = TempDir::new().unwrap();
        let manifest = format!(
            r#"{}

[[hook]]
schema_version = 1
id = "protect-bash"
tool = "detector"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
bindings = [{{ input = "path", field = "session.cwd" }}]
matcher = {{ tool_names = ["Bash"] }}
"#,
            tool_manifest("example", "\"${input.path}\"")
        );
        let package = write_plugin(temp.path(), "example", &manifest);
        write_tool_files(&package);

        let inventory = scan_source_plugins("team", temp.path());
        assert!(inventory.warnings.is_empty(), "{:?}", inventory.warnings);
        let hook = &inventory.plugins[0].hooks[0];
        assert_eq!(hook.source_ref, "team:example#hook:protect-bash");
        assert_eq!(hook.tool_source_ref, "team:example#tool:detector");
        assert_eq!(hook.contract_hash.len(), 64);
    }

    fn tool_manifest(name: &str, argv: &str) -> String {
        format!(
            r#"{}
[[tool]]
schema_version = 1
id = "detector"
entry = "bin/detect.py"
runtime = "python"
runtime_version = ">=3.11"
platforms = ["macos", "linux"]
argv = ["--path", {argv}]
files = ["lib/rules.txt"]
cwd = "tool_root"
env = ["DALO_LOG"]
capabilities = ["filesystem_read"]
availability = "required"

[[tool.inputs]]
name = "path"
type = "path"
required = true
"#,
            valid_manifest(name)
        )
    }

    fn write_tool_files(package: &Path) {
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::create_dir_all(package.join("lib")).unwrap();
        fs::write(package.join("bin/detect.py"), b"print('ok')\n").unwrap();
        fs::write(package.join("lib/rules.txt"), b"rules\n").unwrap();
    }

    #[test]
    fn discovers_tool_and_hashes_only_security_relevant_closure() {
        let temp = TempDir::new().unwrap();
        let package = write_plugin(
            temp.path(),
            "example",
            &tool_manifest("example", r#""${input.path}""#),
        );
        write_tool_files(&package);
        fs::write(package.join("README.md"), "one").unwrap();
        let first = scan_source_plugins("team", temp.path());
        assert!(first.warnings.is_empty(), "{:?}", first.warnings);
        assert_eq!(first.plugins[0].tools.len(), 1);
        let contract = first.plugins[0].tools[0].contract_hash.clone();
        let package_hash = first.plugins[0].package_hash.clone();

        fs::write(package.join("README.md"), "two").unwrap();
        let second = scan_source_plugins("team", temp.path());
        assert_eq!(second.plugins[0].tools[0].contract_hash, contract);
        assert_ne!(second.plugins[0].package_hash, package_hash);

        fs::write(package.join("lib/rules.txt"), "changed").unwrap();
        let third = scan_source_plugins("team", temp.path());
        assert_ne!(third.plugins[0].tools[0].contract_hash, contract);
    }

    #[test]
    fn rejects_path_escape_and_partial_or_unknown_placeholders() {
        let temp = TempDir::new().unwrap();
        let package = write_plugin(
            temp.path(),
            "escape",
            &tool_manifest("escape", r#""prefix-${input.path}""#),
        );
        write_tool_files(&package);
        let placeholder = scan_source_plugins("team", temp.path());
        assert!(placeholder.plugins.is_empty());
        assert!(
            placeholder.warnings[0]
                .message
                .contains("unsafe placeholder")
        );

        let package = write_plugin(
            temp.path(),
            "outside",
            &tool_manifest("outside", r#""${input.path}""#)
                .replace("bin/detect.py", "../detect.py"),
        );
        write_tool_files(&package);
        let escaped = scan_source_plugins("team", temp.path());
        assert!(
            escaped
                .warnings
                .iter()
                .any(|warning| warning.path.ends_with("outside")
                    && warning.message.contains("inside"))
        );
    }

    #[test]
    fn descriptor_input_or_argv_drift_invalidates_contract() {
        let temp = TempDir::new().unwrap();
        let package = write_plugin(
            temp.path(),
            "example",
            &tool_manifest("example", r#""${input.path}""#),
        );
        write_tool_files(&package);
        let first = scan_source_plugins("team", temp.path());
        let contract = first.plugins[0].tools[0].contract_hash.clone();
        fs::write(
            package.join(PLUGIN_FILE),
            tool_manifest("example", r#""${input.path}", "--strict""#),
        )
        .unwrap();
        let second = scan_source_plugins("team", temp.path());
        assert_ne!(second.plugins[0].tools[0].contract_hash, contract);
    }

    #[test]
    fn fallback_must_name_required_skill_member() {
        let temp = TempDir::new().unwrap();
        let manifest = valid_manifest("example")
            .replace("requirement = \"required\"", "requirement = \"optional\"");
        write_plugin(temp.path(), "example", &manifest);
        let inventory = scan_source_plugins("team", temp.path());
        assert!(inventory.plugins.is_empty());
        assert!(
            inventory.warnings[0]
                .message
                .contains("required skill member")
        );
    }

    #[test]
    fn duplicate_stable_ids_invalidate_both_packages() {
        let temp = TempDir::new().unwrap();
        let first = valid_manifest("first").replace("dev.example.first", "dev.example.shared");
        let second = valid_manifest("second").replace("dev.example.second", "dev.example.shared");
        write_plugin(temp.path(), "first", &first);
        write_plugin(temp.path(), "second", &second);
        let inventory = scan_source_plugins("team", temp.path());
        assert!(inventory.plugins.is_empty());
        assert_eq!(inventory.warnings.len(), 2);
        assert!(
            inventory
                .warnings
                .iter()
                .all(|warning| warning.code == PluginInventoryWarningCode::DuplicateStableId)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_package_is_blocking() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let package = write_plugin(temp.path(), "example", &valid_manifest("example"));
        symlink(temp.path(), package.join("escape")).unwrap();
        let inventory = scan_source_plugins("team", temp.path());
        assert!(inventory.plugins.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            PluginInventoryWarningCode::UnsafePackageEntry
        );
    }

    #[test]
    fn resolves_exact_cross_source_dependency_and_deterministic_winner() {
        let temp = TempDir::new().unwrap();
        let one = temp.path().join("one");
        let two = temp.path().join("two");
        let root_manifest = valid_manifest("shared")
            .replace("plugin:base", "plugin:two:base")
            .replace("requirement = \"optional\"", "requirement = \"required\"");
        write_plugin(&one, "shared", &root_manifest);
        write_plugin(&two, "base", &valid_manifest("base"));
        write_plugin(&two, "shared", &valid_manifest("shared"));
        let sources = vec![source("one", &one, 20), source("two", &two, 10)];
        let inventories = sources
            .iter()
            .map(|source| inventory::scan_source(&source.id, &source.path).unwrap())
            .collect::<Vec<_>>();
        let config = config(sources, &["one:shared", "two:shared"]);

        let resolution = resolve_plugins(&config, &inventories);

        assert_eq!(resolution.plugins.len(), 3);
        let one_shared = resolution
            .plugins
            .iter()
            .find(|plugin| plugin.source_ref == "one:shared")
            .unwrap();
        assert_eq!(one_shared.state, PluginState::Shadowed);
        assert_eq!(one_shared.shadowed_by.as_deref(), Some("two:shared"));
        assert!(
            resolution
                .plugins
                .iter()
                .any(|plugin| plugin.source_ref == "two:base")
        );
        assert_eq!(
            serde_json::to_string(&resolution).unwrap(),
            serde_json::to_string(&resolve_plugins(&config, &inventories)).unwrap()
        );
    }

    #[test]
    fn required_dependency_cycle_blocks_every_cycle_member() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("team");
        let alpha = valid_manifest("alpha")
            .replace("plugin:base", "plugin:beta")
            .replace("requirement = \"optional\"", "requirement = \"required\"");
        let beta = valid_manifest("beta")
            .replace("plugin:base", "plugin:alpha")
            .replace("requirement = \"optional\"", "requirement = \"required\"");
        write_plugin(&root, "alpha", &alpha);
        write_plugin(&root, "beta", &beta);
        let sources = vec![source("team", &root, 10)];
        let inventories = vec![inventory::scan_source("team", &root).unwrap()];

        let resolution = resolve_plugins(&config(sources, &["team:alpha"]), &inventories);

        assert!(
            resolution
                .plugins
                .iter()
                .all(|plugin| plugin.state == PluginState::Blocked)
        );
        assert_eq!(
            resolution
                .diagnostics
                .iter()
                .filter(
                    |diagnostic| diagnostic.code == PluginDiagnosticCode::RequiredDependencyCycle
                )
                .count(),
            1
        );
    }

    #[test]
    fn slot_stable_id_confusion_is_blocking() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("team");
        let slot = valid_manifest("foo").replace("dev.example.foo", "dev.example.slot");
        let stable = valid_manifest("other").replace("dev.example.other", "foo");
        write_plugin(&root, "foo", &slot);
        write_plugin(&root, "other", &stable);
        let sources = vec![source("team", &root, 10)];
        let inventories = vec![inventory::scan_source("team", &root).unwrap()];

        let resolution = resolve_plugins(&config(sources, &["team:foo"]), &inventories);

        assert!(resolution.plugins.is_empty());
        assert_eq!(
            resolution.diagnostics[0].code,
            PluginDiagnosticCode::AmbiguousPluginReference
        );
    }
}
