//! Skill inventory scanning.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::{self, AgentInventoryWarning, AgentRecord};
use crate::error::DaloResult;
use crate::plugin::{
    self, PluginInventoryWarning, PluginRecord, ToolAvailability, ToolCapability, ToolInputType,
    ToolRuntime,
};
use crate::store::ApprovalRecord;

const SKILL_FILE: &str = "SKILL.md";
const DELIVERY_FILE: &str = "DELIVERY.toml";
const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;
const MAX_SKILL_METADATA_BYTES: usize = MAX_FRONTMATTER_BYTES + 16;
const MAX_FRONTMATTER_FLOW_DEPTH: usize = 64;

/// Inventory for one source checkout.
#[derive(Debug, Clone, Serialize)]
pub struct SourceInventory {
    /// Source ID.
    pub source_id: String,
    /// Scanned skills.
    pub skills: Vec<SkillRecord>,
    /// Scanned canonical agent packages.
    pub agents: Vec<AgentRecord>,
    /// Scanned passive portable plugins.
    pub plugins: Vec<PluginRecord>,
    /// Non-fatal scan warnings.
    pub warnings: Vec<InventoryWarning>,
    /// Non-fatal canonical-agent package warnings.
    pub agent_warnings: Vec<AgentInventoryWarning>,
    /// Non-fatal portable-plugin package warnings.
    pub plugin_warnings: Vec<PluginInventoryWarning>,
}

/// One discovered skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRecord {
    /// Source ID.
    pub source_id: String,
    /// Source-qualified ref, `<source-id>:<slot-name>`.
    pub source_ref: String,
    /// Stable frontmatter ID when present.
    pub id: Option<String>,
    /// Physical install slot name.
    pub slot_name: String,
    /// Skill directory path.
    pub path: PathBuf,
    /// `SKILL.md` path.
    pub skill_file: PathBuf,
    /// Target-aware delivery strategy.
    pub delivery: SkillDelivery,
    /// Optional description.
    pub description: Option<String>,
    /// Declared dependencies.
    pub requires: Vec<String>,
    /// Declared owners.
    pub owners: Vec<String>,
    /// Declared tags.
    pub tags: Vec<String>,
}

/// How one logical skill is delivered to linked targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillDelivery {
    /// Link the discovered skill directory unchanged.
    Direct,
    /// Select an existing provider-specific directory from the source checkout.
    Prebuilt {
        /// Provider artifacts keyed by logical target ID.
        providers: BTreeMap<String, PrebuiltSkillArtifact>,
        /// Whether an unmapped target may use the logical skill directory.
        universal_fallback: bool,
        /// Fingerprint of the fallback directory when enabled.
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback_fingerprint: Option<String>,
        /// Delivery manifest path used as provenance.
        manifest_path: PathBuf,
    },
    /// Describe an explicitly approved, content-bound generator recipe.
    Generated {
        /// Same-source plugin-local generator tool identity.
        generator: String,
        /// Exact invocation-contract hash of the generator tool.
        generator_contract_hash: String,
        /// Required path input that will receive a Dalo-owned staging root.
        output_input: String,
        /// Expected provider outputs relative to the Dalo-owned staging root.
        providers: BTreeMap<String, PathBuf>,
        /// Content-bound generator recipe identity.
        recipe_hash: String,
        /// Delivery manifest path used as provenance.
        manifest_path: PathBuf,
        /// Immutable source revision bound during resolution.
        #[serde(skip_serializing_if = "Option::is_none")]
        source_commit: Option<String>,
        /// Whether the exact source revision and recipe were approved.
        recipe_approved: bool,
        /// Whether the exact generator tool contract has execution approval.
        generator_approved: bool,
        /// Audited output fingerprints keyed by provider after materialization.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        output_fingerprints: BTreeMap<String, String>,
        /// Content-addressed derivation identity after materialization.
        #[serde(skip_serializing_if = "Option::is_none")]
        derivation_hash: Option<String>,
    },
}

impl SkillDelivery {
    /// Select the immutable directory and fingerprint for one logical target.
    #[must_use]
    pub fn artifact_for(&self, target_id: &str, direct_path: &Path) -> Option<SkillArtifact> {
        match self {
            Self::Direct => Some(SkillArtifact {
                path: direct_path.to_path_buf(),
                fingerprint: None,
                mode: SkillDeliveryMode::Direct,
                provider: None,
            }),
            Self::Prebuilt {
                providers,
                universal_fallback,
                fallback_fingerprint,
                ..
            } => providers.get(target_id).map_or_else(
                || {
                    universal_fallback.then(|| SkillArtifact {
                        path: direct_path.to_path_buf(),
                        fingerprint: fallback_fingerprint.clone(),
                        mode: SkillDeliveryMode::Prebuilt,
                        provider: Some("universal".to_owned()),
                    })
                },
                |artifact| {
                    Some(SkillArtifact {
                        path: artifact.path.clone(),
                        fingerprint: Some(artifact.fingerprint.clone()),
                        mode: SkillDeliveryMode::Prebuilt,
                        provider: Some(target_id.to_owned()),
                    })
                },
            ),
            Self::Generated { .. } => None,
        }
    }

    /// Stable delivery mode label.
    #[must_use]
    pub const fn mode(&self) -> SkillDeliveryMode {
        match self {
            Self::Direct => SkillDeliveryMode::Direct,
            Self::Prebuilt { .. } => SkillDeliveryMode::Prebuilt,
            Self::Generated { .. } => SkillDeliveryMode::Generated,
        }
    }

    /// Bind a generated recipe to the current source revision and local approvals.
    pub fn bind_generated_approvals(
        &mut self,
        source_ref: &str,
        stable_ref: &str,
        source_commit: Option<String>,
        approvals: &[ApprovalRecord],
    ) {
        let Self::Generated {
            generator,
            generator_contract_hash,
            recipe_hash,
            source_commit: bound_commit,
            recipe_approved,
            generator_approved,
            ..
        } = self
        else {
            return;
        };
        *bound_commit = source_commit;
        *recipe_approved = bound_commit.as_deref().is_some_and(|commit| {
            let value = generated_approval_value(source_ref, stable_ref, commit, recipe_hash);
            approvals
                .iter()
                .any(|record| record.scope == "delivery" && record.value == value)
        });
        let tool_value = format!("{generator}@sha256:{generator_contract_hash}");
        *generator_approved = approvals
            .iter()
            .any(|record| record.scope == "tool" && record.value == tool_value);
    }

    /// Exact content- and revision-bound approval value for a generated recipe.
    #[must_use]
    pub fn generated_approval_value(&self, source_ref: &str, stable_ref: &str) -> Option<String> {
        let Self::Generated {
            recipe_hash,
            source_commit: Some(source_commit),
            ..
        } = self
        else {
            return None;
        };
        Some(generated_approval_value(
            source_ref,
            stable_ref,
            source_commit,
            recipe_hash,
        ))
    }
}

fn generated_approval_value(
    source_ref: &str,
    stable_ref: &str,
    source_commit: &str,
    recipe_hash: &str,
) -> String {
    format!("{source_ref}@id:{stable_ref}@{source_commit}@sha256:{recipe_hash}")
}

/// Selected delivery artifact for one logical target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillArtifact {
    /// Directory linked into the target.
    pub path: PathBuf,
    /// Deterministic content fingerprint.
    pub fingerprint: Option<String>,
    /// Delivery mode.
    pub mode: SkillDeliveryMode,
    /// Provider mapping key, or `universal` for an explicit fallback.
    pub provider: Option<String>,
}

/// Stable skill delivery mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillDeliveryMode {
    /// The canonical skill directory is linked unchanged.
    Direct,
    /// A declared provider-specific prebuilt directory is linked.
    Prebuilt,
    /// An approved generator produces audited immutable provider artifacts.
    Generated,
}

impl SkillDeliveryMode {
    /// Text label used in persisted provenance.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Prebuilt => "prebuilt",
            Self::Generated => "generated",
        }
    }
}

/// One validated prebuilt provider artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrebuiltSkillArtifact {
    /// Absolute directory path inside the source checkout.
    pub path: PathBuf,
    /// Deterministic content fingerprint.
    pub fingerprint: String,
}

/// Non-fatal inventory warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryWarning {
    /// Warning code.
    pub code: InventoryWarningCode,
    /// Path related to the warning.
    pub path: PathBuf,
    /// Human-readable message.
    pub message: String,
}

/// Inventory warning code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryWarningCode {
    /// Frontmatter exists but could not be parsed.
    MalformedFrontmatter,
    /// Frontmatter name could not be used as a slot name.
    InvalidSlotName,
    /// Multiple skills in the same source have the same slot name.
    DuplicateSlotName,
    /// A skill path could not be read.
    UnreadablePath,
    /// A symlinked directory was skipped to avoid traversing outside the source
    /// or looping through a cycle.
    SkippedSymlink,
    /// A delivery manifest or provider artifact was invalid or unsafe.
    InvalidDelivery,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryManifest {
    schema_version: u32,
    kind: String,
    #[serde(default)]
    universal_fallback: bool,
    #[serde(default)]
    providers: BTreeMap<String, PathBuf>,
    generator: Option<String>,
    output_input: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SkillFrontmatter {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    requires: Vec<String>,
    owners: Vec<String>,
    tags: Vec<String>,
}

/// Scan a source checkout for skills.
pub fn scan_source(source_id: &str, source_root: &Path) -> DaloResult<SourceInventory> {
    let agent_inventory = agent::scan_source_agents(source_id, source_root);
    let plugin_inventory = plugin::scan_source_plugins(source_id, source_root);
    let mut warnings = Vec::new();
    let skill_dirs = find_skill_dirs(source_root, &mut warnings)?;
    let mut skills = Vec::new();

    for skill_dir in skill_dirs {
        match scan_skill(
            source_id,
            source_root,
            &skill_dir,
            &plugin_inventory.plugins,
        ) {
            Ok((skill, mut skill_warnings)) => {
                // `skill` is `None` when the slot name could not be resolved; the
                // skill is dropped while its warning is still collected.
                if let Some(skill) = skill {
                    skills.push(skill);
                }
                warnings.append(&mut skill_warnings);
            }
            Err(error) => warnings.push(InventoryWarning {
                code: InventoryWarningCode::UnreadablePath,
                path: skill_dir,
                message: error.to_string(),
            }),
        }
    }

    // Provider builds are artifacts of another logical skill, not additional
    // independently selectable skills even when they contain their own SKILL.md.
    let provider_artifact_paths = skills
        .iter()
        .flat_map(|skill| match &skill.delivery {
            SkillDelivery::Prebuilt { providers, .. } => providers
                .values()
                .map(|artifact| artifact.path.clone())
                .collect::<Vec<_>>(),
            SkillDelivery::Direct | SkillDelivery::Generated { .. } => Vec::new(),
        })
        .collect::<std::collections::BTreeSet<_>>();
    skills.retain(|skill| !provider_artifact_paths.contains(&skill.path));
    warnings.retain(|warning| {
        !provider_artifact_paths
            .iter()
            .any(|artifact| warning.path.starts_with(artifact))
    });

    skills.sort_by(|left, right| {
        left.slot_name
            .cmp(&right.slot_name)
            .then_with(|| left.source_ref.cmp(&right.source_ref))
            .then_with(|| left.path.cmp(&right.path))
    });
    warnings.extend(duplicate_slot_warnings(source_id, &skills));
    warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| warning_code_name(left.code).cmp(warning_code_name(right.code)))
    });

    Ok(SourceInventory {
        source_id: source_id.to_owned(),
        skills,
        agents: agent_inventory.agents,
        plugins: plugin_inventory.plugins,
        warnings,
        agent_warnings: agent_inventory.warnings,
        plugin_warnings: plugin_inventory.warnings,
    })
}

fn find_skill_dirs(
    source_root: &Path,
    warnings: &mut Vec<InventoryWarning>,
) -> DaloResult<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![source_root.to_path_buf()];
    let plugins_root = source_root.join("plugins");
    let canonical_source_root = fs::canonicalize(source_root).ok();

    while let Some(dir) = pending.pop() {
        if dir == plugins_root {
            // Plugin-owned support files are inert inventory and must never be
            // rediscovered as standalone managed skills.
            continue;
        }
        if dir.file_name().is_some_and(|name| name == ".git") {
            continue;
        }

        let skill_file = dir.join(SKILL_FILE);
        if skill_file.is_file() {
            let metadata_is_symlink = fs::symlink_metadata(&skill_file)
                .is_ok_and(|metadata| metadata.file_type().is_symlink());
            if metadata_is_symlink
                && !canonical_source_root.as_ref().is_some_and(|source_root| {
                    skill_file
                        .canonicalize()
                        .is_ok_and(|target| target.starts_with(source_root))
                })
            {
                warnings.push(InventoryWarning {
                    code: InventoryWarningCode::SkippedSymlink,
                    path: skill_file,
                    message:
                        "skipped symlinked SKILL.md whose target is outside the source checkout"
                            .to_owned(),
                });
                continue;
            }
            found.push(dir);
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(InventoryWarning {
                    code: InventoryWarningCode::UnreadablePath,
                    path: dir,
                    message: error.to_string(),
                });
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(InventoryWarning {
                        code: InventoryWarningCode::UnreadablePath,
                        path: dir.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(InventoryWarning {
                        code: InventoryWarningCode::UnreadablePath,
                        path: entry.path(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            if file_type.is_symlink() {
                // Regular-file symlinks cannot contain a skill subtree, so they
                // are irrelevant to directory discovery. Repositories commonly
                // alias instruction files (for example CLAUDE.md -> AGENTS.md),
                // and treating those as a degraded skill inventory would make
                // otherwise compatible catalogs permanently unhealthy.
                if fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file()) {
                    continue;
                }
                warnings.push(InventoryWarning {
                    code: InventoryWarningCode::SkippedSymlink,
                    path: entry.path(),
                    message: "skipped symlink to keep the source scan bounded".to_owned(),
                });
            } else if file_type.is_dir() && !is_adoption_staging_dir_name(&entry.file_name()) {
                pending.push(entry.path());
            }
        }
    }

    found.sort();
    Ok(found)
}

fn is_adoption_staging_dir_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    rest.rsplit_once(".dalo-adopting-")
        .is_some_and(|(skill_name, suffix)| !skill_name.is_empty() && !suffix.is_empty())
}

fn scan_skill(
    source_id: &str,
    source_root: &Path,
    skill_dir: &Path,
    plugins: &[PluginRecord],
) -> DaloResult<(Option<SkillRecord>, Vec<InventoryWarning>)> {
    let skill_file = skill_dir.join(SKILL_FILE);
    let (skill_markdown, metadata_truncated) = read_skill_metadata(&skill_file)?;
    let (frontmatter, mut warnings) =
        parse_frontmatter(&skill_markdown, &skill_file, metadata_truncated);
    let Some(frontmatter) = frontmatter else {
        // Metadata participates in stable identity, approvals, and required
        // closure. Never silently activate a skill after losing those fields.
        return Ok((None, warnings));
    };
    let folder_name = skill_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_id.to_owned());
    let Some(slot_name) = select_slot_name(&frontmatter, &folder_name, &skill_file, &mut warnings)
    else {
        // Neither the front-matter name nor the folder name is a usable slot
        // name; drop the skill but keep the warning so callers can surface it.
        return Ok((None, warnings));
    };
    let source_ref = format!("{source_id}:{slot_name}");
    let delivery = match scan_delivery(
        source_id,
        &source_ref,
        frontmatter.id.as_deref(),
        source_root,
        skill_dir,
        plugins,
    ) {
        Ok(delivery) => delivery,
        Err(message) => {
            warnings.push(InventoryWarning {
                code: InventoryWarningCode::InvalidDelivery,
                path: skill_dir.join(DELIVERY_FILE),
                message,
            });
            return Ok((None, warnings));
        }
    };

    Ok((
        Some(SkillRecord {
            source_id: source_id.to_owned(),
            source_ref,
            id: frontmatter.id,
            slot_name,
            path: skill_dir.to_path_buf(),
            skill_file,
            delivery,
            description: frontmatter.description,
            requires: frontmatter.requires,
            owners: frontmatter.owners,
            tags: frontmatter.tags,
        }),
        warnings,
    ))
}

fn scan_delivery(
    source_id: &str,
    source_ref: &str,
    skill_id: Option<&str>,
    source_root: &Path,
    skill_dir: &Path,
    plugins: &[PluginRecord],
) -> Result<SkillDelivery, String> {
    let manifest_path = skill_dir.join(DELIVERY_FILE);
    if !manifest_path.exists() {
        return Ok(SkillDelivery::Direct);
    }
    let manifest_metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|error| format!("cannot inspect delivery manifest: {error}"))?;
    if !manifest_metadata.is_file() || manifest_metadata.file_type().is_symlink() {
        return Err("delivery manifest must be a regular file".to_owned());
    }

    let content = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read delivery manifest: {error}"))?;
    let manifest: DeliveryManifest = toml::from_str(&content)
        .map_err(|error| format!("cannot parse delivery manifest: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported delivery schema version {} (supported: 1)",
            manifest.schema_version
        ));
    }
    match manifest.kind.as_str() {
        "prebuilt" => scan_prebuilt_delivery(source_root, skill_dir, manifest_path, manifest),
        "generated" => scan_generated_delivery(
            source_id,
            source_ref,
            skill_id,
            manifest_path,
            manifest,
            plugins,
        ),
        kind => Err(format!(
            "unsupported delivery kind `{kind}` (supported: prebuilt, generated)"
        )),
    }
}

fn scan_prebuilt_delivery(
    source_root: &Path,
    skill_dir: &Path,
    manifest_path: PathBuf,
    manifest: DeliveryManifest,
) -> Result<SkillDelivery, String> {
    if manifest.generator.is_some() || manifest.output_input.is_some() {
        return Err("prebuilt delivery must not declare generator fields".to_owned());
    }
    if manifest.providers.is_empty() {
        return Err("prebuilt delivery requires at least one provider mapping".to_owned());
    }

    let canonical_source = fs::canonicalize(source_root)
        .map_err(|error| format!("cannot resolve source checkout: {error}"))?;
    let canonical_skill = fs::canonicalize(skill_dir)
        .map_err(|error| format!("cannot resolve logical skill directory: {error}"))?;
    let mut providers: BTreeMap<String, PrebuiltSkillArtifact> = BTreeMap::new();
    for (provider, relative_path) in manifest.providers {
        if provider == "universal" {
            return Err(
                "provider target ID `universal` is reserved for universal_fallback".to_owned(),
            );
        }
        if !valid_provider_id(&provider) {
            return Err(format!("invalid provider target ID `{provider}`"));
        }
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(format!(
                "provider `{provider}` path must be relative and stay inside the source checkout"
            ));
        }
        let artifact_path = source_root.join(&relative_path);
        let canonical_artifact = fs::canonicalize(&artifact_path).map_err(|error| {
            format!(
                "provider `{provider}` artifact `{}` cannot be resolved: {error}",
                relative_path.display()
            )
        })?;
        if !canonical_artifact.starts_with(&canonical_source) {
            return Err(format!(
                "provider `{provider}` artifact must stay inside the source checkout"
            ));
        }
        if canonical_artifact == canonical_skill {
            return Err(format!(
                "provider `{provider}` artifact must differ from the logical skill; use universal_fallback for the canonical directory"
            ));
        }
        if !canonical_artifact.join(SKILL_FILE).is_file() {
            return Err(format!(
                "provider `{provider}` artifact `{}` does not contain SKILL.md",
                relative_path.display()
            ));
        }
        let fingerprint = fingerprint_directory(&artifact_path)
            .map_err(|error| format!("provider `{provider}` artifact is unsafe: {error}"))?;
        providers.insert(
            provider,
            PrebuiltSkillArtifact {
                path: artifact_path,
                fingerprint,
            },
        );
    }

    let fallback_fingerprint = manifest
        .universal_fallback
        .then(|| fingerprint_directory(skill_dir))
        .transpose()?;
    Ok(SkillDelivery::Prebuilt {
        providers,
        universal_fallback: manifest.universal_fallback,
        fallback_fingerprint,
        manifest_path,
    })
}

fn scan_generated_delivery(
    source_id: &str,
    source_ref: &str,
    skill_id: Option<&str>,
    manifest_path: PathBuf,
    manifest: DeliveryManifest,
    plugins: &[PluginRecord],
) -> Result<SkillDelivery, String> {
    if skill_id.is_none() {
        return Err("generated delivery requires a stable skill frontmatter `id`".to_owned());
    }
    if manifest.universal_fallback {
        return Err("generated delivery does not support universal_fallback".to_owned());
    }
    if manifest.providers.is_empty() {
        return Err("generated delivery requires at least one provider output mapping".to_owned());
    }
    let generator = manifest
        .generator
        .ok_or_else(|| "generated delivery requires `generator`".to_owned())?;
    if !generator.starts_with(&format!("{source_id}:")) {
        return Err("generated delivery generator must be a tool from the same source".to_owned());
    }
    let (plugin, tool) = plugins
        .iter()
        .find_map(|plugin| {
            plugin
                .tools
                .iter()
                .find(|tool| tool.source_ref == generator)
                .map(|tool| (plugin, tool))
        })
        .ok_or_else(|| format!("generated delivery references unknown tool `{generator}`"))?;
    if tool.runtime != ToolRuntime::Executable {
        return Err(format!(
            "generator `{generator}` must use the executable runtime so sync never selects an interpreter from ambient PATH"
        ));
    }
    validate_generated_entry(&generator, &plugin.path.join(&tool.entry))?;
    let output_input = manifest
        .output_input
        .ok_or_else(|| "generated delivery requires `output_input`".to_owned())?;
    let input = tool
        .inputs
        .iter()
        .find(|input| input.name == output_input)
        .ok_or_else(|| {
            format!("generator `{generator}` does not declare output input `{output_input}`")
        })?;
    if input.kind != ToolInputType::Path || !input.required {
        return Err(format!(
            "generator output input `{output_input}` must be a required path"
        ));
    }
    if let Some(other) = tool
        .inputs
        .iter()
        .find(|input| input.required && input.name != output_input)
    {
        return Err(format!(
            "generator `{generator}` has unsupported required input `{}`; generated delivery v1 supplies only `{output_input}`",
            other.name
        ));
    }
    let placeholder = format!("${{input.{output_input}}}");
    if !tool.argv.iter().any(|value| value == &placeholder) {
        return Err(format!(
            "generator `{generator}` argv must include `{placeholder}`"
        ));
    }
    if !tool.capabilities.contains(&ToolCapability::FilesystemWrite) {
        return Err(format!(
            "generator `{generator}` must declare the filesystem_write capability"
        ));
    }
    if tool.availability != ToolAvailability::Required {
        return Err(format!(
            "generator `{generator}` must declare required availability"
        ));
    }

    let mut providers: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (provider, relative_path) in manifest.providers {
        if provider == "universal" || !valid_provider_id(&provider) {
            return Err(format!("invalid generated provider target ID `{provider}`"));
        }
        validate_relative_generated_path(&provider, &relative_path)?;
        if let Some((other_provider, other_path)) = providers.iter().find(|(_, other_path)| {
            relative_path.starts_with(other_path) || other_path.starts_with(&relative_path)
        }) {
            return Err(format!(
                "generated provider outputs `{provider}` ({}) and `{other_provider}` ({}) must not overlap",
                relative_path.display(),
                other_path.display()
            ));
        }
        providers.insert(provider, relative_path);
    }
    let recipe_hash = generated_recipe_hash(
        source_ref,
        &generator,
        &tool.contract_hash,
        &output_input,
        &providers,
    );
    Ok(SkillDelivery::Generated {
        generator,
        generator_contract_hash: tool.contract_hash.clone(),
        output_input,
        providers,
        recipe_hash,
        manifest_path,
        source_commit: None,
        recipe_approved: false,
        generator_approved: false,
        output_fingerprints: BTreeMap::new(),
        derivation_hash: None,
    })
}

fn validate_generated_entry(generator: &str, entry: &Path) -> Result<(), String> {
    let bytes = fs::read(entry)
        .map_err(|error| format!("cannot inspect generator `{generator}` entry: {error}"))?;
    let Some(shebang) = bytes.strip_prefix(b"#!") else {
        return Ok(());
    };
    let line = shebang
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let interpreter = line
        .split(|byte| byte.is_ascii_whitespace())
        .find(|part| !part.is_empty())
        .and_then(|part| std::str::from_utf8(part).ok())
        .unwrap_or_default();
    if !interpreter.starts_with('/') || interpreter == "/usr/bin/env" {
        return Err(format!(
            "generator `{generator}` executable entry must use an absolute shebang and must not select an interpreter through /usr/bin/env"
        ));
    }
    Ok(())
}

fn validate_relative_generated_path(provider: &str, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path == Path::new(".")
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "generated provider `{provider}` output must be a non-empty relative path inside the staging root"
        ));
    }
    Ok(())
}

fn generated_recipe_hash(
    source_ref: &str,
    generator: &str,
    generator_contract_hash: &str,
    output_input: &str,
    providers: &BTreeMap<String, PathBuf>,
) -> String {
    let mut hash = Sha256::new();
    for value in [
        "dalo-generated-delivery-v1",
        source_ref,
        generator,
        generator_contract_hash,
        output_input,
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    for (provider, path) in providers {
        hash.update((provider.len() as u64).to_be_bytes());
        hash.update(provider.as_bytes());
        let path = path.as_os_str().as_encoded_bytes();
        hash.update((path.len() as u64).to_be_bytes());
        hash.update(path);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn fingerprint_directory(root: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect `{}`: {error}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("`{}` must be a real directory", root.display()));
    }

    let mut entries = Vec::new();
    collect_fingerprint_entries(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hash = Sha256::new();
    for (relative, kind, mode, content) in entries {
        hash.update(relative.as_os_str().as_encoded_bytes());
        hash.update([0]);
        hash.update([kind]);
        hash.update(mode.to_be_bytes());
        hash.update((content.len() as u64).to_be_bytes());
        hash.update(content);
    }
    let digest = hash.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("sha256:{hex}"))
}

fn collect_fingerprint_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(PathBuf, u8, u32, Vec<u8>)>,
) -> Result<(), String> {
    let children = fs::read_dir(directory)
        .map_err(|error| format!("cannot read `{}`: {error}", directory.display()))?;
    for child in children {
        let child = child.map_err(|error| {
            format!("cannot read entry below `{}`: {error}", directory.display())
        })?;
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .expect("descendant must stay below fingerprint root")
            .to_path_buf();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect `{}`: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("symlink `{}` is not allowed", relative.display()));
        }
        if metadata.is_dir() {
            entries.push((relative, b'd', 0, Vec::new()));
            collect_fingerprint_entries(root, &path, entries)?;
        } else if metadata.is_file() {
            let content = fs::read(&path)
                .map_err(|error| format!("cannot read `{}`: {error}", path.display()))?;
            entries.push((
                relative,
                b'f',
                metadata.permissions().mode() & 0o111,
                content,
            ));
        } else {
            return Err(format!(
                "special filesystem entry `{}` is not allowed",
                relative.display()
            ));
        }
    }
    Ok(())
}

/// Return an invalid-slot warning for one skill directory, if present.
///
/// This reuses the source-inventory parser so callers that copy skills into a
/// managed source can reject names which the next sync would otherwise reject.
pub(crate) fn invalid_slot_name_warning(skill_dir: &Path) -> DaloResult<Option<InventoryWarning>> {
    let (_, warnings) = scan_skill("adopt", skill_dir, skill_dir, &[])?;
    Ok(warnings
        .into_iter()
        .find(|warning| warning.code == InventoryWarningCode::InvalidSlotName))
}

fn read_skill_metadata(path: &Path) -> io::Result<(String, bool)> {
    let file = fs::File::open(path)?;
    let metadata_truncated = file.metadata()?.len() > MAX_SKILL_METADATA_BYTES as u64;
    let mut bytes = Vec::with_capacity(MAX_SKILL_METADATA_BYTES);
    file.take(MAX_SKILL_METADATA_BYTES as u64)
        .read_to_end(&mut bytes)?;
    let markdown = match String::from_utf8(bytes) {
        Ok(markdown) => markdown,
        Err(error) if metadata_truncated && error.utf8_error().error_len().is_none() => {
            let valid_up_to = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid_up_to);
            String::from_utf8(bytes).expect("validated UTF-8 prefix should remain valid")
        }
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    };
    Ok((markdown, metadata_truncated))
}

fn parse_frontmatter(
    markdown: &str,
    path: &Path,
    metadata_truncated: bool,
) -> (Option<SkillFrontmatter>, Vec<InventoryWarning>) {
    let mut warnings = Vec::new();
    // Accept both LF and CRLF after the opening `---` fence so skills authored
    // on Windows parse the same as Unix ones.
    let opened = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"));
    let Some(rest) = opened else {
        return (Some(SkillFrontmatter::default()), warnings);
    };

    let Some(end_index) = frontmatter_end_index(rest) else {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: if metadata_truncated {
                format!("frontmatter exceeds the {MAX_FRONTMATTER_BYTES}-byte safety limit")
            } else {
                "frontmatter start marker has no matching end marker".to_owned()
            },
        });
        return (None, warnings);
    };

    let frontmatter = &rest[..end_index];
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: format!("frontmatter exceeds the {MAX_FRONTMATTER_BYTES}-byte safety limit"),
        });
        return (None, warnings);
    }
    if frontmatter_flow_depth_exceeds(frontmatter, MAX_FRONTMATTER_FLOW_DEPTH) {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: format!(
                "frontmatter flow nesting exceeds the {MAX_FRONTMATTER_FLOW_DEPTH}-level safety limit"
            ),
        });
        return (None, warnings);
    }
    match yaml_serde::from_str(frontmatter) {
        Ok(frontmatter) => (Some(frontmatter), warnings),
        Err(error) => {
            warnings.push(InventoryWarning {
                code: InventoryWarningCode::MalformedFrontmatter,
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            (None, warnings)
        }
    }
}

// Reject pathological flow collections before `yaml_serde` builds its event
// tree. This is intentionally a small lexical guard, not a second YAML parser;
// quoted scalars and comments cannot introduce structural nesting.
fn frontmatter_flow_depth_exceeds(frontmatter: &str, limit: usize) -> bool {
    let structural_frontmatter = frontmatter_without_block_scalar_bodies(frontmatter);
    let mut chars = structural_frontmatter.chars().peekable();
    let mut depth = 0_usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    let mut in_comment = false;
    let mut previous = None;

    while let Some(character) = chars.next() {
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            previous = Some(character);
            continue;
        }
        if in_single_quote {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_single_quote = false;
                }
            }
            previous = Some(character);
            continue;
        }
        if in_double_quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_double_quote = false;
            }
            previous = Some(character);
            continue;
        }

        match character {
            '#' if previous.is_none_or(char::is_whitespace) => in_comment = true,
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '[' | '{' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        previous = Some(character);
    }

    false
}

fn frontmatter_without_block_scalar_bodies(frontmatter: &str) -> String {
    let mut structural = String::with_capacity(frontmatter.len());
    let mut block_scalar_indent = None;

    for line in frontmatter.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let trimmed = content.trim_start_matches(' ');
        let indent = content.len() - trimmed.len();

        if let Some(header_indent) = block_scalar_indent {
            if trimmed.is_empty() || indent > header_indent {
                if line.ends_with('\n') {
                    structural.push('\n');
                }
                continue;
            }
            block_scalar_indent = None;
        }

        structural.push_str(line);
        if is_block_scalar_header(trimmed) {
            block_scalar_indent = Some(indent);
        }
    }

    structural
}

fn is_block_scalar_header(line: &str) -> bool {
    if line.starts_with('#') {
        return false;
    }
    let Some((_, value)) = line.split_once(':') else {
        return false;
    };
    let value = value.trim_start();
    let Some(indicator) = value.chars().next() else {
        return false;
    };
    if !matches!(indicator, '|' | '>') {
        return false;
    }
    value[indicator.len_utf8()..]
        .split('#')
        .next()
        .is_some_and(|suffix| {
            suffix
                .trim()
                .chars()
                .all(|character| matches!(character, '+' | '-' | '1'..='9'))
        })
}

fn frontmatter_end_index(rest: &str) -> Option<usize> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let line_without_cr = line_without_newline
            .strip_suffix('\r')
            .unwrap_or(line_without_newline);
        if line_without_cr == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Resolve the slot name for a skill, or `None` when the skill must be skipped.
///
/// The front-matter `name` wins when valid. Otherwise the directory name is the
/// fallback, but it has to clear the same `is_valid_slot_name` bar because it
/// also becomes a path component under each target (a dir like `.config` would
/// otherwise create `~/.claude/skills/.config`). An invalid fallback yields an
/// `InvalidSlotName` warning and a `None`, so the caller drops the skill.
fn select_slot_name(
    frontmatter: &SkillFrontmatter,
    folder_name: &str,
    path: &Path,
    warnings: &mut Vec<InventoryWarning>,
) -> Option<String> {
    if let Some(name) = frontmatter.name.as_deref() {
        let trimmed = name.trim();
        if is_valid_slot_name(trimmed) {
            return Some(trimmed.to_owned());
        }

        warnings.push(InventoryWarning {
            code: InventoryWarningCode::InvalidSlotName,
            path: path.to_path_buf(),
            message: format!("frontmatter name `{name}` is not a valid slot name"),
        });
    }

    if is_valid_slot_name(folder_name) {
        return Some(folder_name.to_owned());
    }

    warnings.push(InventoryWarning {
        code: InventoryWarningCode::InvalidSlotName,
        path: path.to_path_buf(),
        message: format!("folder name `{folder_name}` is not a valid slot name"),
    });
    None
}

/// Whether a slot name is portable as a materialization path component.
pub(crate) fn is_valid_slot_name(value: &str) -> bool {
    // A slot name becomes a single path component under each target directory,
    // so keep the accepted language conservative and cross-platform: lowercase
    // ASCII tokens only, no hidden/traversal segments, no trailing dots, and no
    // Windows device basenames.
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.starts_with('.')
        || value.ends_with('.')
        || is_windows_reserved_basename(value)
    {
        return false;
    }

    value.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'
            || character == '_'
            || character == '.'
    })
}

fn is_windows_reserved_basename(value: &str) -> bool {
    let basename = value.split('.').next().unwrap_or(value);
    matches!(
        basename,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn duplicate_slot_warnings(source_id: &str, skills: &[SkillRecord]) -> Vec<InventoryWarning> {
    let mut paths_by_slot: BTreeMap<&str, Vec<&Path>> = BTreeMap::new();
    for skill in skills {
        paths_by_slot
            .entry(&skill.slot_name)
            .or_default()
            .push(&skill.path);
    }

    paths_by_slot
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .flat_map(|(slot_name, paths)| {
            paths.into_iter().map(move |path| InventoryWarning {
                code: InventoryWarningCode::DuplicateSlotName,
                path: path.to_path_buf(),
                message: format!(
                    "source `{source_id}` contains multiple skills with slot name `{slot_name}`"
                ),
            })
        })
        .collect()
}

fn warning_code_name(code: InventoryWarningCode) -> &'static str {
    match code {
        InventoryWarningCode::MalformedFrontmatter => "malformed_frontmatter",
        InventoryWarningCode::InvalidSlotName => "invalid_slot_name",
        InventoryWarningCode::DuplicateSlotName => "duplicate_slot_name",
        InventoryWarningCode::UnreadablePath => "unreadable_path",
        InventoryWarningCode::SkippedSymlink => "skipped_symlink",
        InventoryWarningCode::InvalidDelivery => "invalid_delivery",
    }
}

impl std::fmt::Display for InventoryWarningCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(warning_code_name(*self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn write_generator_plugin(root: &Path) {
        let package = root.join("plugins/builder");
        fs::create_dir_all(package.join("bin")).unwrap();
        let entry = package.join("bin/build.sh");
        fs::write(&entry, "#!/bin/sh\nprintf 'not executed\\n'\n").unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            package.join("PLUGIN.toml"),
            r#"schema_version = 1
[plugin]
name = "builder"
description = "Inert generator fixture"

[[tool]]
schema_version = 1
id = "build"
entry = "bin/build.sh"
runtime = "executable"
platforms = ["macos", "linux"]
argv = ["${input.output_dir}"]
cwd = "tool_root"
capabilities = ["filesystem_write"]
availability = "required"

[[tool.inputs]]
name = "output_dir"
type = "path"
required = true
"#,
        )
        .unwrap();
    }

    #[test]
    fn scan_source_should_validate_generated_recipe_without_creating_outputs() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("skills/impeccable");
        fs::create_dir_all(&logical).unwrap();
        fs::write(
            logical.join(SKILL_FILE),
            "---\nid: impeccable.skill\n---\n# Impeccable\n",
        )
        .unwrap();
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"generated\"\ngenerator = \"company:builder#tool:build\"\noutput_input = \"output_dir\"\n\n[providers]\ncodex = \"codex/impeccable\"\nclaude = \"claude/impeccable\"\n",
        )
        .unwrap();
        write_generator_plugin(temp_dir.path());

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        let SkillDelivery::Generated {
            generator,
            generator_contract_hash,
            providers,
            recipe_hash,
            recipe_approved,
            generator_approved,
            ..
        } = &inventory.skills[0].delivery
        else {
            panic!("logical skill should retain an inert generated recipe");
        };
        assert_eq!(generator, "company:builder#tool:build");
        assert_eq!(generator_contract_hash.len(), 64);
        assert_eq!(providers["codex"], PathBuf::from("codex/impeccable"));
        assert_eq!(recipe_hash.len(), 64);
        assert!(!recipe_approved);
        assert!(!generator_approved);
        assert!(!temp_dir.path().join("codex/impeccable").exists());
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_reject_generated_recipe_without_bounded_output_input() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("review");
        fs::create_dir_all(&logical).unwrap();
        fs::write(
            logical.join(SKILL_FILE),
            "---\nid: review.skill\n---\n# Review\n",
        )
        .unwrap();
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"generated\"\ngenerator = \"company:builder#tool:build\"\noutput_input = \"missing\"\n\n[providers]\ncodex = \"codex/review\"\n",
        )
        .unwrap();
        write_generator_plugin(temp_dir.path());

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning
                    .message
                    .contains("does not declare output input `missing`")
        }));
    }

    #[test]
    fn scan_source_should_reject_generated_recipe_without_stable_skill_id() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("review");
        fs::create_dir_all(&logical).unwrap();
        fs::write(logical.join(SKILL_FILE), "# Review\n").unwrap();
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"generated\"\ngenerator = \"company:builder#tool:build\"\noutput_input = \"output_dir\"\n\n[providers]\ncodex = \"codex/review\"\n",
        )
        .unwrap();
        write_generator_plugin(temp_dir.path());

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning
                    .message
                    .contains("requires a stable skill frontmatter `id`")
        }));
    }

    #[test]
    fn scan_source_should_reject_overlapping_generated_provider_outputs() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("skills/review");
        fs::create_dir_all(&logical).unwrap();
        fs::write(
            logical.join(SKILL_FILE),
            "---\nid: review.skill\n---\n# Review\n",
        )
        .unwrap();
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"generated\"\ngenerator = \"company:builder#tool:build\"\noutput_input = \"output_dir\"\n\n[providers]\ncodex = \"shared\"\nclaude = \"shared/review\"\n",
        )
        .unwrap();
        write_generator_plugin(temp_dir.path());

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning.message.contains("must not overlap")
        }));
    }

    #[test]
    fn scan_source_should_reject_generated_runtime_selected_from_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("skills/review");
        fs::create_dir_all(&logical).unwrap();
        fs::write(
            logical.join(SKILL_FILE),
            "---\nid: review.skill\n---\n# Review\n",
        )
        .unwrap();
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"generated\"\ngenerator = \"company:builder#tool:build\"\noutput_input = \"output_dir\"\n\n[providers]\ncodex = \"codex/review\"\n",
        )
        .unwrap();
        write_generator_plugin(temp_dir.path());
        let manifest = temp_dir.path().join("plugins/builder/PLUGIN.toml");
        let content = fs::read_to_string(&manifest)
            .unwrap()
            .replace("runtime = \"executable\"", "runtime = \"python\"");
        fs::write(manifest, content).unwrap();

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning.message.contains("ambient PATH")
        }));
    }

    #[test]
    fn scan_source_should_reject_env_shebang_for_generated_entry() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("skills/review");
        fs::create_dir_all(&logical).unwrap();
        fs::write(
            logical.join(SKILL_FILE),
            "---\nid: review.skill\n---\n# Review\n",
        )
        .unwrap();
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"generated\"\ngenerator = \"company:builder#tool:build\"\noutput_input = \"output_dir\"\n\n[providers]\ncodex = \"codex/review\"\n",
        )
        .unwrap();
        write_generator_plugin(temp_dir.path());
        let entry = temp_dir.path().join("plugins/builder/bin/build.sh");
        fs::write(&entry, "#!/usr/bin/env sh\nprintf 'unsafe\\n'\n").unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning.message.contains("must not select an interpreter")
        }));
    }

    #[test]
    fn scan_source_should_resolve_prebuilt_provider_artifacts_once() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("skills/impeccable");
        let codex = temp_dir.path().join("builds/codex/impeccable");
        let claude = temp_dir.path().join("builds/claude/impeccable");
        for directory in [&logical, &codex, &claude] {
            fs::create_dir_all(directory).expect("skill directory should be created");
            fs::write(directory.join(SKILL_FILE), "# Impeccable\n")
                .expect("skill file should be written");
        }
        fs::write(
            claude.join(SKILL_FILE),
            "---\nprovider-specific: [frontmatter\n---\n# Claude Impeccable\n",
        )
        .expect("provider-specific skill file should be written");
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"prebuilt\"\n\n[providers]\ncodex = \"builds/codex/impeccable\"\nclaude = \"builds/claude/impeccable\"\n",
        )
        .expect("delivery manifest should be written");

        let inventory = scan_source("catalog", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        let SkillDelivery::Prebuilt { providers, .. } = &inventory.skills[0].delivery else {
            panic!("logical skill should use prebuilt delivery");
        };
        assert_eq!(providers["codex"].path, codex);
        assert_eq!(providers["claude"].path, claude);
        assert!(providers["codex"].fingerprint.starts_with("sha256:"));
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_fail_closed_for_unsafe_prebuilt_artifact() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let outside = tempfile::tempdir().expect("outside tempdir should be created");
        let logical = temp_dir.path().join("review");
        fs::create_dir_all(&logical).expect("skill directory should be created");
        fs::write(logical.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        fs::write(outside.path().join(SKILL_FILE), "# Outside\n")
            .expect("outside skill should be written");
        std::os::unix::fs::symlink(outside.path(), temp_dir.path().join("escaped"))
            .expect("escape symlink should be created");
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"prebuilt\"\n\n[providers]\ncodex = \"escaped\"\n",
        )
        .expect("delivery manifest should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning.message.contains("inside the source checkout")
        }));
    }

    #[test]
    fn scan_source_should_reject_a_non_regular_delivery_manifest_without_opening_it() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("review");
        fs::create_dir_all(logical.join(DELIVERY_FILE))
            .expect("directory-shaped manifest should be created");
        fs::write(logical.join(SKILL_FILE), "# Review\n").expect("skill should be written");

        let inventory = scan_source("local", temp_dir.path()).expect("scan should complete");

        assert!(inventory.skills.is_empty());
        assert!(inventory.warnings.iter().any(|warning| {
            warning.code == InventoryWarningCode::InvalidDelivery
                && warning.message.contains("must be a regular file")
        }));
    }

    #[test]
    fn delivery_manifest_should_reserve_universal_for_the_fallback_artifact() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let logical = temp_dir.path().join("review");
        let universal = temp_dir.path().join("builds/universal/review");
        for directory in [&logical, &universal] {
            fs::create_dir_all(directory).expect("skill directory should be created");
            fs::write(directory.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        }
        fs::write(
            logical.join(DELIVERY_FILE),
            "schema_version = 1\nkind = \"prebuilt\"\nuniversal_fallback = true\n\n[providers]\nuniversal = \"builds/universal/review\"\n",
        )
        .expect("delivery manifest should be written");

        let error = scan_delivery(
            "local",
            "local:review",
            None,
            temp_dir.path(),
            &logical,
            &[],
        )
        .expect_err("the fallback identity must not collide with a provider mapping");

        assert!(error.contains("`universal` is reserved"));
    }

    #[test]
    fn scan_source_should_find_skill_with_frontmatter() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("copy-editing");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nid: team.copy-editing\nname: copy-editing\ndescription: Edit copy\nrequires:\n  - style-guide\nowners:\n  - docs\ntags:\n  - writing\n---\n# Copy Editing\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        let skill = &inventory.skills[0];
        assert_eq!(skill.source_ref, "company:copy-editing");
        assert_eq!(skill.id.as_deref(), Some("team.copy-editing"));
        assert_eq!(skill.requires, ["style-guide".to_owned()]);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_parse_yaml_frontmatter_and_preserve_dependencies() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("app");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nid: example.app\nname: app\ndescription: >-\n  A valid folded YAML description.\nrequires: [missing-base]\nowners:\n  - \"team docs\"\nextra:\n  nested: accepted\n---\n# App\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].id.as_deref(), Some("example.app"));
        assert_eq!(inventory.skills[0].requires, ["missing-base"]);
        assert_eq!(
            inventory.skills[0].description.as_deref(),
            Some("A valid folded YAML description.")
        );
    }

    #[test]
    fn scan_source_should_parse_yaml_description_scalar_styles() {
        let cases = [
            ("literal", "|", "First line.\nSecond line.\n"),
            ("folded", ">", "First line. Second line.\n"),
            ("stripped", ">-", "First line. Second line."),
        ];

        for (name, style, expected) in cases {
            let temp_dir = tempfile::tempdir().expect("tempdir should be created");
            let skill_dir = temp_dir.path().join(name);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                format!(
                    "---\nname: {name}\ndescription: {style}\n  First line.\n  Second line.\n---\n# Skill\n"
                ),
            )
            .expect("skill file should be written");

            let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

            assert_eq!(inventory.skills[0].description.as_deref(), Some(expected));
            assert!(inventory.warnings.is_empty());
        }
    }

    #[test]
    fn scan_source_should_parse_plain_and_quoted_description_scalars() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        for (name, description) in [("plain", "Plain text"), ("quoted", "\"Quoted text\"")] {
            let skill_dir = temp_dir.path().join(name);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                format!("---\nname: {name}\ndescription: {description}\n---\n# Skill\n"),
            )
            .expect("skill file should be written");
        }

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(
            inventory.skills[0].description.as_deref(),
            Some("Plain text")
        );
        assert_eq!(
            inventory.skills[1].description.as_deref(),
            Some("Quoted text")
        );
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_warn_when_a_skill_directory_is_symlinked() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        let shared_skill = temp_dir.path().join("shared-review");
        fs::create_dir_all(&source_root).expect("source root should be created");
        fs::create_dir_all(&shared_skill).expect("shared skill should be created");
        fs::write(
            shared_skill.join(SKILL_FILE),
            "---\nname: review\n---\n# Review\n",
        )
        .expect("skill file should be written");
        std::os::unix::fs::symlink(&shared_skill, source_root.join("review"))
            .expect("skill directory should be linked");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(inventory.warnings.len(), 1);
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::SkippedSymlink
        );
    }

    #[test]
    fn scan_source_should_ignore_regular_file_symlinks() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        fs::create_dir_all(source_root.join("skills/review"))
            .expect("skill directory should be created");
        fs::write(source_root.join("AGENTS.md"), "# Instructions\n")
            .expect("instruction file should be written");
        std::os::unix::fs::symlink("AGENTS.md", source_root.join("CLAUDE.md"))
            .expect("instruction alias should be linked");
        fs::write(source_root.join("skills/review/SKILL.md"), "# Review\n")
            .expect("skill file should be written");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_warn_when_skill_metadata_symlink_escapes_source() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        let external_skill = temp_dir.path().join("external-review");
        fs::create_dir_all(source_root.join("skills/review"))
            .expect("skill directory should be created");
        fs::create_dir_all(&external_skill).expect("external skill directory should be created");
        fs::write(
            external_skill.join(SKILL_FILE),
            "---\nname: external-review\n---\n# External Review\n",
        )
        .expect("external skill file should be written");
        std::os::unix::fs::symlink(
            external_skill.join(SKILL_FILE),
            source_root.join("skills/review").join(SKILL_FILE),
        )
        .expect("skill metadata symlink should be created");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(inventory.warnings.len(), 1);
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::SkippedSymlink
        );
        assert_eq!(
            inventory.warnings[0].path,
            source_root.join("skills/review").join(SKILL_FILE)
        );
    }

    #[test]
    fn scan_source_should_allow_skill_metadata_symlink_inside_source() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        let metadata = source_root.join("metadata/review.md");
        let skill_dir = source_root.join("skills/review");
        fs::create_dir_all(metadata.parent().expect("metadata should have a parent"))
            .expect("metadata directory should be created");
        fs::create_dir_all(&skill_dir).expect("skill directory should be created");
        fs::write(&metadata, "---\nname: review\n---\n# Review\n")
            .expect("metadata should be written");
        std::os::unix::fs::symlink(&metadata, skill_dir.join(SKILL_FILE))
            .expect("skill metadata symlink should be created");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_warn_when_a_skill_symlink_is_broken() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        fs::create_dir_all(&source_root).expect("source root should be created");
        std::os::unix::fs::symlink(
            temp_dir.path().join("missing-review"),
            source_root.join("review"),
        )
        .expect("broken skill symlink should be created");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(inventory.warnings.len(), 1);
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::SkippedSymlink
        );
        assert_eq!(inventory.warnings[0].path, source_root.join("review"));
    }

    #[test]
    fn scan_source_should_ignore_hidden_adoption_debris() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        for directory in ["review", ".review.dalo-adopting-interrupted"] {
            let skill_dir = source_root.join(directory);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                "---\nname: review\n---\n# Review\n",
            )
            .expect("skill file should be written");
        }

        let inventory = scan_source("local", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].path, source_root.join("review"));
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_allow_skills_in_hidden_directories() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        let skill_dir = source_root.join("tools/.review");
        fs::create_dir_all(&skill_dir).expect("hidden skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: review\n---\n# Review\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].path, skill_dir);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_skip_malformed_frontmatter() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("app");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: [unterminated\n---\n# App\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::MalformedFrontmatter
        );
    }

    #[test]
    fn scan_source_should_reject_oversized_frontmatter_before_parsing() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("oversized");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!(
                "---\nname: oversized\ndescription: {}\n---\n# Oversized\n",
                "x".repeat(MAX_FRONTMATTER_BYTES)
            ),
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::MalformedFrontmatter
        );
        assert!(inventory.warnings[0].message.contains("byte safety limit"));
    }

    #[test]
    fn scan_source_should_not_read_skill_body_beyond_the_metadata_window() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("bounded");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        let mut markdown = b"---\nname: bounded\n---\n# Bounded\n".to_vec();
        markdown.extend("€".repeat(MAX_SKILL_METADATA_BYTES).as_bytes());
        markdown.push(0xff);
        fs::write(skill_dir.join(SKILL_FILE), markdown).expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].slot_name, "bounded");
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_reject_deep_flow_nesting_before_parsing() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("nested");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        let nesting = MAX_FRONTMATTER_FLOW_DEPTH + 1;
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!(
                "---\nname: nested\ntags: {}value{}\n---\n# Nested\n",
                "[".repeat(nesting),
                "]".repeat(nesting)
            ),
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::MalformedFrontmatter
        );
        assert!(
            inventory.warnings[0]
                .message
                .contains("flow nesting exceeds")
        );
    }

    #[test]
    fn frontmatter_flow_depth_guard_should_ignore_quotes_and_comments() {
        let delimiters = "[".repeat(MAX_FRONTMATTER_FLOW_DEPTH + 1);
        let frontmatter =
            format!("description: \"{delimiters}\"\nowner: '{delimiters}'\n# {delimiters}\n");

        assert!(!frontmatter_flow_depth_exceeds(
            &frontmatter,
            MAX_FRONTMATTER_FLOW_DEPTH
        ));
    }

    #[test]
    fn scan_source_should_allow_flow_delimiters_in_block_scalar_text() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("block-scalar");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        let delimiters = "[".repeat(MAX_FRONTMATTER_FLOW_DEPTH + 1);
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!(
                "---\nname: block-scalar\ndescription: |-\n  {delimiters}\n---\n# Block Scalar\n"
            ),
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(
            inventory.skills[0].description.as_deref(),
            Some(delimiters.as_str())
        );
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_require_frontmatter_end_fence_line() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("workflow");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: workflow\nrequires:\n  - setup\nnotes: |\n  --- divider\n  ---- not a fence\nowners:\n  - team\n---\n# Workflow\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        let skill = &inventory.skills[0];
        assert_eq!(skill.requires, ["setup".to_owned()]);
        assert_eq!(skill.owners, ["team".to_owned()]);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_fallback_to_folder_name_when_frontmatter_name_is_invalid() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("release-notes.local");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: release notes\n---\n# Release Notes\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("local", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].slot_name, "release-notes.local");
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn scan_source_should_report_duplicate_slot_names() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        for dir_name in ["first", "second"] {
            let skill_dir = temp_dir.path().join(dir_name);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                "---\nname: shared\n---\n# Shared\n",
            )
            .expect("skill file should be written");
        }

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");
        let duplicate_warnings = inventory
            .warnings
            .iter()
            .filter(|warning| warning.code == InventoryWarningCode::DuplicateSlotName)
            .count();

        assert_eq!(duplicate_warnings, 2);
    }

    #[test]
    fn scan_source_should_treat_supporting_files_as_part_of_one_skill() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# Review\n").expect("skill file should be written");
        fs::write(skill_dir.join("guide.md"), "supporting").expect("guide should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        // Supporting files live next to `SKILL.md`; they must not spawn extra skill
        // records. Content fingerprints over those files return in V1.1 (drift
        // detection), persisted into the user lock.
        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].source_ref, "company:review");
    }

    #[test]
    fn is_valid_slot_name_should_reject_dot_segments() {
        assert!(!is_valid_slot_name("."));
        assert!(!is_valid_slot_name(".."));
        assert!(!is_valid_slot_name(".config"));
        assert!(!is_valid_slot_name("review."));
    }

    #[test]
    fn is_valid_slot_name_should_reject_non_portable_names() {
        let invalid_names = [
            "Review",
            "review copy",
            "review\ncopy",
            "caf\u{e9}",
            "cafe\u{301}",
            "con",
            "con.skill",
            "aux",
            "nul",
            "com1",
            "lpt9",
        ];

        for name in invalid_names {
            assert!(!is_valid_slot_name(name), "{name} should be invalid");
        }
    }

    #[test]
    fn is_valid_slot_name_should_accept_cross_platform_tokens() {
        for name in ["review", "release-notes.local", "copy_editing", "skill.123"] {
            assert!(is_valid_slot_name(name), "{name} should be valid");
        }
    }

    proptest! {
        #[test]
        fn valid_slot_names_should_stay_portable(value in "\\PC{0,64}") {
            if is_valid_slot_name(&value) {
                prop_assert!(!value.is_empty());
                prop_assert_ne!(value.as_str(), ".");
                prop_assert_ne!(value.as_str(), "..");
                prop_assert!(!value.starts_with('.'));
                prop_assert!(!value.ends_with('.'));
                prop_assert!(!is_windows_reserved_basename(&value));
                let portable = value.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '-'
                        || character == '_'
                        || character == '.'
                });
                prop_assert!(portable);
            }
        }
    }

    #[test]
    fn scan_source_should_skip_skill_when_folder_name_is_invalid() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        // No front-matter `name`, so the slot name falls back to the folder name;
        // the space makes it an invalid slot name, so the skill must be dropped.
        let skill_dir = temp_dir.path().join("bad name");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# No Frontmatter Name\n")
            .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn scan_source_should_skip_uppercase_folder_name() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("Review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# Review\n").expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
        assert!(inventory.warnings[0].message.contains("Review"));
    }

    #[test]
    fn scan_source_should_skip_unicode_folder_name() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("caf\u{e9}");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# Cafe\n").expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn select_slot_name_should_return_none_when_folder_name_is_invalid() {
        let frontmatter = SkillFrontmatter::default();
        let mut warnings = Vec::new();

        let slot_name = select_slot_name(
            &frontmatter,
            "bad name",
            Path::new("/tmp/bad name/SKILL.md"),
            &mut warnings,
        );

        assert!(slot_name.is_none());
        assert_eq!(warnings[0].code, InventoryWarningCode::InvalidSlotName);
    }

    #[test]
    fn scan_source_should_fallback_when_frontmatter_name_has_case_collision_risk() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: Review\n---\n# Review\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].slot_name, "review");
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn scan_source_should_reject_traversal_frontmatter_name() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("legit");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "---\nname: ..\n---\n# Legit\n")
            .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].slot_name, "legit");
    }

    #[test]
    fn scan_source_should_parse_crlf_frontmatter() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("copy-editing");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\r\nname: copy-editing\r\nid: team.copy-editing\r\n---\r\n# Copy Editing\r\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].id.as_deref(), Some("team.copy-editing"));
    }
}
