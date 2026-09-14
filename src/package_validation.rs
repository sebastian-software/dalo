//! Read-only author validation for portable plugin packages.
//!
//! Validation deliberately has no store or approval dependency. It parses and
//! scans a source tree, reports what can be resolved locally, and keeps provider
//! capability and execution trust as separate facts.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{Settings, UserConfig};
use crate::error::{DaloError, DaloResult};
use crate::hook::{self, HookProvider};
use crate::inventory;
use crate::plugin::{self, ComponentKind, MemberRequirement};
use crate::resolver::{self, ResolutionInput};
use crate::source::{SourceConfig, SourceKind};

/// Version of the machine-readable author-validation report.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Validate a source tree without reading store state or executing package code.
pub fn validate(source_root: &Path) -> DaloResult<PackageValidationReport> {
    validate_with_source_id("validation", source_root)
}

/// Validate a source tree using an explicit source identity for local refs.
///
/// Unqualified references resolve against this identity. References naming any
/// other source remain visible as unavailable external dependencies.
pub fn validate_with_source_id(
    source_id: &str,
    source_root: &Path,
) -> DaloResult<PackageValidationReport> {
    if !crate::source::is_valid_source_id(source_id) {
        return Err(DaloError::InvalidArgument {
            reason: "validation source ID must use Dalo's safe source ID format".to_owned(),
        });
    }
    let metadata =
        fs::symlink_metadata(source_root).map_err(|error| DaloError::InvalidArgument {
            reason: format!(
                "source path `{}` cannot be read: {error}",
                source_root.display()
            ),
        })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(DaloError::InvalidArgument {
            reason: format!(
                "source path `{}` must be an existing directory",
                source_root.display()
            ),
        });
    }

    let plugin_inventory = plugin::scan_source_plugins(source_id, source_root);
    let source_inventory = inventory::scan_source_with_plugin_inventory(
        source_id,
        source_root,
        plugin_inventory.clone(),
    )?;
    let mut diagnostics = Vec::new();
    let mut contract_diagnostics = Vec::new();
    for warning in &plugin_inventory.warnings {
        let diagnostic = ValidationDiagnostic {
            code: warning.code.as_str().to_owned(),
            phase: "package_contract".to_owned(),
            severity: "error".to_owned(),
            path: warning.path.clone(),
            message: warning.message.clone(),
        };
        contract_diagnostics.push(diagnostic.clone());
        diagnostics.push(diagnostic);
    }
    for warning in &source_inventory.warnings {
        let diagnostic = ValidationDiagnostic {
            code: source_warning_code(warning.code),
            phase: "member_resolution".to_owned(),
            severity: "warning".to_owned(),
            path: warning.path.clone(),
            message: warning.message.clone(),
        };
        diagnostics.push(diagnostic);
    }

    let available = AvailableComponents::from_inventory(&source_inventory, source_root);
    let config = UserConfig {
        version: crate::config::CONFIG_VERSION,
        settings: Settings {
            autosync: false,
            sync_interval: None,
        },
        sources: vec![SourceConfig {
            id: source_id.to_owned(),
            kind: SourceKind::Local,
            path: source_root.to_path_buf(),
            priority: 0,
            namespace: None,
            enabled: true,
            trusted: true,
            url: None,
            branch: None,
            update_policy: None,
            selection: Vec::new(),
            declared_by: None,
            declared_ref: None,
        }],
        plugins: crate::config::PluginConfig {
            direct: plugin_inventory
                .plugins
                .iter()
                .map(|package| package.source_ref.clone())
                .collect(),
        },
        plugin_policy: Vec::new(),
    };
    // Run the same graph resolver and component projection used by normal
    // planning. The synthetic config is in-memory and carries no approvals.
    let mut graph = plugin::resolve_plugins(&config, std::slice::from_ref(&source_inventory));
    let component_resolution = resolver::resolve(&ResolutionInput {
        sources: &config.sources,
        inventories: vec![source_inventory.clone()],
        approvals: Vec::new(),
    });
    let agents = crate::agent::resolve_agents(
        &config.sources,
        std::slice::from_ref(&source_inventory),
        &[],
    );
    plugin::apply_component_resolution(
        &mut graph,
        &component_resolution,
        &agents,
        &std::collections::BTreeSet::new(),
    );
    for finding in &graph.diagnostics {
        diagnostics.push(ValidationDiagnostic {
            code: finding.code.to_string(),
            phase: "dependency_resolution".to_owned(),
            severity: "error".to_owned(),
            path: source_root.to_path_buf(),
            message: finding.message.clone(),
        });
    }
    let mut members = Vec::new();
    let mut dependencies = Vec::new();
    let mut required_references_resolved = true;
    for package in &plugin_inventory.plugins {
        for member in &package.members {
            let result = resolve_reference(&available, &member.reference);
            if result.status != "resolved" && member.requirement == MemberRequirement::Required {
                required_references_resolved = false;
            }
            if result.status != "resolved" {
                diagnostics.push(ValidationDiagnostic {
                    code: "unresolved_member".to_owned(),
                    phase: "member_resolution".to_owned(),
                    severity: if member.requirement == MemberRequirement::Required {
                        "error".to_owned()
                    } else {
                        "warning".to_owned()
                    },
                    path: package.manifest_file.clone(),
                    message: result.detail.clone(),
                });
            }
            members.push(ReferenceValidation {
                plugin: package.source_ref.clone(),
                reference: member.reference.as_string(),
                requirement: member.requirement.to_string(),
                status: result.status.to_owned(),
                detail: result.detail,
            });
        }
        for dependency in &package.requires {
            let result = resolve_reference(&available, &dependency.reference);
            if result.status != "resolved"
                && dependency.requirement == crate::plugin::DependencyRequirement::Required
            {
                required_references_resolved = false;
            }
            if result.status != "resolved" {
                diagnostics.push(ValidationDiagnostic {
                    code: "unresolved_dependency".to_owned(),
                    phase: "dependency_resolution".to_owned(),
                    severity: if dependency.requirement
                        == crate::plugin::DependencyRequirement::Required
                    {
                        "error".to_owned()
                    } else {
                        "warning".to_owned()
                    },
                    path: package.manifest_file.clone(),
                    message: result.detail.clone(),
                });
            }
            dependencies.push(ReferenceValidation {
                plugin: package.source_ref.clone(),
                reference: dependency.reference.as_string(),
                requirement: match dependency.requirement {
                    crate::plugin::DependencyRequirement::Required => "required".to_owned(),
                    crate::plugin::DependencyRequirement::Optional => "optional".to_owned(),
                },
                status: result.status.to_owned(),
                detail: result.detail,
            });
        }
    }

    let providers = [HookProvider::Claude, HookProvider::Codex]
        .into_iter()
        .map(|provider| provider_status(&plugin_inventory, provider))
        .collect::<Vec<_>>();
    if plugin_inventory.plugins.is_empty() && plugin_inventory.warnings.is_empty() {
        contract_diagnostics.push(ValidationDiagnostic {
            code: "no_plugin_packages".to_owned(),
            phase: "package_contract".to_owned(),
            severity: "error".to_owned(),
            path: source_root.to_path_buf(),
            message: "source contains no valid plugins/<name>/PLUGIN.toml package".to_owned(),
        });
        diagnostics.push(contract_diagnostics.last().cloned().expect("just pushed"));
    }
    let package_contract = CheckResult {
        valid: contract_diagnostics.is_empty(),
        diagnostics: contract_diagnostics,
    };
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.phase.cmp(&right.phase))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
    Ok(PackageValidationReport {
        schema_version: REPORT_SCHEMA_VERSION,
        profile: "portable-agent-packages/0.1".to_owned(),
        source_path: source_root.to_path_buf(),
        valid: package_contract.valid
            && required_references_resolved
            && graph.diagnostics.is_empty(),
        package_contract,
        members,
        dependencies,
        providers,
        execution_authorization: ExecutionAuthorization {
            status: "never_granted".to_owned(),
            check: "not_applicable".to_owned(),
            detail: "author validation has no trust store and never executes package code"
                .to_owned(),
        },
        packages: plugin_inventory
            .plugins
            .iter()
            .map(ValidatedPackage::from_record)
            .collect(),
        diagnostics,
    })
}

/// Machine-readable validation report. A successful structural check does not
/// A passing package contract does not imply that external references, provider
/// behavior, or execution trust exist.
#[derive(Debug, Clone, Serialize)]
pub struct PackageValidationReport {
    /// Report schema version.
    pub schema_version: u32,
    /// Portable package profile validated.
    pub profile: String,
    /// Source tree inspected.
    pub source_path: PathBuf,
    /// True only when local checks and required references pass.
    pub valid: bool,
    /// Production parser, package-boundary, and cross-field contract checks.
    pub package_contract: CheckResult,
    /// Member references and their local availability.
    pub members: Vec<ReferenceValidation>,
    /// Plugin dependencies and their local availability.
    pub dependencies: Vec<ReferenceValidation>,
    /// Provider adapter capability status at the pinned baselines.
    pub providers: Vec<ProviderCapabilityStatus>,
    /// Explicitly non-authorizing trust result.
    pub execution_authorization: ExecutionAuthorization,
    /// Valid packages discovered in the source tree.
    pub packages: Vec<ValidatedPackage>,
    /// All diagnostics, including unresolved references.
    pub diagnostics: Vec<ValidationDiagnostic>,
}

/// One validation phase result.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    /// Whether every diagnostic in this phase passed.
    pub valid: bool,
    /// Diagnostics belonging to this phase.
    pub diagnostics: Vec<ValidationDiagnostic>,
}

/// One resolved or unavailable component reference.
#[derive(Debug, Clone, Serialize)]
pub struct ReferenceValidation {
    /// Plugin declaring the reference.
    pub plugin: String,
    /// Canonical reference spelling.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Required, optional, or recommended.
    pub requirement: String,
    /// `resolved`, `missing`, or `unavailable_external`.
    pub status: String,
    /// Explanation suitable for author output.
    pub detail: String,
}

/// Provider capability at Dalo's verified adapter baseline.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderCapabilityStatus {
    /// Provider name.
    pub provider: String,
    /// Baseline version used for the capability claim.
    pub baseline: String,
    /// Capability area checked by this report.
    pub scope: String,
    /// `not_applicable`, `supported`, or `unsupported`.
    pub status: String,
    /// Hook identities not preserved by this provider.
    pub unsupported_hooks: Vec<String>,
}

/// Execution trust is intentionally not evaluated by source validation.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionAuthorization {
    /// Always `never_granted` for this command.
    pub status: String,
    /// Always `not_applicable` for this command.
    pub check: String,
    /// Why no authorization claim is made.
    pub detail: String,
}

/// One actionable validator finding.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationDiagnostic {
    /// Stable diagnostic code.
    pub code: String,
    /// Validation stage.
    pub phase: String,
    /// `error` or `warning`.
    pub severity: String,
    /// Related filesystem path.
    pub path: PathBuf,
    /// Actionable detail.
    pub message: String,
}

/// Safe package summary included in validation output.
///
/// Provider overlay values are intentionally reduced to their names because
/// they are inert adapter-owned data and may contain credentials or tokens.
#[derive(Debug, Clone, Serialize)]
pub struct ValidatedPackage {
    /// Source containing the package.
    pub source_id: String,
    /// Canonical package identity.
    pub source_ref: String,
    /// Package slot.
    pub slot_name: String,
    /// Optional stable identity.
    pub id: Option<String>,
    /// Human-facing description.
    pub description: String,
    /// Optional authored version.
    pub version: Option<String>,
    /// Package directory.
    pub path: PathBuf,
    /// Manifest path.
    pub manifest_file: PathBuf,
    /// Passive members.
    pub members: Vec<plugin::PluginMember>,
    /// Local tool contracts.
    pub tools: Vec<plugin::ToolRecord>,
    /// Local hook contracts.
    pub hooks: Vec<plugin::HookRecord>,
    /// Plugin dependencies.
    pub requires: Vec<plugin::PluginDependency>,
    /// Provider overlay names only.
    pub provider_overlays: Vec<String>,
    /// Complete package hash.
    pub package_hash: String,
}

impl ValidatedPackage {
    fn from_record(record: &plugin::PluginRecord) -> Self {
        Self {
            source_id: record.source_id.clone(),
            source_ref: record.source_ref.clone(),
            slot_name: record.slot_name.clone(),
            id: record.id.clone(),
            description: record.description.clone(),
            version: record.version.clone(),
            path: record.path.clone(),
            manifest_file: record.manifest_file.clone(),
            members: record.members.clone(),
            tools: record.tools.clone(),
            hooks: record.hooks.clone(),
            requires: record.requires.clone(),
            provider_overlays: record.providers.keys().cloned().collect(),
            package_hash: record.package_hash.clone(),
        }
    }
}

struct AvailableComponents {
    skills: BTreeMap<String, BTreeSet<String>>,
    agents: BTreeMap<String, BTreeSet<String>>,
    instructions: BTreeMap<String, BTreeSet<String>>,
    plugins: BTreeMap<String, BTreeSet<String>>,
    source_id: String,
}

const MAX_INSTRUCTION_BYTES: u64 = 1024 * 1024;

impl AvailableComponents {
    fn from_inventory(source: &inventory::SourceInventory, source_root: &Path) -> Self {
        let mut skills: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for record in &source.skills {
            skills
                .entry(record.slot_name.clone())
                .or_default()
                .insert(record.source_ref.clone());
            if let Some(id) = &record.id {
                skills
                    .entry(id.clone())
                    .or_default()
                    .insert(record.source_ref.clone());
            }
        }
        let mut agents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for record in &source.agents {
            agents
                .entry(record.slot_name.clone())
                .or_default()
                .insert(record.source_ref.clone());
            if let Some(id) = &record.id {
                agents
                    .entry(id.clone())
                    .or_default()
                    .insert(record.source_ref.clone());
            }
        }
        let mut plugins: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for record in &source.plugins {
            plugins
                .entry(record.slot_name.clone())
                .or_default()
                .insert(record.source_ref.clone());
            if let Some(id) = &record.id {
                plugins
                    .entry(id.clone())
                    .or_default()
                    .insert(record.source_ref.clone());
            }
        }
        let mut instructions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let instructions_root = source_root.join("instructions");
        let root_is_safe = fs::symlink_metadata(&instructions_root)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
        if root_is_safe && let Ok(entries) = fs::read_dir(instructions_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|kind| kind.is_file())
                    && path.extension().and_then(|value| value.to_str()) == Some("md")
                    && bounded_utf8_file(&path)
                    && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
                {
                    instructions
                        .entry(stem.to_owned())
                        .or_default()
                        .insert(path.to_string_lossy().into_owned());
                }
            }
        }
        Self {
            skills,
            agents,
            instructions,
            plugins,
            source_id: source.source_id.clone(),
        }
    }
}

fn bounded_utf8_file(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_INSTRUCTION_BYTES
    {
        return false;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_INSTRUCTION_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    bytes.len() as u64 <= MAX_INSTRUCTION_BYTES && std::str::from_utf8(&bytes).is_ok()
}

struct ReferenceResult {
    status: &'static str,
    detail: String,
}

fn resolve_reference(
    available: &AvailableComponents,
    reference: &plugin::ComponentReference,
) -> ReferenceResult {
    if reference
        .source_id
        .as_deref()
        .is_some_and(|source| source != available.source_id)
    {
        return ReferenceResult {
            status: "unavailable_external",
            detail: format!(
                "{} references source `{}`; external sources are not resolved without a store",
                reference.as_string(),
                reference.source_id.as_deref().unwrap_or_default()
            ),
        };
    }
    let found = match reference.kind {
        ComponentKind::Skill => available.skills.get(&reference.selector),
        ComponentKind::Agent => available.agents.get(&reference.selector),
        ComponentKind::Instruction => available.instructions.get(&reference.selector),
        ComponentKind::Plugin => available.plugins.get(&reference.selector),
    };
    match found {
        Some(candidates) if candidates.len() == 1 => ReferenceResult {
            status: "resolved",
            detail: "resolved in the supplied source tree".to_owned(),
        },
        Some(candidates) => ReferenceResult {
            status: "ambiguous",
            detail: format!(
                "{} matches multiple local components: {}",
                reference.as_string(),
                candidates.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        },
        None => ReferenceResult {
            status: "missing",
            detail: format!(
                "{} is not present in the supplied source tree",
                reference.as_string()
            ),
        },
    }
}

fn provider_status(
    inventory: &plugin::PluginInventory,
    provider: HookProvider,
) -> ProviderCapabilityStatus {
    let unsupported_hooks = inventory
        .plugins
        .iter()
        .flat_map(|package| package.hooks.iter())
        .filter(|hook| !hook::provider_supports_descriptor(provider, &hook.descriptor))
        .map(|hook| hook.source_ref.clone())
        .collect::<Vec<_>>();
    ProviderCapabilityStatus {
        provider: match provider {
            HookProvider::Claude => "claude".to_owned(),
            HookProvider::Codex => "codex".to_owned(),
        },
        baseline: provider.baseline().to_owned(),
        scope: "hooks".to_owned(),
        status: if inventory
            .plugins
            .iter()
            .all(|package| package.hooks.is_empty())
        {
            "not_applicable".to_owned()
        } else if unsupported_hooks.is_empty() {
            "supported".to_owned()
        } else {
            "unsupported".to_owned()
        },
        unsupported_hooks,
    }
}

fn source_warning_code(code: inventory::InventoryWarningCode) -> String {
    format!(
        "source_{}",
        match code {
            inventory::InventoryWarningCode::MalformedFrontmatter => "malformed_frontmatter",
            inventory::InventoryWarningCode::InvalidSlotName => "invalid_slot_name",
            inventory::InventoryWarningCode::DuplicateSlotName => "duplicate_slot_name",
            inventory::InventoryWarningCode::UnreadablePath => "unreadable_path",
            inventory::InventoryWarningCode::SkippedSymlink => "skipped_symlink",
            inventory::InventoryWarningCode::InvalidDelivery => "invalid_delivery",
        }
    )
}
